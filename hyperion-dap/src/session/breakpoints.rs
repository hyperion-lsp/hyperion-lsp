// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Line breakpoints.
//!
//! A source file is matched to classes through the `SourceFile` attribute the
//! compiler writes into every class file (`Main.java`, `Server.kt`), and a
//! line is matched through each method's line table. That covers everything
//! one file compiles to: top-level Kotlin functions in `FooKt`, companion and
//! nested classes, lambdas (synthetic methods), and Java anonymous, local and
//! inner classes. Classes that are not loaded yet are picked up by a
//! `ClassPrepare` request filtered on the same `SourceFile` name.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use hyperion_jdwp::commands::{self, Command, EventModifier, SUSPEND_ALL, SUSPEND_EVENT_THREAD};
use hyperion_jdwp::replies::{self, LoadedClass};

use super::state::{BreakpointEntry, SourceBreakpoints};
use super::DebugSession;
use crate::types::{
    Breakpoint, BreakpointEventBody, SetBreakpointsArguments, SetBreakpointsResponseBody,
};

/// How far past a line without code a breakpoint may slide to the next line
/// that has some (a comment, a blank line, a declaration).
const MAX_LINE_SLIDE: u64 = 10;

/// Prefixes of classes that can never come from user source; skipped when the
/// package of the source file is unknown.
const SYSTEM_PREFIXES: &[&str] = &["Ljava/", "Ljavax/", "Ljdk/", "Lsun/", "Lcom/sun/", "["];

/// What the adapter needs to know about a source file besides its lines.
pub(super) struct SourceFacts {
    /// JVM package in internal form (`com/example`; empty for the default package).
    pub package: String,
    /// Line ranges (1-based, inclusive) of every class-like body in the file.
    pub type_ranges: Vec<(u64, u64)>,
}

/// Read the facts of a Java or Kotlin file. `None` if unreadable.
pub(super) fn read_source_facts(path: &Path) -> Option<SourceFacts> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(SourceFacts {
        package: package_of_source(&text),
        type_ranges: type_body_ranges(&text),
    })
}

/// Line ranges of class, interface, enum, record, object and anonymous-class
/// bodies, found by matching braces (skipping strings and comments). Used to
/// keep a breakpoint that slides to the next line with code inside the type
/// it was set in: a type that is not loaded yet cannot provide that line.
pub(super) fn type_body_ranges(text: &str) -> Vec<(u64, u64)> {
    let chars: Vec<char> = text.chars().collect();
    let mut ranges = Vec::new();
    // (is a type body, line of the opening brace)
    let mut stack: Vec<(bool, u64)> = Vec::new();
    let mut prefix = String::new();
    let mut line = 1u64;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match (c, next) {
            ('\n', _) => {
                line += 1;
                prefix.push(' ');
            }
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            ('/', Some('*')) => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
                i += 2;
                continue;
            }
            ('"', _) => {
                // Text block or ordinary string.
                let block = chars.get(i + 1) == Some(&'"') && chars.get(i + 2) == Some(&'"');
                i += if block { 3 } else { 1 };
                while i < chars.len() {
                    if chars[i] == '\n' {
                        line += 1;
                        if !block {
                            break;
                        }
                    }
                    if chars[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if chars[i] == '"' {
                        if !block {
                            break;
                        }
                        if chars.get(i + 1) == Some(&'"') && chars.get(i + 2) == Some(&'"') {
                            i += 2;
                            break;
                        }
                    }
                    i += 1;
                }
                prefix.push_str(" str ");
            }
            ('\'', _) => {
                // Character literal (or a Kotlin/Java generic tick that never occurs).
                i += 1;
                while i < chars.len() && chars[i] != '\'' && chars[i] != '\n' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                prefix.push_str(" ch ");
            }
            ('{', _) => {
                stack.push((opens_type_body(&prefix), line));
                prefix.clear();
            }
            ('}', _) => {
                if let Some((is_type, start)) = stack.pop() {
                    if is_type {
                        ranges.push((start, line));
                    }
                }
                prefix.clear();
            }
            (';', _) => prefix.clear(),
            _ => prefix.push(c),
        }
        i += 1;
    }
    ranges
}

fn opens_type_body(prefix: &str) -> bool {
    let words: Vec<&str> = prefix
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$' || c == '@'))
        .filter(|w| !w.is_empty())
        .collect();
    if words.iter().any(|w| {
        matches!(
            *w,
            "class" | "interface" | "enum" | "record" | "object" | "@interface"
        )
    }) {
        return true;
    }
    // `new Foo(...) {` starts an anonymous class.
    let trimmed = prefix.trim_end();
    trimmed.ends_with(')')
        && words.windows(2).any(|pair| pair[0] == "new")
        && !words
            .iter()
            .any(|w| matches!(*w, "if" | "for" | "while" | "switch" | "catch" | "try"))
}

pub(super) fn package_of_source(text: &str) -> String {
    let mut in_block_comment = false;
    for line in text.lines().take(400) {
        let mut line = line.trim();
        if in_block_comment {
            match line.find("*/") {
                Some(end) => {
                    line = line[end + 2..].trim();
                    in_block_comment = false;
                }
                None => continue,
            }
        }
        if line.starts_with("/*") && !line.contains("*/") {
            in_block_comment = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("package ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '$' | '`'))
                .filter(|c| *c != '`')
                .collect();
            return name.replace('.', "/");
        }
        // The package statement precedes every declaration.
        if line.starts_with("import ")
            || line.starts_with("class ")
            || line.starts_with("public ")
            || line.starts_with("fun ")
            || line.starts_with("object ")
            || line.starts_with("interface ")
        {
            break;
        }
    }
    String::new()
}

fn signature_package(signature: &str) -> &str {
    let inner = signature.strip_prefix('L').unwrap_or(signature);
    inner.rfind('/').map_or("", |index| &inner[..index])
}

impl DebugSession {
    // ---- Handler: setBreakpoints ----

    pub async fn handle_set_breakpoints(
        &mut self,
        args: SetBreakpointsArguments,
    ) -> Result<SetBreakpointsResponseBody, String> {
        let source_path = args.source.path.as_deref().ok_or("source path required")?;
        let path = PathBuf::from(source_path);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .or_else(|| args.source.name.clone())
            .ok_or("could not determine the source file name")?;
        // DAP replaces the complete breakpoint list of a source.
        let previous_hits = self.clear_source_breakpoints(&path).await;

        let facts = read_source_facts(&path);
        let package = facts.as_ref().map(|facts| facts.package.clone());
        let type_ranges = facts.map(|facts| facts.type_ranges).unwrap_or_default();
        self.remember_source_root(&path, package.as_deref());

        let entries: Vec<BreakpointEntry> = args
            .breakpoints
            .iter()
            .map(|requested| {
                let id = self.breakpoints.next_id;
                self.breakpoints.next_id += 1;
                BreakpointEntry {
                    id,
                    line: requested.line,
                    actual_line: None,
                    condition: requested.condition.clone().filter(|c| !c.is_empty()),
                    hit_condition: requested
                        .hit_condition
                        .clone()
                        .filter(|c| !c.trim().is_empty()),
                    log_message: requested.log_message.clone().filter(|c| !c.is_empty()),
                    // A breakpoint the client re-sent unchanged keeps counting.
                    hits: previous_hits
                        .get(&(
                            requested.line,
                            requested
                                .hit_condition
                                .clone()
                                .filter(|c| !c.trim().is_empty()),
                        ))
                        .copied()
                        .unwrap_or(0),
                    request_ids: Vec::new(),
                    bound: HashSet::new(),
                }
            })
            .collect();
        let entry_count = entries.len();
        self.breakpoints.sources.insert(
            path.clone(),
            SourceBreakpoints {
                file_name: file_name.clone(),
                package: package.clone(),
                type_ranges,
                source: args.source.clone(),
                entries,
            },
        );

        if entry_count > 0 {
            // Register for future classes first so nothing loaded between the
            // scan below and the request is missed.
            self.ensure_class_prepare_request(&file_name).await;
            let classes = self
                .loaded_classes_for_source(&file_name, package.as_deref())
                .await;
            for index in 0..entry_count {
                self.bind_entry(&path, index, &classes, true).await;
            }
        }
        self.release_unused_class_prepare_requests().await;

        let Some(source_breakpoints) = self.breakpoints.sources.get(&path) else {
            return Ok(SetBreakpointsResponseBody {
                breakpoints: Vec::new(),
            });
        };
        let breakpoints = source_breakpoints
            .entries
            .iter()
            .map(|entry| entry_to_breakpoint(entry, &args.source, self.connection.is_some()))
            .collect();
        Ok(SetBreakpointsResponseBody { breakpoints })
    }

    /// Remove every installed request of `path` and forget its entries.
    /// Returns the hit counts by `(requested line, hit condition)`.
    async fn clear_source_breakpoints(
        &mut self,
        path: &Path,
    ) -> std::collections::HashMap<(u64, Option<String>), u64> {
        let Some(previous) = self.breakpoints.sources.remove(path) else {
            return std::collections::HashMap::new();
        };
        let mut hits = std::collections::HashMap::new();
        for entry in previous.entries {
            hits.insert((entry.line, entry.hit_condition.clone()), entry.hits);
            self.clear_requests(entry.request_ids).await;
        }
        hits
    }

    async fn clear_requests(&mut self, request_ids: Vec<u32>) {
        for request_id in request_ids {
            self.breakpoints.conditions.remove(&request_id);
            if let Ok(conn) = self.conn() {
                let command =
                    Command::event_request_clear(commands::EVENT_KIND_BREAKPOINT, request_id);
                if let Err(error) = conn.send_command(command).await {
                    tracing::warn!("failed to clear breakpoint {request_id}: {error}");
                }
            }
        }
    }

    /// Forget where an entry was bound (its requests are removed), so it can
    /// be bound again, e.g. on the exact line after a slide.
    async fn unbind_entry(&mut self, path: &Path, index: usize) {
        let request_ids = match self
            .breakpoints
            .sources
            .get_mut(path)
            .and_then(|source| source.entries.get_mut(index))
        {
            Some(entry) => {
                entry.bound.clear();
                entry.actual_line = None;
                std::mem::take(&mut entry.request_ids)
            }
            None => return,
        };
        self.clear_requests(request_ids).await;
    }

    async fn ensure_class_prepare_request(&mut self, file_name: &str) {
        if self
            .breakpoints
            .class_prepare_requests
            .contains_key(file_name)
        {
            return;
        }
        let Ok(conn) = self.conn() else {
            return;
        };
        let id_sizes = *conn.id_sizes();
        let command = Command::event_request_set(
            commands::EVENT_KIND_CLASS_PREPARE,
            SUSPEND_EVENT_THREAD,
            &[EventModifier::SourceNameMatch(file_name.to_owned())],
            &id_sizes,
        );
        match conn.send_command(command).await {
            Ok(reply) => {
                let mut data = reply.data;
                if let Ok(request_id) = replies::parse_event_request_set(&mut data) {
                    self.breakpoints
                        .class_prepare_requests
                        .insert(file_name.to_owned(), request_id);
                }
            }
            Err(error) => {
                tracing::warn!("failed to register ClassPrepare for {file_name}: {error}");
            }
        }
    }

    /// Drop ClassPrepare requests whose source file no longer has breakpoints.
    async fn release_unused_class_prepare_requests(&mut self) {
        let wanted: HashSet<String> = self
            .breakpoints
            .sources
            .values()
            .filter(|source| !source.entries.is_empty())
            .map(|source| source.file_name.clone())
            .collect();
        let stale: Vec<String> = self
            .breakpoints
            .class_prepare_requests
            .keys()
            .filter(|name| !wanted.contains(*name))
            .cloned()
            .collect();
        for name in stale {
            if let Some(request_id) = self.breakpoints.class_prepare_requests.remove(&name) {
                if let Ok(conn) = self.conn() {
                    let command = Command::event_request_clear(
                        commands::EVENT_KIND_CLASS_PREPARE,
                        request_id,
                    );
                    if let Err(error) = conn.send_command(command).await {
                        tracing::warn!("failed to clear ClassPrepare request: {error}");
                    }
                }
            }
        }
    }

    // ---- Class discovery ----

    /// Refresh the list of loaded, prepared classes.
    pub(super) async fn refresh_all_classes(&mut self) -> Vec<LoadedClass> {
        let Ok(conn) = self.conn() else {
            return Vec::new();
        };
        let id_sizes = *conn.id_sizes();
        let reply = match conn
            .send_command(Command::vm_all_classes_with_generic())
            .await
        {
            Ok(reply) => reply,
            Err(error) => {
                tracing::warn!("failed to list loaded classes: {error}");
                return Vec::new();
            }
        };
        let mut data = reply.data;
        let decoded = match replies::parse_all_classes_with_generic(&mut data, &id_sizes) {
            Ok(decoded) => decoded,
            Err(error) => {
                // Return before storing: a failed decode must not replace the last good
                // listing with an empty one.
                tracing::warn!("failed to decode loaded classes: {error}");
                return Vec::new();
            }
        };
        let classes: Vec<LoadedClass> = decoded
            .into_iter()
            // Bit 2 = prepared: only then do line tables exist.
            .filter(|class| class.status & 2 != 0)
            .collect();
        for class in &classes {
            self.types
                .signature_by_id
                .insert(class.type_id, class.signature.clone());
            self.types
                .by_signature
                .insert(class.signature.clone(), class.type_id);
        }
        self.types.all_classes = Some(classes.clone());
        classes
    }

    /// Loaded classes compiled from `file_name`, as `(type id, type tag)`.
    async fn loaded_classes_for_source(
        &mut self,
        file_name: &str,
        package: Option<&str>,
    ) -> Vec<(u64, u8)> {
        let all = self.refresh_all_classes().await;
        let mut matching = Vec::new();
        for class in all {
            match package {
                Some(package) if signature_package(&class.signature) != package => continue,
                None if SYSTEM_PREFIXES
                    .iter()
                    .any(|prefix| class.signature.starts_with(prefix)) =>
                {
                    continue;
                }
                _ => {}
            }
            if self.source_file_of(class.type_id).await.as_deref() == Some(file_name) {
                matching.push((class.type_id, class.ref_type_tag));
            }
        }
        matching
    }

    /// The `SourceFile` attribute of a class (cached).
    pub(super) async fn source_file_of(&mut self, class_id: u64) -> Option<String> {
        if let Some(cached) = self.types.source_files.get(&class_id) {
            return cached.clone();
        }
        let conn = self.connection.as_ref()?;
        let id_sizes = *conn.id_sizes();
        let result = match conn
            .send_command(Command::reference_type_source_file(
                class_id,
                id_sizes.reference_type_id_size,
            ))
            .await
        {
            Ok(reply) => {
                let mut data = reply.data;
                replies::parse_source_file(&mut data).ok()
            }
            Err(_) => None,
        };
        self.types.source_files.insert(class_id, result.clone());
        result
    }

    // ---- Binding ----

    /// The location (method, code index) of the first instruction of `line`
    /// in every method of `class_id` that has code for it.
    async fn locations_for_line(&mut self, class_id: u64, line: u64) -> Vec<(u64, u64)> {
        let Some(methods) = self.get_methods(class_id).await else {
            return Vec::new();
        };
        let mut locations = Vec::new();
        for method in &methods {
            let Some(table) = self.get_line_table(class_id, method.method_id).await else {
                continue;
            };
            let first = table
                .iter()
                .filter(|entry| i64::from(entry.line_number) == line as i64)
                .map(|entry| entry.line_code_index)
                .min();
            if let Some(index) = first {
                locations.push((method.method_id, index));
            }
        }
        locations
    }

    /// The smallest line above `line` (within the slide limit) that has code.
    async fn nearest_line_after(
        &mut self,
        classes: &[(u64, u8)],
        line: u64,
        limit_line: u64,
    ) -> Option<u64> {
        let mut best: Option<u64> = None;
        for &(class_id, _) in classes {
            let Some(methods) = self.get_methods(class_id).await else {
                continue;
            };
            for method in &methods {
                let Some(table) = self.get_line_table(class_id, method.method_id).await else {
                    continue;
                };
                for entry in &table {
                    let candidate = entry.line_number as u64;
                    if candidate > line
                        && candidate <= limit_line
                        && candidate - line <= MAX_LINE_SLIDE
                        && best.is_none_or(|current| candidate < current)
                    {
                        best = Some(candidate);
                    }
                }
            }
        }
        best
    }

    /// Install requests for `entry` at every matching location of `classes`.
    /// `allow_slide` lets an entry with no code on its line move to the next
    /// line that has code.
    async fn bind_entry(
        &mut self,
        path: &Path,
        index: usize,
        classes: &[(u64, u8)],
        allow_slide: bool,
    ) -> bool {
        let Some(entry) = self
            .breakpoints
            .sources
            .get(path)
            .and_then(|source| source.entries.get(index))
        else {
            return false;
        };
        let requested = entry.line;
        let mut already_bound = !entry.bound.is_empty();
        let slid_line = entry.actual_line.filter(|line| *line != requested);
        // A slide must stay inside the innermost type around the requested
        // line: code outside it belongs to other classes.
        let limit_line = self
            .breakpoints
            .sources
            .get(path)
            .and_then(|source| {
                source
                    .type_ranges
                    .iter()
                    .filter(|(start, end)| *start <= requested && requested <= *end)
                    .min_by_key(|(start, end)| end - start)
                    .map(|(_, end)| *end)
            })
            .unwrap_or(u64::MAX);

        // The requested line always comes first: a class that has code exactly
        // there (a Kotlin inline lambda's `invokeSuspend`, an enum constant
        // body) beats a slide made when only other classes were loaded.
        let mut hits: Vec<(u64, u8, u64, u64)> = Vec::new();
        let mut line = requested;
        for &(class_id, tag) in classes {
            for (method, code) in self.locations_for_line(class_id, line).await {
                hits.push((class_id, tag, method, code));
            }
        }
        if !hits.is_empty() && slid_line.is_some() {
            self.unbind_entry(path, index).await;
            already_bound = false;
        }
        if hits.is_empty() {
            if let Some(slid) = slid_line {
                // Bound on a slid line: further classes may add locations there.
                line = slid;
                for &(class_id, tag) in classes {
                    for (method, code) in self.locations_for_line(class_id, line).await {
                        hits.push((class_id, tag, method, code));
                    }
                }
            } else if !already_bound && allow_slide {
                if let Some(next) = self
                    .nearest_line_after(classes, requested, limit_line)
                    .await
                {
                    line = next;
                    for &(class_id, tag) in classes {
                        for (method, code) in self.locations_for_line(class_id, line).await {
                            hits.push((class_id, tag, method, code));
                        }
                    }
                }
            }
        }

        let mut newly_bound = false;
        for (class_id, tag, method, code) in hits {
            let entry = self
                .breakpoints
                .sources
                .get(path)
                .and_then(|source| source.entries.get(index));
            let Some(entry) = entry else {
                return newly_bound;
            };
            if entry.bound.contains(&(class_id, method, code)) {
                continue;
            }
            self.types.type_tags.insert(class_id, tag);
            let condition = entry.condition.clone();
            let Some(request_id) = self.install_breakpoint(tag, class_id, method, code).await
            else {
                continue;
            };
            if let Some(condition) = condition {
                self.breakpoints.conditions.insert(request_id, condition);
            }
            if let Some(entry) = self
                .breakpoints
                .sources
                .get_mut(path)
                .and_then(|source| source.entries.get_mut(index))
            {
                entry.bound.insert((class_id, method, code));
                entry.request_ids.push(request_id);
                if entry.actual_line.is_none() {
                    entry.actual_line = Some(line);
                }
                newly_bound = true;
            }
        }
        newly_bound
    }

    async fn install_breakpoint(
        &self,
        type_tag: u8,
        class_id: u64,
        method_id: u64,
        code_index: u64,
    ) -> Option<u32> {
        let conn = self.conn().ok()?;
        let id_sizes = *conn.id_sizes();
        let command = Command::event_request_set(
            commands::EVENT_KIND_BREAKPOINT,
            SUSPEND_ALL,
            &[EventModifier::LocationOnly {
                type_tag,
                class_id,
                method_id,
                index: code_index,
            }],
            &id_sizes,
        );
        match conn.send_command(command).await {
            Ok(reply) => {
                let mut data = reply.data;
                replies::parse_event_request_set(&mut data).ok()
            }
            Err(error) => {
                tracing::warn!("failed to set breakpoint: {error}");
                None
            }
        }
    }

    // ---- Suspending stops while debuggee code runs for an evaluation ----

    /// Remove the breakpoint and exception requests so code run by an
    /// evaluation cannot stop (a stop inside `InvokeMethod` never returns).
    /// Nested calls are counted; `resume_stopping_events` puts them back.
    pub(super) async fn suspend_stopping_events(&mut self) {
        self.stop_events_suspended += 1;
        if self.stop_events_suspended > 1 {
            return;
        }
        let mut request_ids: Vec<u32> = Vec::new();
        for source in self.breakpoints.sources.values_mut() {
            for entry in &mut source.entries {
                request_ids.append(&mut entry.request_ids);
            }
        }
        for request_id in &request_ids {
            self.breakpoints.conditions.remove(request_id);
        }
        let exception_ids = std::mem::take(&mut self.exceptions.requests);
        let Ok(conn) = self.conn() else {
            return;
        };
        for request_id in request_ids {
            let command = Command::event_request_clear(commands::EVENT_KIND_BREAKPOINT, request_id);
            if let Err(error) = conn.send_command(command).await {
                tracing::warn!("failed to suspend breakpoint {request_id}: {error}");
            }
        }
        for request_id in exception_ids {
            let command = Command::event_request_clear(commands::EVENT_KIND_EXCEPTION, request_id);
            if let Err(error) = conn.send_command(command).await {
                tracing::warn!("failed to suspend exception request {request_id}: {error}");
            }
        }
    }

    pub(super) async fn resume_stopping_events(&mut self) {
        self.stop_events_suspended = self.stop_events_suspended.saturating_sub(1);
        if self.stop_events_suspended > 0 || self.connection.is_none() {
            return;
        }
        let mut pending: Vec<(PathBuf, usize, super::state::BoundLocation)> = Vec::new();
        for (path, source) in &self.breakpoints.sources {
            for (index, entry) in source.entries.iter().enumerate() {
                let mut locations: Vec<_> = entry.bound.iter().copied().collect();
                locations.sort_unstable();
                pending.extend(
                    locations
                        .into_iter()
                        .map(|location| (path.clone(), index, location)),
                );
            }
        }
        for (path, index, (class_id, method_id, code_index)) in pending {
            let tag = self.types.type_tags.get(&class_id).copied().unwrap_or(1);
            let Some(request_id) = self
                .install_breakpoint(tag, class_id, method_id, code_index)
                .await
            else {
                continue;
            };
            let Some(entry) = self
                .breakpoints
                .sources
                .get_mut(&path)
                .and_then(|source| source.entries.get_mut(index))
            else {
                continue;
            };
            entry.request_ids.push(request_id);
            if let Some(condition) = entry.condition.clone() {
                self.breakpoints.conditions.insert(request_id, condition);
            }
        }
        if let Some((caught, uncaught)) = self.exceptions.active {
            if let Err(error) = self.install_exception_request(caught, uncaught).await {
                tracing::warn!("failed to restore exception breakpoints: {error}");
            }
        }
    }

    // ---- ClassPrepare ----

    /// A class finished preparing: bind every breakpoint of its source file
    /// that has code in it. Returns DAP `breakpoint` events for breakpoints
    /// that just became verified.
    pub async fn handle_class_prepare(
        &mut self,
        signature: &str,
        type_id: u64,
        ref_type_tag: u8,
    ) -> Vec<BreakpointEventBody> {
        self.types
            .signature_by_id
            .insert(type_id, signature.to_owned());
        self.types
            .by_signature
            .insert(signature.to_owned(), type_id);

        if self
            .breakpoints
            .sources
            .values()
            .all(|source| source.entries.is_empty())
        {
            return Vec::new();
        }
        let Some(file_name) = self.source_file_of(type_id).await else {
            return Vec::new();
        };
        let package = signature_package(signature).to_owned();
        let paths: Vec<PathBuf> = self
            .breakpoints
            .sources
            .iter()
            .filter(|(_, source)| {
                source.file_name == file_name
                    && source
                        .package
                        .as_deref()
                        .is_none_or(|expected| expected == package)
            })
            .map(|(path, _)| path.clone())
            .collect();

        let mut events = Vec::new();
        for path in paths {
            let count = self
                .breakpoints
                .sources
                .get(&path)
                .map_or(0, |source| source.entries.len());
            for index in 0..count {
                let (was_verified, line_before) = self
                    .breakpoints
                    .sources
                    .get(&path)
                    .and_then(|source| source.entries.get(index))
                    .map_or((false, None), |entry| {
                        (!entry.bound.is_empty(), entry.actual_line)
                    });
                let changed = self
                    .bind_entry(&path, index, &[(type_id, ref_type_tag)], !was_verified)
                    .await;
                let line_after = self
                    .breakpoints
                    .sources
                    .get(&path)
                    .and_then(|source| source.entries.get(index))
                    .and_then(|entry| entry.actual_line);
                if changed && (!was_verified || line_after != line_before) {
                    if let Some(source) = self.breakpoints.sources.get(&path) {
                        if let Some(entry) = source.entries.get(index) {
                            events.push(BreakpointEventBody {
                                reason: "changed".to_owned(),
                                breakpoint: entry_to_breakpoint(entry, &source.source, true),
                            });
                        }
                    }
                }
            }
        }
        events
    }

    // ---- Source roots ----

    /// Remember the directory a source file's package tree starts in, so
    /// stack frames of sibling classes resolve to files.
    fn remember_source_root(&mut self, path: &Path, package: Option<&str>) {
        let Some(package) = package else {
            return;
        };
        let mut root = path.parent().map(Path::to_path_buf);
        for segment in package.split('/').filter(|s| !s.is_empty()).rev() {
            root = root.and_then(|dir| {
                (dir.file_name().and_then(|n| n.to_str()) == Some(segment))
                    .then(|| dir.parent().map(Path::to_path_buf))
                    .flatten()
            });
        }
        if let Some(root) = root {
            if !self.source_roots.contains(&root) {
                self.source_roots.push(root);
            }
        }
    }
}

fn entry_to_breakpoint(
    entry: &BreakpointEntry,
    source: &crate::types::Source,
    connected: bool,
) -> Breakpoint {
    let verified = !entry.bound.is_empty();
    Breakpoint {
        id: Some(entry.id),
        verified,
        message: (!verified).then(|| {
            if connected {
                "class not loaded yet, or no executable code at this line".to_owned()
            } else {
                "not connected to a debuggee".to_owned()
            }
        }),
        line: Some(entry.actual_line.unwrap_or(entry.line)),
        source: Some(source.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_is_read_from_java_and_kotlin_sources() {
        assert_eq!(
            package_of_source("package com.example.app;\nclass A {}"),
            "com/example/app"
        );
        assert_eq!(
            package_of_source("@file:JvmName(\"X\")\npackage a.b\nfun f() {}"),
            "a/b"
        );
        assert_eq!(
            package_of_source("/*\n package no.pe\n*/\npackage yes.pe;"),
            "yes/pe"
        );
        assert_eq!(package_of_source("import a.B;\nclass A {}"), "");
        assert_eq!(package_of_source("package `my.pkg`\n"), "my/pkg");
    }

    #[test]
    fn type_bodies_are_found_through_strings_comments_and_anonymous_classes() {
        let source = "public class A {\n  // class in a comment {\n  String s = \"class {\";\n  static class B {\n    void f() {}\n  }\n  Runnable r = new Runnable() {\n    public void run() { if (x) { } }\n  };\n  void g() { Runnable q = () -> { }; }\n}\n";
        let mut ranges = type_body_ranges(source);
        ranges.sort_unstable();
        assert_eq!(ranges, vec![(1, 11), (4, 6), (7, 9)]);
        // Kotlin objects and enum classes.
        let kotlin = "object O {\n  fun f() {}\n}\nenum class E { X, Y }\n";
        let mut ranges = type_body_ranges(kotlin);
        ranges.sort_unstable();
        assert_eq!(ranges, vec![(1, 3), (4, 4)]);
    }

    #[test]
    fn signature_package_handles_default_package_and_nested_classes() {
        assert_eq!(
            signature_package("Lcom/example/Outer$Inner;"),
            "com/example"
        );
        assert_eq!(signature_package("LMain;"), "");
        assert_eq!(signature_package("Lcom/example/FooKt;"), "com/example");
    }
}
