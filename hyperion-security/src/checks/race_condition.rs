// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Race condition / TOCTOU (Time-of-Check to Time-of-Use) detection
//!
//! Detects check-then-act patterns that are vulnerable to race conditions.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

pub struct RaceConditionCheck;

fn get_method_with_modifiers(node: &SyntaxNode) -> String {
    let method_text = node.text().to_string();

    // Look for MODIFIER_LIST sibling that precedes this METHOD_DECL
    if let Some(parent) = node.parent() {
        let node_range = node.text_range();
        let mut prev_sibling = None;

        for child in parent.children() {
            if child.text_range() == node_range {
                // Found the method node, use the previous sibling if it's MODIFIER_LIST
                break;
            }
            if child.kind() == SyntaxKind::MODIFIER_LIST {
                prev_sibling = Some(child);
            }
        }

        if let Some(modifiers) = prev_sibling {
            return format!("{} {}", modifiers.text(), method_text);
        }
    }

    method_text
}

/// Checks whether the node is protected by a recognized synchronization context.
fn has_synchronization_context(node: &SyntaxNode) -> bool {
    // Check 1: Is this method declared synchronized?
    if is_synchronized_method(node) {
        return true;
    }

    // Check 2: Is the node inside a synchronized block?
    if is_inside_synchronized_block(node) {
        return true;
    }

    // Check 3: Is this a private method in a class with synchronized methods?
    // (Heuristic: private methods in synchronized classes are likely protected)
    if is_private_method_in_synchronized_class(node) {
        return true;
    }

    // Check 4: Does the method use explicit locks?
    if uses_explicit_locks(node) {
        return true;
    }

    false
}

/// Check if the method is declared with the synchronized modifier
fn is_synchronized_method(node: &SyntaxNode) -> bool {
    method_has_modifier(node, SyntaxKind::SYNCHRONIZED_KW)
}

/// Method modifiers are siblings immediately preceding `METHOD_DECL` in the
/// parser tree. Only consider the adjacent list; using every modifier list in
/// the class makes one synchronized/private member affect unrelated methods.
fn method_has_modifier(node: &SyntaxNode, modifier: SyntaxKind) -> bool {
    let mut current = Some(node.clone());
    while let Some(n) = current {
        if n.kind() == SyntaxKind::METHOD_DECL {
            let Some(parent) = n.parent() else {
                return false;
            };
            let mut adjacent_modifiers = None;
            for child in parent.children() {
                if child.text_range() == n.text_range() {
                    break;
                }
                if child.kind() == SyntaxKind::MODIFIER_LIST {
                    adjacent_modifiers = Some(child);
                } else {
                    adjacent_modifiers = None;
                }
            }
            return adjacent_modifiers.is_some_and(|modifiers| {
                modifiers
                    .descendants_with_tokens()
                    .any(|element| element.kind() == modifier)
            });
        }
        current = n.parent();
    }
    false
}

/// Check if the node is inside a synchronized block
fn is_inside_synchronized_block(node: &SyntaxNode) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if is_synchronized_statement(&parent) {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// Whether the node is a `synchronized (lock) { ... }` statement, as in
/// `synchronized (this) { ... }` or `synchronized (lock) { ... }`.
fn is_synchronized_statement(node: &SyntaxNode) -> bool {
    let text = node.text().to_string();
    let trimmed = text.trim_start();
    trimmed.starts_with("synchronized")
        && (trimmed.contains("synchronized (") || trimmed.contains("synchronized("))
}

/// Check if this is a private method in a class that has synchronized methods
/// This is a heuristic: private methods are only called from within the class,
/// so if the class has synchronized methods, the private method is likely protected
fn is_private_method_in_synchronized_class(node: &SyntaxNode) -> bool {
    let mut current = Some(node.clone());
    while let Some(n) = current.clone() {
        if n.kind() == SyntaxKind::METHOD_DECL {
            if !method_has_modifier(&n, SyntaxKind::PRIVATE_KW) {
                return false;
            }
            break;
        }
        current = n.parent();
    }

    // Now check if the containing class has synchronized methods
    let mut current = Some(node.clone());
    while let Some(n) = current {
        if n.kind() == SyntaxKind::CLASS_DECL {
            let class_text = n.text().to_string();
            // Look for synchronized methods in the class
            return class_text.contains("synchronized");
        }
        current = n.parent();
    }

    false
}

/// Check if the method uses explicit lock mechanisms
fn uses_explicit_locks(node: &SyntaxNode) -> bool {
    let mut current = Some(node.clone());
    let mut text = node.text().to_string();
    while let Some(ancestor) = current {
        if ancestor.kind() == SyntaxKind::METHOD_DECL {
            text = ancestor.text().to_string();
            break;
        }
        current = ancestor.parent();
    }

    // Explicit lock patterns
    text.contains("ReentrantLock")
        || text.contains("ReadWriteLock")
        || text.contains(".lock()")
        || text.contains(".unlock()")
        || text.contains("tryLock()")
        || text.contains("StampedLock")
        || text.contains("Semaphore")
}

// ============================================================================
// PHASE 2: TOCTOU Context Awareness (FP Reduction Round 6)
// ============================================================================

/// Check if the TOCTOU pattern occurs in a logging context.
/// Logging file existence is informational, not a security issue.
fn is_logging_context(node: &SyntaxNode) -> bool {
    let mut current = node.parent();

    while let Some(parent) = current {
        let text = parent.text().to_string();
        let lower = text.to_lowercase();

        // Check for logger method calls
        let logging_patterns = [
            "logger.info(",
            "logger.debug(",
            "logger.warn(",
            "logger.error(",
            "logger.trace(",
            "log.info(",
            "log.debug(",
            "log.warn(",
            "log.error(",
            ".log(",
            "system.out.println(",
        ];

        if logging_patterns.iter().any(|p| lower.contains(p)) {
            return true;
        }

        current = parent.parent();
    }

    false
}

/// Check if the file path appears to be from configuration (not user input).
/// Configuration paths are typically constants or loaded from properties.
fn is_config_file_path(node: &SyntaxNode) -> bool {
    // Get the full method or class context
    let mut current = Some(node.clone());
    let mut method_text = String::new();
    let mut class_text = String::new();

    while let Some(n) = current {
        match n.kind() {
            SyntaxKind::METHOD_DECL if method_text.is_empty() => {
                method_text = n.text().to_string();
            }
            SyntaxKind::CLASS_DECL if class_text.is_empty() => {
                class_text = n.text().to_string();
                break;
            }
            _ => {}
        }
        current = n.parent();
    }

    let combined_text = format!("{class_text} {method_text}");
    let lower = combined_text.to_lowercase();

    // Check for configuration-related patterns
    let config_indicators = [
        "config",
        "metadata",
        "resource",
        "filepath",
        "file_path",
        "location",
        "directory",
        "dir",
    ];

    // Must have config-related naming
    if !config_indicators.iter().any(|ind| lower.contains(ind)) {
        return false;
    }

    // Look for property loading patterns or static final constants
    if combined_text.contains("getProperty(")
        || combined_text.contains("getenv(")
        || combined_text.contains("@Value(")
        || combined_text.contains("application.properties")
        || combined_text.contains("application.yml")
        || combined_text.contains("static final")
    {
        return true;
    }

    false
}

/// Check if the operation is a cache-related file operation.
/// Cache invalidation TOCTOU is not a security issue (at most a cache miss).
fn is_cache_operation(node: &SyntaxNode) -> bool {
    let mut current = Some(node.clone());

    while let Some(n) = current {
        let text = n.text().to_string();
        let lower = text.to_lowercase();

        // Check for cache-related patterns
        let cache_patterns = [
            "cache",
            "metadata",
            "cached",
            "temp",
            "temporary",
            ".cache",
            "/cache/",
            "cachedir",
        ];

        if cache_patterns.iter().any(|p| lower.contains(p)) {
            return true;
        }

        // Check method names for cache-related operations
        if n.kind() == SyntaxKind::METHOD_DECL
            && (lower.contains("cache") || lower.contains("metadata"))
        {
            return true;
        }

        current = n.parent();
    }

    false
}

/// Check if the file path might be user-controlled.
/// User-controlled paths in TOCTOU are the actual security issue.
fn is_user_controlled_path(text: &str) -> bool {
    let lower = text.to_lowercase();

    // User input indicators in variable names
    let user_input_patterns = [
        "request",
        "param",
        "input",
        "upload",
        "user",
        "file_name", // Different from file_path (which is often config)
        "filename",  // User-provided filename
    ];

    if !user_input_patterns.iter().any(|p| lower.contains(p)) {
        return false;
    }

    // Look for request parameter extraction patterns
    if text.contains("@RequestParam")
        || text.contains("@PathVariable")
        || text.contains("getParameter(")
        || text.contains("request.get")
        || text.contains("MultipartFile")
    {
        return true;
    }

    false
}

/// Determine if a TOCTOU file operation pattern is security-relevant.
///
/// Returns false (skip) only if we detect benign contexts:
/// 1. Logging context (logging file existence is informational)
/// 2. Config file path (static configuration paths are low risk)
/// 3. Cache operation (cache races cause cache misses, not security issues)
///
/// Otherwise returns true (flag it) to maintain security coverage.
/// User-controlled paths get higher confidence scoring.
fn is_security_relevant_toctou(node: &SyntaxNode) -> (bool, bool) {
    let text = node.text().to_string();

    // Skip if in logging context
    if is_logging_context(node) {
        return (false, false);
    }

    // Skip if config file path
    if is_config_file_path(node) {
        return (false, false);
    }

    // Skip if cache operation
    if is_cache_operation(node) {
        return (false, false);
    }

    // Flag with high confidence if user-controlled path
    if is_user_controlled_path(&text) {
        return (true, true); // (should_flag, is_user_controlled)
    }

    // Default: flag it (maintain security coverage)
    // These are potential TOCTOU issues even if not user-controlled
    (true, false) // (should_flag, is_user_controlled)
}

// ============================================================================
// PHASE 3: Local Variable Race Conditions (FP Reduction Round 6)
// ============================================================================

/// Check if a variable is a method parameter.
/// Method parameters are local to the call, no concurrent access possible.
fn is_method_parameter(node: &SyntaxNode, var_name: &str) -> bool {
    // Walk up to find METHOD_DECL
    let mut current = Some(node.clone());
    while let Some(n) = current {
        if n.kind() == SyntaxKind::METHOD_DECL {
            let method_text = n.text().to_string();

            // Look for parameter list
            if let Some(start) = method_text.find('(') {
                if let Some(end) = method_text[start..].find(')') {
                    let params = &method_text[start..start + end];

                    // Check if var_name appears in parameter list
                    // Match: "LogoutRequest logoutRequest" or "List<String> items"
                    // Use word boundary checks to avoid partial matches
                    let words: Vec<&str> = params.split_whitespace().collect();
                    if words
                        .iter()
                        .any(|w| w.trim_end_matches(',').trim_end_matches(')') == var_name)
                    {
                        return true;
                    }
                }
            }
            break;
        }
        current = n.parent();
    }
    false
}

/// Check if a variable is a local variable (not a field).
/// Local variables are stack-allocated per thread, no race conditions.
fn is_local_variable(node: &SyntaxNode, var_name: &str) -> bool {
    // Walk up to find LOCAL_VAR_DECL
    let mut current = Some(node.clone());

    // First, check if we can find a local variable declaration
    while let Some(n) = current.clone() {
        if n.kind() == SyntaxKind::LOCAL_VAR_DECL {
            let text = n.text().to_string();
            // Check if this declaration contains our variable name
            let words: Vec<&str> = text.split_whitespace().collect();
            if words
                .iter()
                .any(|w| w.trim_end_matches(';').trim_end_matches('=') == var_name)
            {
                return true;
            }
        }

        // If we hit METHOD_DECL, we're still in local scope
        if n.kind() == SyntaxKind::METHOD_DECL {
            // Look for variable declarations in method body
            let method_text = n.text().to_string();

            if declares_local_variable(&method_text, var_name) {
                return true;
            }

            break;
        }

        current = n.parent();
    }

    false
}

/// Whether `method_text` declares `var_name` as a local, as in `List<String> items =`,
/// `val items =` or `final int items =`. A bare `items = ...` assigns a field, and the
/// assignment alone does not make it a local.
fn declares_local_variable(method_text: &str, var_name: &str) -> bool {
    method_text.match_indices(var_name).any(|(start, _)| {
        let before = &method_text[..start];
        let after = method_text[start + var_name.len()..].trim_start();

        // The name must start a word: a longer identifier such as "xitems" does not count
        let starts_word = before
            .chars()
            .last()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '$'));
        let assigns = after.starts_with('=') && !after.starts_with("==");

        // A type comes before the name: an identifier, or the end of a generic or array type
        let trimmed = before.trim_end();
        let word_before = trimmed
            .rsplit(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
            .next()
            .unwrap_or("");
        let after_type = !trimmed.is_empty()
            && (trimmed.ends_with('>') || trimmed.ends_with(']') || !word_before.is_empty())
            && !matches!(word_before, "else" | "do" | "return" | "throw" | "yield");

        starts_word && assigns && after_type
    })
}

/// Check if a variable is an instance or static field.
/// Fields can be accessed concurrently, requiring synchronization.
fn is_field(node: &SyntaxNode, var_name: &str) -> bool {
    // If it's not a parameter and not a local variable, it's likely a field
    !is_method_parameter(node, var_name) && !is_local_variable(node, var_name)
}

/// Check if a collection type is concurrent/thread-safe by design.
fn is_concurrent_collection_type(node: &SyntaxNode, var_name: &str) -> bool {
    let concurrent_types = [
        "ConcurrentHashMap",
        "ConcurrentLinkedQueue",
        "CopyOnWriteArrayList",
        "ConcurrentSkipListMap",
        "BlockingQueue",
        "ConcurrentMap",
    ];

    let mut current = Some(node.clone());
    while let Some(ancestor) = current {
        if ancestor.kind() == SyntaxKind::CLASS_DECL {
            return ancestor
                .descendants()
                .filter(|descendant| descendant.kind() == SyntaxKind::FIELD_DECL)
                .any(|field| {
                    let field_text = field.text().to_string();
                    let declares_variable = field
                        .descendants_with_tokens()
                        .filter_map(rowan::NodeOrToken::into_token)
                        .any(|token| token.kind().is_identifier() && token.text() == var_name);
                    declares_variable
                        && concurrent_types
                            .iter()
                            .any(|collection_type| field_text.contains(collection_type))
                });
        }
        current = ancestor.parent();
    }

    false
}

/// Check if a variable is from parsed/immutable data.
/// Data from XML/JSON parsing is immutable after parsing.
fn is_from_parsed_data(collection: &str) -> bool {
    // Look for getter method calls that suggest parsed/immutable data
    let immutable_sources = [
        ".getSessionIndexes()",
        ".parse(",
        ".unmarshal(",
        ".deserialize(",
        "JSON.parse",
        "XML.parse",
        "SAML",
        "Request", // HTTP request objects are per-thread
    ];

    immutable_sources
        .iter()
        .any(|source| collection.contains(source))
}

/// Extract the variable name from a check-then-act pattern.
/// Returns the variable being checked (e.g., "items" from "items.isEmpty()").
fn extract_collection_variable_name(text: &str) -> String {
    // Look for patterns like "items.isEmpty()" or "list.get("
    // Extract the variable name before the dot

    // Common check patterns
    let patterns = [
        ".isEmpty()",
        ".size()",
        ".length()",
        ".containsKey(",
        ".contains(",
    ];

    for pattern in &patterns {
        if let Some(pos) = text.find(pattern) {
            // Work backwards from the pattern to find the variable name
            let before_pattern = &text[..pos];

            // Find the last whitespace or operator before the variable
            let var_start = before_pattern
                .rfind(|c: char| c.is_whitespace() || c == '(' || c == '!' || c == '&' || c == '|')
                .map_or(0, |i| i + 1);

            let var_name = before_pattern[var_start..].trim();
            if !var_name.is_empty() {
                return var_name.to_string();
            }
        }
    }

    // Default: return empty string if no variable found
    String::new()
}

/// Determine if a collection race condition check is security-relevant.
///
/// Returns true only if:
/// 1. Collection is an instance/static field (shared state)
/// 2. NOT in a synchronized method/block
/// 3. NOT a concurrent collection type
/// 4. NOT from parsed/immutable data
fn is_security_relevant_collection_race(node: &SyntaxNode, var_name: &str) -> bool {
    // Skip if empty variable name (couldn't extract)
    if var_name.is_empty() {
        return false;
    }

    // Skip if in synchronized context
    if has_synchronization_context(node) {
        return false;
    }

    // The collection is the root identifier of the expression, as in "request" for
    // "request.getHeaders()". Its parameter, local or field status decides the rest.
    let root: String = var_name
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if root.is_empty() {
        return false;
    }

    // Skip if it's a method parameter (local to call)
    if is_method_parameter(node, &root) {
        return false;
    }

    // Skip if it's a local variable (stack-allocated)
    if is_local_variable(node, &root) {
        return false;
    }

    // Skip if it's a concurrent collection type
    if is_concurrent_collection_type(node, &root) {
        return false;
    }

    // Skip if from parsed/immutable data
    if is_from_parsed_data(var_name) {
        return false;
    }

    // Must be a field to have concurrent access
    if !is_field(node, &root) {
        return false;
    }

    // It's a field without synchronization → flag it
    true
}

/// File operations that act on a path after an existence check.
const FILE_OPERATIONS: [&str; 9] = [
    "FileInputStream",
    "FileOutputStream",
    "FileReader",
    "FileWriter",
    "Files.newInputStream",
    "Files.newOutputStream",
    "new File(",
    "file.delete()",
    "Files.delete",
];

/// Whether a `synchronized` statement inside the method holds both the existence check
/// and the file operation. The check and the use then run under one lock.
fn check_and_use_locked(method: &SyntaxNode) -> bool {
    method.descendants().any(|block| {
        if !is_synchronized_statement(&block) {
            return false;
        }
        let text = block.text().to_string();
        text.contains(".exists()")
            && FILE_OPERATIONS
                .iter()
                .any(|operation| text.contains(operation))
    })
}

impl Check for RaceConditionCheck {
    fn name(&self) -> &'static str {
        "race-condition"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::METHOD_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = get_method_with_modifiers(node);
        let mut findings = Vec::new();

        // Pattern 1: File existence check followed by file operation
        if (text.contains("file.exists()") || text.contains(".exists()"))
            && FILE_OPERATIONS
                .iter()
                .any(|operation| text.contains(operation))
        {
            // PHASE 2: Check if this TOCTOU is actually security-relevant
            // Skip logging contexts, config paths, and cache operations
            let (should_flag, is_user_controlled) = is_security_relevant_toctou(node);

            if should_flag && !has_synchronization_context(node) && !check_and_use_locked(node) {
                // Security-relevant TOCTOU without synchronization - flag it
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);

                let (message, severity, confidence, remediation) = if is_user_controlled {
                    (
                        "TOCTOU vulnerability: File existence check followed by file operation with user-controlled path. The file state could change between the check and use, leading to security issues or crashes.".to_string(),
                        Severity::High,
                        0.70,
                        "Use try-with-resources and handle FileNotFoundException, or use Files.newInputStream with StandardOpenOption for atomic operations. Avoid separate existence checks. Validate and sanitize user-provided file paths.".to_string()
                    )
                } else {
                    (
                        "TOCTOU vulnerability: File existence check followed by file operation. The file state could change between the check and use, leading to security issues or crashes.".to_string(),
                        Severity::Medium,
                        0.25,
                        "Use try-with-resources and handle FileNotFoundException, or use Files.newInputStream with StandardOpenOption for atomic operations. Avoid separate existence checks.".to_string()
                    )
                };

                let mut finding = Finding::new(VulnType::RaceCondition, message, location, code);
                finding.severity = severity;
                finding.confidence = confidence;
                finding.remediation = remediation;
                findings.push(finding);
            }
        }

        // Pattern 2: Balance check followed by withdrawal/debit
        if (text.contains("getBalance()") || text.contains(".balance"))
            && (text.contains("withdraw(")
                || text.contains("debit(")
                || text.contains("deduct(")
                || text.contains("subtract(")
                || text.contains("setBalance(")
                || text.contains(".balance =")
                || text.contains(".balance-"))
        {
            // Look for if condition checking balance
            let has_balance_check = text.contains("if")
                && (text.contains("balance >")
                    || text.contains("balance >=")
                    || text.contains("getBalance() >")
                    || text.contains("getBalance() >="));

            if has_balance_check {
                // Check for proper synchronization or transactions (Phase 3.1 improvement)
                let has_protection =
                    has_synchronization_context(node) || text.contains("@Transactional");

                if !has_protection {
                    let location = context.node_location(node);
                    let code = context.get_code_snippet(node);
                    let message = "Race condition: Balance check followed by withdrawal. In concurrent scenarios, balance could be modified between check and withdrawal, leading to negative balance or other inconsistencies.".to_string();

                    let mut finding =
                        Finding::new(VulnType::RaceCondition, message, location, code);
                    finding.severity = Severity::High;
                    finding.confidence = 0.25;
                    finding.remediation = "Use database-level optimistic locking (@Version), pessimistic locking, or atomic operations. Wrap the operation in a transaction with proper isolation level.".to_string();
                    findings.push(finding);
                }
            }
        }

        // Pattern 3: Check-then-act on shared resources without synchronization.
        // Analyze each `if` independently so a synchronized block elsewhere in the
        // method cannot mask an unprotected operation.
        for if_node in node
            .descendants()
            .filter(|descendant| descendant.kind() == SyntaxKind::IF_STMT)
        {
            let if_text = if_node.text().to_string();
            let collection_var = extract_collection_variable_name(&if_text);
            if collection_var.is_empty() {
                continue;
            }

            let has_action = ["get", "remove", "poll", "pop", "put", "add"]
                .iter()
                .any(|method| if_text.contains(&format!("{collection_var}.{method}(")));
            if !has_action || !is_security_relevant_collection_race(&if_node, &collection_var) {
                continue;
            }

            let location = context.node_location(&if_node);
            let code = context.get_code_snippet(&if_node);
            let message = "Potential race condition: Non-atomic check-then-act pattern on a shared collection. Another thread could modify the collection between the check and mutation.".to_string();

            let mut finding = Finding::new(VulnType::RaceCondition, message, location, code);
            finding.severity = Severity::Medium;
            finding.confidence = 0.25;
            finding.remediation =
                "Use an atomic collection operation (such as putIfAbsent or computeIfAbsent), or protect the check and mutation with the same lock.".to_string();
            findings.push(finding);
        }

        findings
    }
}
