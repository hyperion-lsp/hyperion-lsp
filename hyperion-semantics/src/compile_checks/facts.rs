// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Index-backed facts the compile checks may rely on.
//!
//! Every compile error this crate reports must be *provable*: a check may only
//! speak when the type hierarchy it reasons over is known completely. A single
//! unresolved supertype (an unindexed library, a missing generated class)
//! turns every negative conclusion about that type into a guess, so the
//! queries here answer `None` -- "unknown" -- instead of `false` in that case.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use hyperion_workspace::{Symbol, WorkspaceIndex};
use lsp_types::SymbolKind;

/// The complete set of supertypes of one type, including itself.
pub type SupertypeSet = Rc<HashSet<String>>;

/// Memo table for [`TypeFacts`]; own one per diagnostics pass so the
/// hierarchy of a type is walked once, however many calls mention it.
#[derive(Default)]
pub struct TypeFactsCache {
    supertypes: RefCell<HashMap<String, Option<SupertypeSet>>>,
}

/// View over the workspace index for one diagnostics pass.
pub struct TypeFacts<'a> {
    index: &'a WorkspaceIndex,
    cache: &'a TypeFactsCache,
}

impl<'a> TypeFacts<'a> {
    pub const fn new(index: &'a WorkspaceIndex, cache: &'a TypeFactsCache) -> Self {
        Self { index, cache }
    }

    pub const fn index(&self) -> &'a WorkspaceIndex {
        self.index
    }

    /// The indexed declaration of a class-like type.
    pub fn type_symbol(&self, fqn: &str) -> Option<Arc<Symbol>> {
        if !self.index.has_class(fqn) {
            return None;
        }
        self.index.get_symbol(fqn)
    }

    /// True when `fqn` names a type declaration the index knows.
    pub fn is_known_type(&self, fqn: &str) -> bool {
        self.type_symbol(fqn).is_some()
    }

    /// Every supertype of `fqn` (classes and interfaces, transitively, plus
    /// `java.lang.Object` and itself) -- or `None` when any edge of the
    /// hierarchy is unresolved.
    pub fn supertypes(&self, fqn: &str) -> Option<SupertypeSet> {
        if let Some(cached) = self.cache.supertypes.borrow().get(fqn) {
            return cached.clone();
        }
        let computed = self.compute_supertypes(fqn);
        self.cache
            .supertypes
            .borrow_mut()
            .insert(fqn.to_owned(), computed.clone());
        computed
    }

    fn compute_supertypes(&self, fqn: &str) -> Option<SupertypeSet> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut pending = vec![fqn.to_owned()];
        while let Some(current) = pending.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if current == "java.lang.Object" {
                continue;
            }
            let symbol = self.type_symbol(&current)?;
            let resolved = self.index.direct_supertypes(&current);
            // Every *written* supertype must have produced a resolved edge;
            // otherwise the index silently dropped it (missing dependency).
            let mut written: Vec<&str> = symbol.interfaces.iter().map(|s| &**s).collect();
            if let Some(parent) = symbol.parent_class.as_deref() {
                written.push(parent);
            }
            for name in written {
                if !resolved_covers(&resolved, name) {
                    return None;
                }
            }
            pending.extend(resolved);
        }
        seen.insert("java.lang.Object".to_owned());
        Some(Rc::new(seen))
    }

    /// `Some(true)` / `Some(false)` when the relation is decided by a complete
    /// hierarchy, `None` when it cannot be decided.
    pub fn is_subtype(&self, sub: &str, sup: &str) -> Option<bool> {
        if sub == sup || sup == "java.lang.Object" {
            return Some(true);
        }
        let supertypes = self.supertypes(sub)?;
        Some(supertypes.contains(sup))
    }

    /// Whether the declaration lives in a Kotlin/Groovy/Scala source file,
    /// whose member model differs from Java's (properties, default
    /// arguments, extension functions).
    pub fn is_foreign_language_type(&self, fqn: &str) -> bool {
        let Some(location) = self.index.find_symbol(fqn) else {
            return false;
        };
        self.index
            .get_file_path(location.file_id)
            .and_then(|path| {
                path.extension()
                    .map(|ext| ext.to_string_lossy().into_owned())
            })
            .is_some_and(|ext| matches!(ext.as_str(), "kt" | "kts" | "groovy" | "scala"))
    }

    /// True for interfaces, false for classes/enums/records.
    pub fn is_interface(&self, fqn: &str) -> bool {
        self.type_symbol(fqn)
            .is_some_and(|symbol| symbol.kind == SymbolKind::INTERFACE)
    }
}

/// Erased simple name of a written type ("java.util.Map.Entry<K,V>" -> "Entry").
fn written_simple_name(written: &str) -> &str {
    let raw = written.split('<').next().unwrap_or(written).trim();
    raw.rsplit('.').next().unwrap_or(raw)
}

fn resolved_covers(resolved: &[String], written: &str) -> bool {
    let simple = written_simple_name(written);
    resolved
        .iter()
        .any(|candidate| written_simple_name(candidate) == simple)
}
