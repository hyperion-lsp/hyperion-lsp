// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Taint tracker for analyzing tainted data flow through expressions

use crate::method_summaries::{MethodSummary, MethodSummaryRegistry, TaintBehavior};
use crate::sanitizers::SanitizerRegistry;
use crate::sources::SourceRegistry;
use crate::taint::{TaintState, TrustLevel, VulnTypes};
use crate::type_resolver::TypeAwareAnalyzer;
use hyperion_parser::syntax::{AstNode, FieldAccessExpr, MethodCallExpr, SyntaxKind, SyntaxNode};
use std::collections::{HashMap, HashSet};

/// Tracks taint state through variable assignments and expressions
pub struct TaintTracker<'a> {
    /// Map of variable names to their taint state
    tainted_vars: HashMap<String, TaintState>,

    /// Set of parameter names that are tainted (from annotations)
    tainted_params: HashSet<String>,

    /// Set of PreparedStatement variable names (these are safe for SQL)
    prepared_statements: HashSet<String>,

    /// Map of array names to their element taint states
    /// Vec index represents array index (None = untainted, Some = tainted)
    /// If we don't know the specific index, we treat the entire array as tainted
    tainted_arrays: HashMap<String, Vec<Option<TaintState>>>,

    /// Map of collection names (List, Set, Map) to their taint state
    /// Uses a conservative approach: if ANY tainted element is added to a collection,
    /// the ENTIRE collection is considered tainted. This catches most real vulnerabilities.
    tainted_collections: HashMap<String, TaintState>,

    /// Map of lambda variable names to their captured taint state
    /// Tracks when lambdas capture tainted variables from outer scope
    /// Key: lambda variable name (generated or explicit)
    /// Value: taint state of captured variables
    tainted_lambdas: HashMap<String, TaintState>,

    /// Map of field names to their taint state
    /// Tracks class fields across method boundaries (intra-class only)
    tainted_fields: HashMap<String, TaintState>,

    /// Map of variable names to inferred type names (best-effort)
    var_types: HashMap<String, String>,

    /// Type-aware analyzer for reducing false positives
    type_analyzer: TypeAwareAnalyzer,

    /// Registry of taint sources
    source_registry: &'a SourceRegistry,

    /// Registry of method summaries for common library methods
    method_summaries: MethodSummaryRegistry,

    /// Registry of known sanitizers
    sanitizer_registry: SanitizerRegistry,
}

impl<'a> TaintTracker<'a> {
    /// Create a new taint tracker
    #[must_use]
    pub fn new(
        source_registry: &'a SourceRegistry,
        type_analyzer: TypeAwareAnalyzer,
        sanitizer_registry: SanitizerRegistry,
    ) -> Self {
        Self {
            tainted_vars: HashMap::new(),
            tainted_params: HashSet::new(),
            prepared_statements: HashSet::new(),
            tainted_arrays: HashMap::new(),
            tainted_collections: HashMap::new(),
            tainted_lambdas: HashMap::new(),
            tainted_fields: HashMap::new(),
            var_types: HashMap::new(),
            type_analyzer,
            source_registry,
            method_summaries: MethodSummaryRegistry::new(),
            sanitizer_registry,
        }
    }

    /// Check if a variable is tainted
    #[must_use]
    pub fn is_tainted(&self, var: &str) -> Option<&TaintState> {
        self.tainted_vars.get(var)
    }

    /// Mark a variable as tainted with the given state
    pub fn mark_tainted(&mut self, var: String, state: TaintState) {
        self.tainted_vars.insert(var, state);
    }

    /// Replace a variable binding's taint and return its previous state.
    pub(crate) fn replace_taint(
        &mut self,
        var: &str,
        state: Option<TaintState>,
    ) -> Option<TaintState> {
        match state {
            Some(state) => self.tainted_vars.insert(var.to_string(), state),
            None => self.tainted_vars.remove(var),
        }
    }

    /// Check if a variable is a PreparedStatement
    #[must_use]
    pub fn is_prepared_statement(&self, var: &str) -> bool {
        self.prepared_statements.contains(var)
    }

    /// Mark a variable as a PreparedStatement
    pub fn mark_prepared_statement(&mut self, var: String) {
        self.prepared_statements.insert(var);
    }

    /// Check if a field is tainted
    #[must_use]
    pub fn is_field_tainted(&self, field: &str) -> Option<&TaintState> {
        self.tainted_fields.get(field)
    }

    /// Mark a field as tainted with the given state
    pub fn mark_field_tainted(&mut self, field: &str, state: TaintState) {
        self.tainted_fields.insert(field.to_string(), state);
    }

    /// Reset taint tracking state (called when entering a new method)
    /// Note: Fields are NOT reset - they persist across methods within the class
    pub fn reset(&mut self) {
        self.tainted_vars.clear();
        self.tainted_params.clear();
        self.prepared_statements.clear();
        self.tainted_arrays.clear();
        self.tainted_collections.clear();
        self.tainted_lambdas.clear();
        self.var_types.clear();
        // Intentionally NOT clearing tainted_fields - they persist across methods
    }

    /// Initialize field taint state from pre-collected class-level analysis
    /// This is called before scanning a class to set up cross-method field taint tracking
    pub fn initialize_field_taint(&mut self, field_taint: HashMap<String, TaintState>) {
        self.tainted_fields = field_taint;
    }

    /// Get a clone of the current field taint state
    /// Used to collect field taint from first-pass analysis
    #[must_use]
    pub fn get_field_taint(&self) -> HashMap<String, TaintState> {
        self.tainted_fields.clone()
    }

    /// Get all tainted variable names (for debugging)
    #[must_use]
    pub fn get_tainted_var_names(&self) -> Vec<String> {
        self.tainted_vars.keys().cloned().collect()
    }

    /// Clear all field taint state
    /// Called between class analyses
    pub fn clear_field_taint(&mut self) {
        self.tainted_fields.clear();
    }

    pub fn set_var_type(&mut self, var: String, type_name: String) {
        self.var_types.insert(var, type_name);
    }

    #[must_use]
    pub fn get_var_type(&self, var: &str) -> Option<&str> {
        self.var_types.get(var).map(std::string::String::as_str)
    }

    /// The method summary for a call. Summaries are keyed by class, so an instance call
    /// is matched by its receiver's declared type. A static call's receiver is the class
    /// name, which is the fallback.
    fn summary_for_call(
        &self,
        receiver_name: Option<&str>,
        method_name: &str,
    ) -> Option<&MethodSummary> {
        let declared_type = receiver_name
            .and_then(|receiver| self.var_types.get(receiver))
            .map(std::string::String::as_str);
        self.method_summaries
            .get_summary(declared_type.or(receiver_name), method_name)
    }

    /// Mark a specific array element as tainted
    pub fn mark_array_element_tainted(&mut self, array: &str, index: usize, state: TaintState) {
        let elements = self.tainted_arrays.entry(array.to_string()).or_default();

        // Extend the vector if needed
        if index >= elements.len() {
            elements.resize(index + 1, None);
        }

        elements[index] = Some(state);
    }

    /// Check if a specific array element is tainted
    #[must_use]
    pub fn is_array_element_tainted(&self, array: &str, index: usize) -> Option<&TaintState> {
        self.tainted_arrays
            .get(array)
            .and_then(|elements| elements.get(index))
            .and_then(|opt| opt.as_ref())
    }

    /// Check if any element in an array is tainted
    /// Returns true if we don't know the specific index being accessed (conservative)
    #[must_use]
    pub fn is_any_array_element_tainted(&self, array: &str) -> bool {
        self.tainted_arrays
            .get(array)
            .is_some_and(|elements| elements.iter().any(std::option::Option::is_some))
    }

    /// Mark the entire array as tainted (all elements)
    /// This is used when we don't know which specific elements are tainted
    pub fn mark_whole_array_tainted(&mut self, array: &str, state: TaintState) {
        // Store a single element to indicate the whole array is tainted
        // We'll use index 0 and check for it specially
        self.tainted_arrays
            .insert(array.to_string(), vec![Some(state)]);
    }

    /// Get taint state for array access, returning taint if any element is tainted
    /// This implements conservative handling of unknown indices
    #[must_use]
    pub fn get_array_taint(&self, array: &str) -> Option<TaintState> {
        self.tainted_arrays.get(array).and_then(|elements| {
            // Return the first tainted element we find
            elements.iter().find_map(|opt| opt.as_ref()).cloned()
        })
    }

    // ========================================================================
    // Collection Taint Tracking (List, Set, Map)
    // ========================================================================

    /// Mark a collection as tainted
    /// This is called when a tainted element is added to the collection (add/put)
    pub fn mark_collection_tainted(&mut self, collection: &str, state: TaintState) {
        self.tainted_collections
            .insert(collection.to_string(), state);
    }

    /// Check if a collection is tainted
    #[must_use]
    pub fn is_collection_tainted(&self, collection: &str) -> Option<&TaintState> {
        self.tainted_collections.get(collection)
    }

    /// Get taint state for collection access
    /// Returns the taint state if the collection contains tainted elements
    #[must_use]
    pub fn get_collection_taint(&self, collection: &str) -> Option<TaintState> {
        self.tainted_collections.get(collection).cloned()
    }

    // ========================================================================
    // Lambda Taint Tracking (Phase 4.2)
    // ========================================================================

    /// Mark a lambda as tainted (captures a tainted variable)
    /// This is called when a lambda captures a variable from outer scope that is tainted
    pub fn mark_lambda_tainted(&mut self, lambda_id: String, state: TaintState) {
        self.tainted_lambdas.insert(lambda_id, state);
    }

    /// Check if a lambda is tainted
    #[must_use]
    pub fn is_lambda_tainted(&self, lambda_id: &str) -> Option<&TaintState> {
        self.tainted_lambdas.get(lambda_id)
    }

    /// Get taint state for lambda access
    #[must_use]
    pub fn get_lambda_taint(&self, lambda_id: &str) -> Option<TaintState> {
        self.tainted_lambdas.get(lambda_id).cloned()
    }

    /// Check if an expression is tainted, returning the taint state if so
    pub fn expr_taint(
        &self,
        node: &SyntaxNode,
        node_location: &impl Fn(&SyntaxNode) -> crate::finding::Location,
        get_code_snippet: &impl Fn(&SyntaxNode) -> String,
    ) -> Option<TaintState> {
        self.expr_taint_memoized(&mut HashMap::new(), node, node_location, get_code_snippet)
    }

    /// The tracker cannot change during one `expr_taint` call, so each node's
    /// taint is computed once. Without this, a method chain is 2^n: every call
    /// evaluates its receiver more than once, and the receiver is the rest of
    /// the chain.
    fn expr_taint_memoized(
        &self,
        memo: &mut HashMap<SyntaxNode, Option<TaintState>>,
        node: &SyntaxNode,
        node_location: &impl Fn(&SyntaxNode) -> crate::finding::Location,
        get_code_snippet: &impl Fn(&SyntaxNode) -> String,
    ) -> Option<TaintState> {
        if let Some(taint) = memo.get(node) {
            return taint.clone();
        }
        let taint = self.expr_taint_uncached(memo, node, node_location, get_code_snippet);
        memo.insert(node.clone(), taint.clone());
        taint
    }

    fn expr_taint_uncached(
        &self,
        memo: &mut HashMap<SyntaxNode, Option<TaintState>>,
        node: &SyntaxNode,
        node_location: &impl Fn(&SyntaxNode) -> crate::finding::Location,
        get_code_snippet: &impl Fn(&SyntaxNode) -> String,
    ) -> Option<TaintState> {
        match node.kind() {
            SyntaxKind::NAME_EXPR => {
                // Check if it's a tainted variable or field
                for child in node.children_with_tokens() {
                    if let Some(token) = child.as_token() {
                        // Accept IDENT or keywords that can be used as identifiers
                        let is_identifier_like =
                            token.kind() == SyntaxKind::IDENT || token.kind() == SyntaxKind::TO_KW;

                        if is_identifier_like {
                            let name = token.text().to_string();

                            // First check if it's a variable
                            if let Some(taint) = self.tainted_vars.get(&name) {
                                return Some(taint.clone());
                            }

                            // If not a variable, check if it's a field
                            // This handles the case of "field" without "this."
                            if let Some(taint) = self.is_field_tainted(&name) {
                                let location = node_location(node);
                                let code = get_code_snippet(node);
                                return Some(taint.propagate(
                                    location,
                                    format!("Field access: {name}"),
                                    code,
                                ));
                            }
                        }
                    }
                }
                None
            }
            SyntaxKind::METHOD_CALL_EXPR => {
                // First check if this is a numeric conversion - those are safe
                if MethodCallExpr::cast(node.clone()).is_some()
                    && self.type_analyzer.is_sql_safe_type(node)
                {
                    return None;
                }

                // Continue with existing taint checking
                // Check if method call is a taint source
                #[cfg(test)]
                {
                    eprintln!("METHOD_CALL_EXPR structure:");
                    eprintln!("  Text: {:?}", node.text());
                    for child in node.children_with_tokens() {
                        match child {
                            rowan::NodeOrToken::Node(n) => eprintln!("  Node: {:?}", n.kind()),
                            rowan::NodeOrToken::Token(t) => {
                                eprintln!("  Token: {:?} = {:?}", t.kind(), t.text());
                            }
                        }
                    }
                }
                if let Some(call) = MethodCallExpr::cast(node.clone()) {
                    let mut method_name = call.name().map(|t| t.text().to_string());
                    let mut receiver_name = call
                        .receiver()
                        .and_then(|r| Self::get_receiver_name(r.syntax()));

                    // Some typed-AST shapes omit a static receiver or name from the
                    // wrapper accessors. Recover them from the nested call node's own
                    // receiver/DOT/name children; no parent-sibling walk is needed.
                    if method_name.is_none() || receiver_name.is_none() {
                        let (static_method, static_receiver) =
                            Self::extract_static_call_parts(node);
                        if method_name.is_none() {
                            method_name = static_method;
                        }
                        if receiver_name.is_none() {
                            receiver_name = static_receiver;
                        }
                    }

                    #[cfg(test)]
                    eprintln!(
                        "expr_taint(METHOD_CALL): receiver={:?}, method={:?}",
                        receiver_name, method_name
                    );

                    // Check if this is toString() on a tainted StringBuilder/StringBuffer
                    if let Some(ref name) = method_name {
                        if name == "toString" {
                            if let Some(ref recv) = receiver_name {
                                if let Some(taint) = self.tainted_vars.get(recv) {
                                    let location = node_location(node);
                                    let code = get_code_snippet(node);
                                    return Some(taint.propagate(
                                        location,
                                        format!("toString() on tainted StringBuilder '{recv}'"),
                                        code,
                                    ));
                                }
                            }
                        }
                    }

                    if let Some(ref name) = method_name {
                        // First try with receiver name (e.g., "request", "req")
                        let mut is_src = self
                            .source_registry
                            .is_source(receiver_name.as_deref(), name);

                        // If not found by name, try with receiver's declared type
                        // This handles cases like: HttpServletRequest r; r.getParameter("id")
                        if is_src.is_none() {
                            if let Some(ref recv_name) = receiver_name {
                                if let Some(recv_type) = self.var_types.get(recv_name) {
                                    is_src = self
                                        .source_registry
                                        .is_source(Some(recv_type.as_str()), name);
                                    #[cfg(test)]
                                    if is_src.is_some() {
                                        eprintln!(
                                            "  is_source found via type: {} -> {}",
                                            recv_name, recv_type
                                        );
                                    }
                                }
                            }
                        }

                        #[cfg(test)]
                        eprintln!("  is_source result: {:?}", is_src);
                        if let Some(source_kind) = is_src {
                            // Create taint state for this source with appropriate trust level
                            let location = node_location(node);
                            let code = get_code_snippet(node);
                            let source_desc = receiver_name
                                .map_or_else(|| name.clone(), |recv| format!("{recv}.{name}"));
                            let trust_level = source_kind.trust_level();
                            return Some(TaintState::tainted_with_kind(
                                source_desc,
                                location,
                                code,
                                VulnTypes::all_vulns(),
                                trust_level,
                                source_kind,
                            ));
                        }
                    }

                    // Check method summaries for known library methods
                    if let Some(ref name) = method_name {
                        if let Some(summary) = self.summary_for_call(receiver_name.as_deref(), name)
                        {
                            #[cfg(test)]
                            eprintln!(
                                "  Found method summary for {}.{}",
                                receiver_name.as_deref().unwrap_or("?"),
                                name
                            );

                            match &summary.taint_behavior {
                                TaintBehavior::AlwaysClean => {
                                    // Method sanitizes input (e.g., Integer.parseInt)
                                    #[cfg(test)]
                                    eprintln!("  -> AlwaysClean, returning None");
                                    return None;
                                }
                                TaintBehavior::AlwaysTainted => {
                                    // Method always returns tainted data (e.g., ObjectInputStream.readObject)
                                    // This is used for deserialization methods that return attacker-controlled objects
                                    // Deserialization sources are SemiTrusted since the data typically comes from
                                    // internal systems (though the actual risk depends on context)
                                    let location = node_location(node);
                                    let code = get_code_snippet(node);
                                    let source_desc = receiver_name.as_ref().map_or_else(
                                        || name.clone(),
                                        |recv| format!("{recv}.{name}"),
                                    );
                                    #[cfg(test)]
                                    eprintln!("  -> AlwaysTainted, creating taint from deserialization source: {}", source_desc);
                                    return Some(TaintState::tainted_with_kind(
                                        source_desc,
                                        location,
                                        code,
                                        VulnTypes::all_vulns(),
                                        TrustLevel::SemiTrusted, // Deserialization typically from internal sources
                                        crate::sources::SourceKind::Deserialization,
                                    ));
                                }
                                TaintBehavior::PropagatesReceiver => {
                                    // Method returns tainted if receiver is tainted (e.g., String.toLowerCase)
                                    if let Some(receiver) = call.receiver() {
                                        if let Some(taint) = self.expr_taint_memoized(
                                            memo,
                                            receiver.syntax(),
                                            node_location,
                                            get_code_snippet,
                                        ) {
                                            let location = node_location(node);
                                            let code = get_code_snippet(node);
                                            #[cfg(test)]
                                            eprintln!(
                                                "  -> PropagatesReceiver, receiver is tainted"
                                            );
                                            return Some(taint.propagate(
                                                location,
                                                format!(
                                                    "Method {}.{} propagates taint from receiver",
                                                    receiver_name.as_deref().unwrap_or("?"),
                                                    name
                                                ),
                                                code,
                                            ));
                                        }
                                    }
                                }
                                TaintBehavior::PropagatesArg { arg_index } => {
                                    // Method returns tainted if specific argument is tainted
                                    if let Some(args) = call.args() {
                                        let args_vec: Vec<_> = args.args().collect();
                                        if let Some(arg) = args_vec.get(*arg_index) {
                                            if let Some(taint) = self.expr_taint_memoized(
                                                memo,
                                                arg.syntax(),
                                                node_location,
                                                get_code_snippet,
                                            ) {
                                                let location = node_location(node);
                                                let code = get_code_snippet(node);
                                                #[cfg(test)]
                                                eprintln!(
                                                    "  -> PropagatesArg[{}], arg is tainted",
                                                    arg_index
                                                );
                                                return Some(taint.propagate(
                                                    location,
                                                    format!("Method {}.{} propagates taint from argument {}", receiver_name.as_deref().unwrap_or("?"), name, arg_index),
                                                    code,
                                                ));
                                            }
                                        }
                                    }
                                }
                                TaintBehavior::CombinesArgs { arg_indices } => {
                                    // Method returns tainted if any specified argument is tainted
                                    if let Some(args) = call.args() {
                                        let args_vec: Vec<_> = args.args().collect();
                                        for &idx in arg_indices {
                                            if let Some(arg) = args_vec.get(idx) {
                                                if let Some(taint) = self.expr_taint_memoized(
                                                    memo,
                                                    arg.syntax(),
                                                    node_location,
                                                    get_code_snippet,
                                                ) {
                                                    let location = node_location(node);
                                                    let code = get_code_snippet(node);
                                                    #[cfg(test)]
                                                    eprintln!(
                                                        "  -> CombinesArgs, arg[{}] is tainted",
                                                        idx
                                                    );
                                                    return Some(taint.propagate(
                                                        location,
                                                        format!("Method {}.{} propagates taint from arguments", receiver_name.as_deref().unwrap_or("?"), name),
                                                        code,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                                TaintBehavior::PropagatesAll => {
                                    // Method returns tainted if receiver OR any argument is tainted
                                    // Check receiver first
                                    if let Some(receiver) = call.receiver() {
                                        if let Some(taint) = self.expr_taint_memoized(
                                            memo,
                                            receiver.syntax(),
                                            node_location,
                                            get_code_snippet,
                                        ) {
                                            let location = node_location(node);
                                            let code = get_code_snippet(node);
                                            #[cfg(test)]
                                            eprintln!("  -> PropagatesAll, receiver is tainted");
                                            return Some(taint.propagate(
                                                location,
                                                format!(
                                                    "Method {}.{} propagates taint",
                                                    receiver_name.as_deref().unwrap_or("?"),
                                                    name
                                                ),
                                                code,
                                            ));
                                        }
                                    }
                                    // Check arguments
                                    if let Some(args) = call.args() {
                                        for arg in args.args() {
                                            if let Some(taint) = self.expr_taint_memoized(
                                                memo,
                                                arg.syntax(),
                                                node_location,
                                                get_code_snippet,
                                            ) {
                                                let location = node_location(node);
                                                let code = get_code_snippet(node);
                                                #[cfg(test)]
                                                eprintln!(
                                                    "  -> PropagatesAll, argument is tainted"
                                                );
                                                return Some(taint.propagate(
                                                    location,
                                                    format!("Method {}.{} propagates taint from argument", receiver_name.as_deref().unwrap_or("?"), name),
                                                    code,
                                                ));
                                            }
                                        }
                                    }
                                }
                                TaintBehavior::TaintsReceiver { .. }
                                | TaintBehavior::TaintsReceiverFromValue { .. } => {
                                    // These behaviors mark the RECEIVER as tainted (handled in scanner.rs),
                                    // but don't propagate taint to the return value.
                                    // Collection mutation methods like add(), put() don't return tainted data,
                                    // they just mark the collection as tainted for later get() calls.
                                    #[cfg(test)]
                                    eprintln!("  -> TaintsReceiver behavior - return value not tainted (handled in scanner)");
                                    // Don't return here - fall through to check if receiver is already tainted
                                }
                            }
                        }
                    }

                    // Check if this method is a known sanitizer (e.g., Pattern.quote)
                    if let Some(ref name) = method_name {
                        if let Some(sanitizes) = self
                            .sanitizer_registry
                            .is_sanitizer(receiver_name.as_deref(), name)
                        {
                            let mut input_taint: Option<TaintState> = None;

                            if let Some(receiver) = call.receiver() {
                                if let Some(taint) = self.expr_taint_memoized(
                                    memo,
                                    receiver.syntax(),
                                    node_location,
                                    get_code_snippet,
                                ) {
                                    input_taint = Some(taint);
                                }
                            }

                            if input_taint.is_none() {
                                if let Some(args) = call.args() {
                                    for arg in args.args() {
                                        if let Some(taint) = self.expr_taint_memoized(
                                            memo,
                                            arg.syntax(),
                                            node_location,
                                            get_code_snippet,
                                        ) {
                                            input_taint = Some(taint);
                                            break;
                                        }
                                    }
                                }
                            }

                            if let Some(taint) = input_taint {
                                let location = node_location(node);
                                let code = get_code_snippet(node);
                                let sanitized = taint.sanitize(sanitizes);

                                if sanitized.is_clean() {
                                    return None;
                                }

                                let desc = format!(
                                    "Sanitized via {}.{}",
                                    receiver_name.as_deref().unwrap_or("?"),
                                    name
                                );
                                return Some(sanitized.propagate(location, desc, code));
                            }
                        }
                    }

                    // Check if receiver is tainted - if so, method call propagates taint
                    if let Some(receiver) = call.receiver() {
                        if let Some(taint) = self.expr_taint_memoized(
                            memo,
                            receiver.syntax(),
                            node_location,
                            get_code_snippet,
                        ) {
                            // Method called on tainted object propagates taint
                            let location = node_location(node);
                            let code = get_code_snippet(node);
                            return Some(taint.propagate(
                                location,
                                "Method call on tainted value".to_string(),
                                code,
                            ));
                        }
                    }

                    // Check if any argument is tainted - if so, method call propagates taint
                    // This handles cases like: String result = someMethod(taintedArg)
                    // EXCEPT for numeric parsing methods which sanitize the input
                    // (converting to a number prevents SQL injection, command injection, etc.)
                    #[cfg(test)]
                    eprintln!(
                        "Checking numeric sanitizer: method={:?}, receiver={:?}",
                        method_name, receiver_name
                    );
                    let is_numeric_sanitizer = method_name.as_ref().is_some_and(|m| {
                        let is_parse_method = matches!(
                            m.as_str(),
                            "parseInt"
                                | "parseLong"
                                | "parseShort"
                                | "parseByte"
                                | "parseFloat"
                                | "parseDouble"
                                | "valueOf"
                        );
                        let is_numeric_class = receiver_name.as_ref().is_some_and(|r| {
                            matches!(
                                r.as_str(),
                                "Integer"
                                    | "Long"
                                    | "Short"
                                    | "Byte"
                                    | "Float"
                                    | "Double"
                                    | "BigInteger"
                                    | "BigDecimal"
                            )
                        });
                        is_parse_method && is_numeric_class
                    });

                    if !is_numeric_sanitizer {
                        if let Some(args) = call.args() {
                            for arg in args.args() {
                                if let Some(taint) = self.expr_taint_memoized(
                                    memo,
                                    arg.syntax(),
                                    node_location,
                                    get_code_snippet,
                                ) {
                                    let location = node_location(node);
                                    let code = get_code_snippet(node);

                                    // Phase 9: Conservative method return tainting
                                    // For methods without explicit summaries that take tainted arguments,
                                    // use TrustLevel::Derived to indicate lower confidence.
                                    // This catches cases like custom getter methods that pass tainted data through.
                                    let method_name_str = method_name.as_deref().unwrap_or("");
                                    let has_summary = self
                                        .summary_for_call(receiver_name.as_deref(), method_name_str)
                                        .is_some();

                                    if has_summary {
                                        // Method has explicit summary - propagate with original trust level
                                        return Some(taint.propagate(
                                            location,
                                            "Method call with tainted argument".to_string(),
                                            code,
                                        ));
                                    } else if is_likely_data_returning_method(method_name_str) {
                                        // Conservative: method likely returns derived data
                                        // Use Derived trust level for lower confidence
                                        #[cfg(test)]
                                        eprintln!(
                                            "  -> Conservative taint: {} likely returns data derived from tainted arg",
                                            method_name_str
                                        );
                                        return Some(TaintState::tainted_with_trust(
                                            format!(
                                                "Conservative: {method_name_str} may return tainted data"
                                            ),
                                            location,
                                            code,
                                            VulnTypes::all_vulns(),
                                            TrustLevel::Derived,
                                        ));
                                    }
                                    // Method doesn't match data-returning heuristics
                                    // Still propagate but with Derived trust level for safety
                                    return Some(TaintState::tainted_with_trust(
                                        format!(
                                            "Method {method_name_str} called with tainted argument"
                                        ),
                                        location,
                                        code,
                                        VulnTypes::all_vulns(),
                                        TrustLevel::Derived,
                                    ));
                                }
                            }
                        }
                    }
                }

                // Also check if receiver is tainted (for children in general structure)
                for child in node.children() {
                    if is_expr_kind(child.kind()) {
                        if let Some(taint) =
                            self.expr_taint_memoized(memo, &child, node_location, get_code_snippet)
                        {
                            return Some(taint);
                        }
                    }
                }
                None
            }
            SyntaxKind::BINARY_EXPR => {
                // String concatenation propagates taint
                for child in node.children() {
                    if is_expr_kind(child.kind()) {
                        if let Some(taint) =
                            self.expr_taint_memoized(memo, &child, node_location, get_code_snippet)
                        {
                            return Some(taint);
                        }
                    }
                }
                None
            }
            SyntaxKind::PAREN_EXPR => {
                for child in node.children() {
                    if is_expr_kind(child.kind()) {
                        return self.expr_taint_memoized(
                            memo,
                            &child,
                            node_location,
                            get_code_snippet,
                        );
                    }
                }
                None
            }
            SyntaxKind::TERNARY_EXPR => {
                // Either branch being tainted makes result tainted
                let children: Vec<_> = node.children().filter(|c| is_expr_kind(c.kind())).collect();
                for child in children.iter().skip(1) {
                    if let Some(taint) =
                        self.expr_taint_memoized(memo, child, node_location, get_code_snippet)
                    {
                        return Some(taint);
                    }
                }
                None
            }
            SyntaxKind::NEW_EXPR => {
                // Handle array initialization: new Type[] { ... }
                // or constructor calls: new ClassName(...)

                // Check for array initialization expression
                if let Some(array_init) = node
                    .children()
                    .find(|c| c.kind() == SyntaxKind::ARRAY_INIT_EXPR)
                {
                    // Check if any element in array initialization is tainted
                    for child in array_init.children() {
                        if is_expr_kind(child.kind()) {
                            if let Some(taint) = self.expr_taint_memoized(
                                memo,
                                &child,
                                node_location,
                                get_code_snippet,
                            ) {
                                let location = node_location(node);
                                let code = get_code_snippet(node);
                                return Some(taint.propagate(
                                    location,
                                    "Array initialized with tainted data".to_string(),
                                    code,
                                ));
                            }
                        }
                    }
                }

                // Check for regular constructor calls with arguments
                if let Some(arg_list) = node.children().find(|c| c.kind() == SyntaxKind::ARG_LIST) {
                    for arg in arg_list.children() {
                        if is_expr_kind(arg.kind()) {
                            if let Some(taint) = self.expr_taint_memoized(
                                memo,
                                &arg,
                                node_location,
                                get_code_snippet,
                            ) {
                                // Propagate taint through constructor
                                let location = node_location(node);
                                let code = get_code_snippet(node);
                                return Some(taint.propagate(
                                    location,
                                    "Tainted data passed to constructor".to_string(),
                                    code,
                                ));
                            }
                        }
                    }
                }
                None
            }
            SyntaxKind::ARRAY_ACCESS_EXPR => {
                // Array access: array[index]
                //
                // The parser wraps the receiver expression and the index expression in a single
                // ARRAY_ACCESS_EXPR node, so the receiver is typically the first expression child.
                let mut expr_children = node.children().filter(|c| is_expr_kind(c.kind()));
                let receiver_expr = expr_children.next()?;

                // Fast-path for simple `name[index]` patterns: check the tracked array state.
                if receiver_expr.kind() == SyntaxKind::NAME_EXPR {
                    if let Some(array_name) = Self::get_receiver_name(&receiver_expr) {
                        if let Some(taint) = self
                            .get_array_taint(&array_name)
                            .or_else(|| self.is_tainted(&array_name).cloned())
                        {
                            let location = node_location(node);
                            let code = get_code_snippet(node);
                            return Some(taint.propagate(
                                location,
                                format!("Array access on tainted array '{array_name}'"),
                                code,
                            ));
                        }
                    }
                }

                // Fallback: if the receiver expression itself is tainted (e.g. field access or
                // method returning an array), propagate taint through the access.
                if let Some(taint) =
                    self.expr_taint_memoized(memo, &receiver_expr, node_location, get_code_snippet)
                {
                    let location = node_location(node);
                    let code = get_code_snippet(node);
                    return Some(taint.propagate(
                        location,
                        "Array access on tainted receiver expression".to_string(),
                        code,
                    ));
                }

                None
            }
            SyntaxKind::ARRAY_INIT_EXPR => {
                // Array initialization: { expr1, expr2, ... }
                // Check if any initializer expression is tainted
                for child in node.children() {
                    if is_expr_kind(child.kind()) {
                        if let Some(taint) =
                            self.expr_taint_memoized(memo, &child, node_location, get_code_snippet)
                        {
                            // Return taint if any element is tainted
                            return Some(taint);
                        }
                    }
                }
                None
            }
            SyntaxKind::FIELD_ACCESS_EXPR => {
                // Field access: this.field or object.field
                // With nested trees, FIELD_ACCESS_EXPR contains:
                //   THIS_EXPR/NAME_EXPR (receiver), DOT, IDENT("fieldName")
                // The field name is the last IDENT token child.

                let field_name_str = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|t| t.kind() == SyntaxKind::IDENT)
                    .last()
                    .map(|t| t.text().to_string());

                if let Some(ref field_name_str) = field_name_str {
                    tracing::debug!(field = %field_name_str, "Checking field access taint");

                    if let Some(taint) = self.is_field_tainted(field_name_str) {
                        tracing::debug!(field = %field_name_str, "Field is tainted");

                        let location = node_location(node);
                        let code = get_code_snippet(node);
                        return Some(taint.propagate(
                            location,
                            format!("Field access: {field_name_str}"),
                            code,
                        ));
                    }
                    tracing::debug!(field = %field_name_str, "Field is NOT tainted");

                    // Also check if receiver is a tainted variable (e.g., obj.field where obj is tainted)
                    if let Some(receiver) = node.children().next() {
                        if let Some(taint) = self.expr_taint_memoized(
                            memo,
                            &receiver,
                            node_location,
                            get_code_snippet,
                        ) {
                            let location = node_location(node);
                            let code = get_code_snippet(node);
                            return Some(taint.propagate(
                                location,
                                format!("Field access on tainted receiver: .{field_name_str}"),
                                code,
                            ));
                        }
                    }
                }
                None
            }
            SyntaxKind::CAST_EXPR => {
                // Cast expression: (Type) expression
                // Taint propagates through the cast - the type changes but the data doesn't
                for child in node.children() {
                    if is_expr_kind(child.kind()) {
                        if let Some(taint) =
                            self.expr_taint_memoized(memo, &child, node_location, get_code_snippet)
                        {
                            let location = node_location(node);
                            let code = get_code_snippet(node);
                            return Some(taint.propagate(
                                location,
                                "Cast expression preserves taint".to_string(),
                                code,
                            ));
                        }
                    }
                }
                None
            }
            SyntaxKind::LAMBDA_EXPR => {
                // Lambda expression: x -> expr or (x, y) -> expr
                // Phase 4.2: Track taint through lambda captures
                //
                // A lambda is tainted if:
                // 1. It captures a tainted variable from outer scope
                // 2. The lambda body produces tainted output
                //
                // For lambda as a value (e.g., Function<String, String> f = s -> tainted + s):
                // We check if the lambda body references any tainted variables

                #[cfg(test)]
                eprintln!("LAMBDA_EXPR detected at: {}", node.text());

                let parameters: HashSet<String> =
                    crate::scanner::utilities::lambda_parameter_names(node)
                        .into_iter()
                        .collect();

                // Scan lambda body for tainted variable references. Parameters
                // shadow same-named variables in the enclosing method and are
                // not captures.
                for desc in node.descendants() {
                    if desc.kind() == SyntaxKind::NAME_EXPR {
                        // Check if this name refers to a tainted variable
                        for token in desc.children_with_tokens() {
                            if let Some(t) = token.as_token() {
                                if t.kind() == SyntaxKind::IDENT {
                                    let var_name = t.text().to_string();

                                    if parameters.contains(&var_name) {
                                        continue;
                                    }

                                    // Check if the captured variable is tainted
                                    if let Some(taint) = self.tainted_vars.get(&var_name) {
                                        let location = node_location(node);
                                        let code = get_code_snippet(node);

                                        #[cfg(test)]
                                        eprintln!(
                                            "  Lambda captures tainted variable: {}",
                                            var_name
                                        );

                                        return Some(taint.propagate(
                                            location,
                                            format!(
                                                "Lambda captures tainted variable '{var_name}'"
                                            ),
                                            code,
                                        ));
                                    }

                                    // Check if the captured field is tainted
                                    if let Some(taint) = self.tainted_fields.get(&var_name) {
                                        let location = node_location(node);
                                        let code = get_code_snippet(node);

                                        #[cfg(test)]
                                        eprintln!("  Lambda captures tainted field: {}", var_name);

                                        return Some(taint.propagate(
                                            location,
                                            format!("Lambda captures tainted field '{var_name}'"),
                                            code,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }

                None
            }
            SyntaxKind::METHOD_REF_EXPR => {
                // Method reference: Object::method
                // Phase 4.2: Method references can propagate taint from the receiver
                //
                // For example: taintedObj::toString would be tainted
                // Check if the receiver (before ::) is tainted

                #[cfg(test)]
                eprintln!("METHOD_REF_EXPR detected at: {}", node.text());

                // Look for NAME_EXPR siblings that represent the receiver
                if let Some(parent) = node.parent() {
                    for sibling in parent.children() {
                        if sibling.kind() == SyntaxKind::NAME_EXPR {
                            if let Some(receiver_name) = Self::get_receiver_name(&sibling) {
                                #[cfg(test)]
                                eprintln!("  Method reference receiver: {}", receiver_name);

                                // Check if receiver is tainted
                                if let Some(taint) = self.tainted_vars.get(&receiver_name) {
                                    let location = node_location(node);
                                    let code = get_code_snippet(node);

                                    return Some(taint.propagate(
                                        location,
                                        format!(
                                            "Method reference on tainted receiver '{receiver_name}'"
                                        ),
                                        code,
                                    ));
                                }
                            }
                        }
                    }
                }

                None
            }
            _ => None,
        }
    }

    /// Get receiver name from a node (handles NAME_EXPR, FIELD_ACCESS_EXPR, METHOD_CALL_EXPR)
    fn get_receiver_name(node: &SyntaxNode) -> Option<String> {
        match node.kind() {
            SyntaxKind::NAME_EXPR => {
                for child in node.children_with_tokens() {
                    if let Some(token) = child.as_token() {
                        if token.kind().is_identifier() {
                            return Some(token.text().to_string());
                        }
                    }
                }
                None
            }
            SyntaxKind::FIELD_ACCESS_EXPR => {
                FieldAccessExpr::cast(node.clone()).and_then(|access| {
                    access
                        .name()
                        .map(|name_token| name_token.text().to_string())
                })
            }
            SyntaxKind::METHOD_CALL_EXPR => {
                // For chained calls like foo.bar().baz(), get "bar"
                MethodCallExpr::cast(node.clone())
                    .and_then(|call| call.name().map(|name_token| name_token.text().to_string()))
            }
            _ => None,
        }
    }

    /// Extract method name and receiver from a METHOD_CALL_EXPR node.
    /// With nested trees, METHOD_CALL_EXPR contains receiver, DOT, IDENT, ARG_LIST as children.
    fn extract_static_call_parts(node: &SyntaxNode) -> (Option<String>, Option<String>) {
        // With nested trees, `node` IS the METHOD_CALL_EXPR containing everything
        if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
            let mut receiver_name: Option<String> = None;
            let mut method_name: Option<String> = None;
            let mut found_dot = false;

            for elem in node.children_with_tokens() {
                match elem {
                    rowan::NodeOrToken::Node(ref n) => {
                        if !found_dot && n.kind() == SyntaxKind::NAME_EXPR {
                            receiver_name = n
                                .children_with_tokens()
                                .filter_map(rowan::NodeOrToken::into_token)
                                .find(|t| t.kind().is_identifier())
                                .map(|t| t.text().to_string());
                        }
                    }
                    rowan::NodeOrToken::Token(ref t) => {
                        if t.kind() == SyntaxKind::DOT {
                            found_dot = true;
                        } else if found_dot && t.kind().is_identifier() {
                            method_name = Some(t.text().to_string());
                        }
                    }
                }
            }

            if found_dot {
                return (method_name, receiver_name);
            }
        }

        (None, None)
    }

    /// Check for method call patterns like `receiver.method(args)` in nested METHOD_CALL_EXPR nodes
    pub fn check_method_call_pattern_taint_in_children(
        &self,
        node: &SyntaxNode,
        node_location: &impl Fn(&SyntaxNode) -> crate::finding::Location,
        get_code_snippet: &impl Fn(&SyntaxNode) -> String,
    ) -> Option<TaintState> {
        for child in node.children() {
            if child.kind() != SyntaxKind::METHOD_CALL_EXPR {
                continue;
            }

            let (method_name, receiver_name) = Self::extract_static_call_parts(&child);

            let Some(method_str) = method_name else {
                continue;
            };

            #[cfg(test)]
            eprintln!(
                "Found method call pattern: {:?}.{}",
                receiver_name, method_str
            );

            if let Some(ref recv) = receiver_name {
                if let Some(source_kind) = self
                    .source_registry
                    .is_source(Some(recv.as_str()), &method_str)
                {
                    #[cfg(test)]
                    eprintln!("  -> This is a taint source!");

                    let location = node_location(node);
                    let code = get_code_snippet(node);
                    let source_desc = format!("{recv}.{method_str}");
                    let trust_level = source_kind.trust_level();
                    return Some(TaintState::tainted_with_kind(
                        source_desc,
                        location,
                        code,
                        VulnTypes::all_vulns(),
                        trust_level,
                        source_kind,
                    ));
                }

                if let Some(taint) = self.tainted_vars.get(recv) {
                    #[cfg(test)]
                    eprintln!(
                        "  -> Receiver '{}' is tainted, propagating through method call",
                        recv
                    );

                    let location = node_location(node);
                    let code = get_code_snippet(node);
                    return Some(taint.propagate(
                        location,
                        format!("Method call {recv}.{method_str} on tainted value"),
                        code,
                    ));
                }
            }
        }

        None
    }

    /// Check if node contains a numeric type conversion pattern
    /// Detects: Integer.parseInt(...), Long.parseLong(...), etc.
    pub fn is_numeric_type_conversion_in_siblings(node: &SyntaxNode) -> bool {
        // With nested trees, look for METHOD_CALL_EXPR descendants
        for child in node.children() {
            if child.kind() != SyntaxKind::METHOD_CALL_EXPR {
                continue;
            }

            let (method_name, class_name) = Self::extract_static_call_parts(&child);

            if let (Some(class), Some(method)) = (class_name, method_name) {
                let is_numeric_class = matches!(
                    class.as_str(),
                    "Integer"
                        | "Long"
                        | "Short"
                        | "Byte"
                        | "Float"
                        | "Double"
                        | "BigInteger"
                        | "BigDecimal"
                );
                let is_parse_method = matches!(
                    method.as_str(),
                    "parseInt"
                        | "parseLong"
                        | "parseShort"
                        | "parseByte"
                        | "parseFloat"
                        | "parseDouble"
                        | "valueOf"
                        | "decode"
                        | "parseUnsignedInt"
                        | "parseUnsignedLong"
                );

                if is_numeric_class && is_parse_method {
                    #[cfg(test)]
                    eprintln!(
                        "Detected numeric conversion in siblings: {}.{}",
                        class, method
                    );
                    return true;
                }
            }
        }

        false
    }
}

/// Check if a SyntaxKind is an expression kind
const fn is_expr_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::INSTANCEOF_EXPR
            | SyntaxKind::PATTERN_INSTANCEOF_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::METHOD_REF_EXPR
            | SyntaxKind::CONSTRUCTOR_REF_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::CLASS_LITERAL_EXPR
            | SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::SWITCH_EXPR
    )
}

/// Check if a method name suggests it returns data derived from its arguments.
///
/// Methods with these prefixes are likely to return data that includes or is derived
/// from their input arguments:
/// - `get*`, `find*`, `load*`, `fetch*`, `read*` - data retrieval methods
/// - `parse*`, `process*`, `transform*`, `convert*` - data transformation methods
/// - `build*`, `create*`, `make*`, `generate*` - factory methods that may embed input
///
/// This is a conservative heuristic - it may produce false positives, but that's acceptable
/// for security scanning where missing a vulnerability is worse than a false alarm.
fn is_likely_data_returning_method(method_name: &str) -> bool {
    // Check for common data-returning method prefixes
    let prefixes = [
        "get",
        "find",
        "load",
        "fetch",
        "read",
        "parse",
        "process",
        "transform",
        "convert",
        "build",
        "create",
        "make",
        "generate",
        "extract",
        "resolve",
        "lookup",
        "retrieve",
        "compute",
        "calculate",
        "derive",
        "format",
    ];

    let method_lower = method_name.to_lowercase();
    prefixes
        .iter()
        .any(|prefix| method_lower.starts_with(prefix))
}

#[cfg(test)]
mod method_summary_lookup_tests {
    use super::*;
    use crate::finding::Location;
    use hyperion_parser::{Lexer, Parser, SyntaxNode as RawSyntaxNode};

    fn location() -> Location {
        Location {
            file: std::path::PathBuf::from("A.java"),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
        }
    }

    /// The first method call in `class A { void m() { Object x = <call>; } }`
    fn call_node(call: &str) -> SyntaxNode {
        let source = format!("class A {{ void m() {{ Object x = {call}; }} }}");
        let tokens = Lexer::new(&source).collect();
        let (green, _errors) = Parser::parse(tokens);
        RawSyntaxNode::new_root(green)
            .descendants()
            .find(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
            .expect("a method call")
    }

    #[test]
    fn instance_call_matches_its_summary_by_the_receivers_declared_type() {
        let mut frameworks = crate::frameworks::FrameworkRegistry::with_builtin();
        let sanitizer_registry = frameworks.sanitizer_registry().clone();
        let mut tracker = TaintTracker::new(
            frameworks.source_registry(),
            TypeAwareAnalyzer::new(),
            sanitizer_registry,
        );
        // "n" is declared as Integer, so its summary is the Integer one. Integer.parseInt is
        // AlwaysClean and returns clean data whatever it is given.
        tracker.set_var_type("n".to_string(), "Integer".to_string());
        tracker.mark_tainted(
            "term".to_string(),
            TaintState::tainted(
                "request.getParameter".to_string(),
                location(),
                String::new(),
                VulnTypes::all_vulns(),
            ),
        );

        let node = call_node("n.parseInt(term)");
        let taint = tracker.expr_taint(&node, &|_| location(), &|_| String::new());
        assert!(
            taint.is_none(),
            "the summary is keyed by the declared type, so the sanitizer applies"
        );
    }

    #[test]
    fn static_call_still_matches_its_summary_by_the_class_name() {
        let mut frameworks = crate::frameworks::FrameworkRegistry::with_builtin();
        let sanitizer_registry = frameworks.sanitizer_registry().clone();
        let mut tracker = TaintTracker::new(
            frameworks.source_registry(),
            TypeAwareAnalyzer::new(),
            sanitizer_registry,
        );
        tracker.mark_tainted(
            "term".to_string(),
            TaintState::tainted(
                "request.getParameter".to_string(),
                location(),
                String::new(),
                VulnTypes::all_vulns(),
            ),
        );

        // Integer.parseInt is AlwaysClean. A static call's receiver is the class itself.
        let node = call_node("Integer.parseInt(term)");
        let taint = tracker.expr_taint(&node, &|_| location(), &|_| String::new());
        assert!(taint.is_none());
    }
}
