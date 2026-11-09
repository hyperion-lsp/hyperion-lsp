// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Null pointer denial of service detection (CWE-476)
//!
//! Detects potential null pointer dereferences in request handlers that
//! could allow attackers to cause service crashes via crafted requests.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

pub struct NullPointerDosCheck;

/// Detects actual chained method calls (e.g., `.getAddress().getStreet()`)
/// vs separate statements (e.g., `service.save(); other.method();`)
///
/// Returns true only if the text contains patterns like `).methodName(`
/// which indicates a method return value is immediately used for another call.
fn has_real_chained_calls(text: &str) -> bool {
    // Look for pattern: ).identifier(
    // This matches actual chaining like .getAddress().getStreet()
    // but NOT separate statements like service.save(); other.method();

    let bytes = text.as_bytes();
    let len = bytes.len();

    for i in 0..len {
        if i + 2 >= len {
            break;
        }

        // Look for ")..." (paren followed by eventual dot with optional whitespace)
        if bytes[i] == b')' {
            let mut j = i + 1;

            // Skip whitespace after ')'
            while j < len && bytes[j].is_ascii_whitespace() {
                j += 1;
            }

            // Check for '.'
            if j < len && bytes[j] == b'.' {
                j += 1;

                // Skip whitespace after '.'
                while j < len && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }

                // Now look for identifier
                let ident_start = j;
                while j < len && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }

                // Check if we found an identifier followed by '('
                if j > ident_start && j < len && bytes[j] == b'(' {
                    return true;
                }
            }
        }
    }

    false
}

/// Checks if the method has Spring Security guarantees for non-null parameters.
///
/// Spring Security automatically ensures certain parameters are non-null in secured endpoints:
/// - Authentication/Principal parameters are guaranteed non-null
/// - @AuthenticationPrincipal annotated parameters are guaranteed non-null
/// - UserDetails parameters are guaranteed non-null
///
/// These are ONLY non-null when the endpoint is secured (has @PreAuthorize, @Secured, etc.)
/// or when Spring Security is enabled and the endpoint requires authentication.
fn has_spring_security_guarantee(text: &str) -> bool {
    // Check for @AuthenticationPrincipal annotation (clear indicator)
    if text.contains("@AuthenticationPrincipal") {
        return true;
    }

    // Helper to check if a string appears as a word (not as a substring of another word)
    fn has_word_pattern(text: &str, pattern: &str) -> bool {
        if let Some(idx) = text.find(pattern) {
            // Check character before pattern (if exists)
            if idx > 0 {
                let before = text.as_bytes()[idx - 1];
                // If character before is alphanumeric or underscore, it's part of another word
                if before.is_ascii_alphanumeric() || before == b'_' {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }

    // Check for specific Spring Security parameter patterns
    // We need to avoid false positives like "CustomAuthentication"

    // Pattern 1: "Authentication auth" or "Authentication authentication"
    // Must check that "Authentication" is not part of a longer word like "CustomAuthentication"
    if has_word_pattern(text, "Authentication auth")
        || has_word_pattern(text, "Authentication authentication")
    {
        return true;
    }

    // Pattern 2: "Principal principal"
    if has_word_pattern(text, "Principal principal") {
        return true;
    }

    // Pattern 3: "UserDetails" followed by common parameter names
    if has_word_pattern(text, "UserDetails user")
        || has_word_pattern(text, "UserDetails currentUser")
        || has_word_pattern(text, "UserDetails userDetails")
    {
        return true;
    }

    false
}

/// Checks if @RequestBody has Jackson's automatic null body validation.
///
/// Jackson (Spring's default JSON parser) automatically rejects null request bodies
/// with HTTP 400 before the controller method is called, unless:
/// - The parameter is marked as required=false
/// - Custom deserializers are used that allow null
///
/// IMPORTANT: Jackson only validates that the REQUEST BODY is not null.
/// For DTOs with nested objects, Jackson does NOT validate nested fields unless @Valid is present.
///
/// Examples:
/// - `@RequestBody String payload` → Jackson validates (String can't have nested nulls)
/// - `@RequestBody Order order` → Jackson validates `order != null`, but NOT `order.address != null`
///
/// This function returns true only for simple types (String, primitives) where Jackson
/// provides complete null protection.
fn has_jackson_body_validation(text: &str) -> bool {
    // Find @RequestBody annotation
    let request_body_idx = match text.find("@RequestBody") {
        Some(idx) => idx,
        None => return false,
    };

    let remaining_text = &text[request_body_idx..];

    // Check if there's a required=false parameter
    // Strategy: If there's a '(' after @RequestBody, extract until the matching ')'
    let after_annotation = &remaining_text[12..]; // Skip "@RequestBody"
    let has_params = after_annotation.trim_start().starts_with('(');

    if has_params {
        // Has parameters: check for required = false
        let paren_start = remaining_text
            .find('(')
            .expect("has_params guarantees '(' exists");
        let paren_end = match remaining_text[paren_start..].find(')') {
            Some(idx) => paren_start + idx,
            None => return false, // Malformed, be conservative
        };

        let annotation_with_params = &remaining_text[..=paren_end];
        if annotation_with_params.contains("required = false")
            || annotation_with_params.contains("required=false")
        {
            // Explicitly optional - can be null
            return false;
        }
    }

    // Now check if the parameter type is a simple type (String or primitive)
    // Pattern: @RequestBody[(...)] TypeName paramName
    //
    // Find the type name after the annotation (and optional parameters)

    let type_start = if has_params {
        // Find closing paren and skip to the type
        let paren_end = remaining_text
            .find(')')
            .expect("paren_end check above ensures ')' exists");
        remaining_text[paren_end + 1..].trim_start()
    } else {
        // No parameters, type comes right after @RequestBody
        after_annotation.trim_start()
    };

    // Extract the type name (first word)
    let type_name = type_start.split_whitespace().next().unwrap_or("");

    // Only provide Jackson validation guarantee for simple types
    // For complex DTOs, nested fields may be null even if the DTO itself isn't
    is_simple_type(type_name)
}

/// Check if a type is simple enough that Jackson's non-null guarantee extends to all usage
fn is_simple_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "String"
            | "int"
            | "Integer"
            | "long"
            | "Long"
            | "boolean"
            | "Boolean"
            | "double"
            | "Double"
            | "float"
            | "Float"
            | "byte"
            | "Byte"
            | "short"
            | "Short"
            | "char"
            | "Character"
            | "byte[]"
            | "String[]" // Arrays are also simple (no nested objects)
    )
}

impl Check for NullPointerDosCheck {
    fn name(&self) -> &'static str {
        "null-pointer-dos"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::CLASS_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();
        let mut findings = Vec::new();

        // Only check classes with HTTP endpoint methods
        let is_endpoint = text.contains("@GetMapping")
            || text.contains("@PostMapping")
            || text.contains("@PutMapping")
            || text.contains("@DeleteMapping")
            || text.contains("@PatchMapping")
            || text.contains("@RequestMapping")
            || text.contains("@Path(");

        if !is_endpoint {
            return Vec::new();
        }

        // Pattern 1: Optional.get() without isPresent() check
        if text.contains("Optional") && text.contains(".get()") {
            // Check if there's NO isPresent() or isEmpty() check
            let has_presence_check = text.contains("isPresent()")
                || text.contains("isEmpty()")
                || text.contains("ifPresent(")
                || text.contains("orElse(")
                || text.contains("orElseThrow(")
                || text.contains("orElseGet(");

            if !has_presence_check {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Null pointer DoS: Optional.get() called without checking isPresent(). If the Optional is empty, this will throw NoSuchElementException and crash the service.".to_string();

                let mut finding = Finding::new(VulnType::NullPointerDos, message, location, code);
                finding.severity = Severity::Medium;
                finding.remediation = "Use orElseThrow() with a proper exception or check isPresent() before calling get(). Better yet, use orElse(), orElseGet(), or ifPresent() to avoid the get() call entirely.".to_string();
                findings.push(finding);
            }
        }

        // Pattern 2: @RequestParam without validation and direct usage
        if text.contains("@RequestParam") {
            // Look for @RequestParam without required=false or without @NotNull/@Valid
            let has_validation = text.contains("@NotNull")
                || text.contains("@Valid")
                || text.contains("@Validated")
                || text.contains("required = true")
                || text.contains("required=true");

            // If it has required=false, it's potentially nullable
            let is_optional_param =
                text.contains("required = false") || text.contains("required=false");

            // Check for direct method calls on parameters (e.g., param.length(), param.getX())
            // This is a heuristic - we look for patterns like ".something()" after parameter-like names
            let has_method_calls = text.contains("id.")
                || text.contains("name.")
                || text.contains("email.")
                || text.contains("value.");

            if is_optional_param && has_method_calls && !has_validation {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Null pointer DoS: @RequestParam with required=false used without null check. Attackers can omit the parameter to cause NullPointerException.".to_string();

                let mut finding = Finding::new(VulnType::NullPointerDos, message, location, code);
                finding.severity = Severity::Medium;
                finding.remediation =
                    "Add null check before using the parameter, or use @NotNull annotation to ensure it's present. Alternatively, use Optional<T> as the parameter type.".to_string();
                findings.push(finding);
            }
        }

        // Pattern 3: @PathVariable without validation used directly
        if text.contains("@PathVariable") {
            // Check if @Valid or @NotNull is present
            let has_validation =
                text.contains("@NotNull") || text.contains("@Valid") || text.contains("@Validated");

            // PathVariables can be null in some cases (e.g., optional path segments)
            // Look for direct method calls without null checks
            let has_findby_get = text.contains("findById(")
                && (text.contains(".get()")
                    || text.contains(".getName(")
                    || text.contains(".getId("));

            if has_findby_get && !has_validation {
                // Check if there's a null check or Optional handling
                let has_null_handling = text.contains("if (")
                    && (text.contains("!= null") || text.contains("== null"))
                    || text.contains("isPresent()")
                    || text.contains("orElse");

                if !has_null_handling {
                    let location = context.node_location(node);
                    let code = context.get_code_snippet(node);
                    let message = "Null pointer DoS: Object retrieved by ID used without null check. If findById() returns empty Optional or null, accessing methods will throw NullPointerException.".to_string();

                    let mut finding =
                        Finding::new(VulnType::NullPointerDos, message, location, code);
                    finding.severity = Severity::Medium;
                    finding.remediation = "Check if the object exists before accessing its methods. Use Optional.orElseThrow() with proper 404 error, or check for null explicitly.".to_string();
                    findings.push(finding);
                }
            }
        }

        // Pattern 4: @RequestBody field access without null check
        if text.contains("@RequestBody") {
            // Check if @RequestBody parameter has @Valid, @Validated, or @NotNull
            // Bean Validation handles null checks when these annotations are present
            let has_bean_validation = {
                // Look for @Valid/@Validated/@NotNull near @RequestBody
                // Pattern: @Valid @RequestBody or @Valid\n    @RequestBody
                text.find("@RequestBody").is_some_and(|idx| {
                    // Check 100 chars before @RequestBody for validation annotations
                    let start = idx.saturating_sub(100);
                    let prefix = &text[start..idx];
                    prefix.contains("@Valid")
                        || prefix.contains("@Validated")
                        || prefix.contains("@NotNull")
                })
            };

            // Check for Spring Security guarantees (Authentication param = secured endpoint)
            let has_security_guarantee = has_spring_security_guarantee(&text);

            // Check for Jackson's automatic null body validation
            let has_jackson_validation = has_jackson_body_validation(&text);

            // Only flag if:
            // 1. No bean validation annotations
            // 2. Actual chained method calls exist (not just "contains 2 dots")
            // 3. No Spring Security guarantee (Authentication param)
            // 4. No Jackson null body protection (required=false would disable it)
            if !has_bean_validation
                && !has_security_guarantee
                && !has_jackson_validation
                && has_real_chained_calls(&text)
            {
                // Check for explicit null checks in the method
                let has_null_check = text.contains("!= null") || text.contains("== null");

                if !has_null_check {
                    let location = context.node_location(node);
                    let code = context.get_code_snippet(node);
                    let message = "Null pointer DoS: Chained method calls on @RequestBody without validation. If any object in the chain is null, NullPointerException will crash the service.".to_string();

                    let mut finding =
                        Finding::new(VulnType::NullPointerDos, message, location, code);
                    finding.severity = Severity::Medium;
                    finding.remediation = "Add @Valid annotation to the @RequestBody parameter and use @NotNull on required fields. Alternatively, add explicit null checks for nested fields.".to_string();
                    findings.push(finding);
                }
            }
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_real_chained_calls_true_cases() {
        // TRUE POSITIVE: Actual chained calls
        assert!(has_real_chained_calls("order.getAddress().getStreet()"));
        assert!(has_real_chained_calls("user.getProfile().getName()"));
        assert!(has_real_chained_calls("request.getData().toString()"));
        assert!(has_real_chained_calls("obj.get().isEmpty()"));
    }

    #[test]
    fn test_has_real_chained_calls_false_cases() {
        // FALSE POSITIVE: Separate method calls
        assert!(!has_real_chained_calls(
            "service.save(order); emailService.send();"
        ));
        assert!(!has_real_chained_calls("orderService.save(order);"));
        assert!(!has_real_chained_calls(
            "user.setName(name); user.setEmail(email);"
        ));

        // Single dot - not chained
        assert!(!has_real_chained_calls("order.getStreet()"));

        // No method calls at all
        assert!(!has_real_chained_calls("String name = order.name;"));
    }

    #[test]
    fn test_has_real_chained_calls_edge_cases() {
        // Method call with spaces (should still detect)
        assert!(has_real_chained_calls("order.getAddress()  .  getStreet()"));

        // Nested in other code
        assert!(has_real_chained_calls(
            "if (true) { String s = order.getAddress().getStreet(); }"
        ));

        // Multiple chains in same text
        assert!(has_real_chained_calls("a.b().c(); d.e().f();"));
    }

    #[test]
    fn test_has_spring_security_guarantee_with_authentication() {
        // TRUE: Authentication parameter variants
        assert!(has_spring_security_guarantee(
            "public void test(Authentication auth) {}"
        ));
        assert!(has_spring_security_guarantee(
            "public void test(Authentication authentication) {}"
        ));
        assert!(has_spring_security_guarantee(
            "@PostMapping\npublic ResponseEntity<?> create(Authentication auth, @RequestBody Req req) {}"
        ));
    }

    #[test]
    fn test_has_spring_security_guarantee_with_principal() {
        // TRUE: Principal parameter
        assert!(has_spring_security_guarantee(
            "public void test(Principal principal) {}"
        ));
        assert!(has_spring_security_guarantee(
            "@GetMapping\npublic String get(Principal principal) { return principal.getName(); }"
        ));
    }

    #[test]
    fn test_has_spring_security_guarantee_with_annotation() {
        // TRUE: @AuthenticationPrincipal annotation
        assert!(has_spring_security_guarantee(
            "public void test(@AuthenticationPrincipal UserDetails user) {}"
        ));
        assert!(has_spring_security_guarantee(
            "public void test(@AuthenticationPrincipal String username) {}"
        ));
    }

    #[test]
    fn test_has_spring_security_guarantee_with_userdetails() {
        // TRUE: UserDetails parameter with common names
        assert!(has_spring_security_guarantee(
            "public void test(UserDetails user) {}"
        ));
        assert!(has_spring_security_guarantee(
            "public void test(UserDetails currentUser) {}"
        ));
        assert!(has_spring_security_guarantee(
            "public void test(UserDetails userDetails) {}"
        ));
        assert!(has_spring_security_guarantee(
            "@PostMapping\npublic ResponseEntity<?> create(UserDetails currentUser) {}"
        ));
    }

    #[test]
    fn test_has_spring_security_guarantee_false_cases() {
        // FALSE: No Spring Security parameters
        assert!(!has_spring_security_guarantee(
            "public void test(@RequestBody Request req) {}"
        ));
        assert!(!has_spring_security_guarantee(
            "@GetMapping\npublic String get() {}"
        ));
        assert!(!has_spring_security_guarantee(
            "public void test(String name, int id) {}"
        ));

        // FALSE: Similar but not exact matches
        assert!(!has_spring_security_guarantee(
            "public void test(CustomAuthentication auth) {}"
        ));
        assert!(!has_spring_security_guarantee(
            "// Authentication parameter would be here"
        ));
    }

    #[test]
    fn test_has_jackson_body_validation_default_required() {
        // TRUE: @RequestBody without required=false for SIMPLE types (Jackson validates completely)
        assert!(has_jackson_body_validation(
            "public void test(@RequestBody String payload) {}"
        ));
        assert!(has_jackson_body_validation("@RequestBody Integer count"));

        // FALSE: Complex DTOs (Jackson only validates top level, not nested fields)
        assert!(!has_jackson_body_validation(
            "public void test(@RequestBody Request req) {}"
        ));
        assert!(!has_jackson_body_validation(
            "@PostMapping\npublic ResponseEntity<?> create(@RequestBody CreateRequest request) {}"
        ));
    }

    #[test]
    fn test_has_jackson_body_validation_explicit_required_true() {
        // TRUE: Explicitly marked as required for simple types
        assert!(has_jackson_body_validation(
            "@RequestBody(required = true) String payload"
        ));

        // FALSE: Complex DTOs even with required=true (nested fields may be null)
        assert!(!has_jackson_body_validation(
            "@RequestBody(required = true) Request req"
        ));
    }

    #[test]
    fn test_has_jackson_body_validation_optional_body() {
        // FALSE: Optional @RequestBody (required=false)
        assert!(!has_jackson_body_validation(
            "@RequestBody(required = false) Request req"
        ));
        assert!(!has_jackson_body_validation(
            "@RequestBody(required=false) Request req"
        ));
        assert!(!has_jackson_body_validation(
            "@PostMapping\npublic void test(@RequestBody(required = false) Request req) {}"
        ));
    }

    #[test]
    fn test_has_jackson_body_validation_no_request_body() {
        // FALSE: No @RequestBody present
        assert!(!has_jackson_body_validation(
            "public void test(@RequestParam String name) {}"
        ));
        assert!(!has_jackson_body_validation(
            "@GetMapping\npublic String get() {}"
        ));
        assert!(!has_jackson_body_validation("@PathVariable Long id"));
    }

    #[test]
    fn test_has_jackson_body_validation_with_other_params_having_required_false() {
        // TRUE: @RequestBody without required=false, even though @RequestHeader has it
        // This is a common webhook pattern
        let code = r#"
            @PostMapping("/webhook")
            public ResponseEntity<?> handleWebhook(
                @RequestBody String payload,
                @RequestHeader(value = "Stripe-Signature", required = false) String signature
            ) {
                return ResponseEntity.ok("success");
            }
        "#;
        assert!(has_jackson_body_validation(code));
    }

    #[test]
    fn test_has_jackson_body_validation_request_body_with_required_false() {
        // FALSE: @RequestBody explicitly marked as optional
        let code = r#"
            @PostMapping("/test")
            public ResponseEntity<?> test(
                @RequestBody(required = false) String payload
            ) {
                return ResponseEntity.ok("success");
            }
        "#;
        assert!(!has_jackson_body_validation(code));
    }
}
