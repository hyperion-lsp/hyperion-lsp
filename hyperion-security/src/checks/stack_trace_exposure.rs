// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Stack trace exposure detection
//!
//! Detects when exception stack traces or detailed error messages are exposed
//! to end users, potentially revealing sensitive system information.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Check for stack trace exposure (CWE-209)
///
/// Detects:
/// - e.printStackTrace() in web handlers
/// - Returning exception.getMessage() or exception.toString() to clients
/// - @ExceptionHandler returning full exception details
pub struct StackTraceExposureCheck;

impl StackTraceExposureCheck {
    /// Methods that expose stack trace information
    const DANGEROUS_EXCEPTION_METHODS: &'static [&'static str] =
        &["printStackTrace", "toString", "getStackTrace", "getCause"];

    fn adjacent_modifier_text(node: &SyntaxNode) -> String {
        node.prev_sibling()
            .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST)
            .map_or_else(String::new, |modifiers| modifiers.text().to_string())
    }

    /// Exception types safe to return getMessage() to clients (validation exceptions)
    ///
    /// These exceptions typically contain developer-controlled messages that are
    /// designed to be shown to end users (e.g., "Email already exists").
    const SAFE_VALIDATION_EXCEPTIONS: &'static [&'static str] = &[
        "IllegalArgumentException",
        "IllegalStateException",
        "EntityNotFoundException",
        "BadRequestException",
        "ValidationException",
        "MethodArgumentNotValidException",
        "ConstraintViolationException",
        "NoSuchElementException",
        // Additional common validation exceptions
        "ResourceNotFoundException",
        "ConflictException",
        "AlreadyExistsException",
        "DuplicateKeyException",
        "InvalidFormatException",
        "MissingServletRequestParameterException",
        "HttpMessageNotReadableException",
        "BindException",
        "TypeMismatchException",
        // Custom application exceptions (common naming patterns)
        "BusinessException",
        "DomainException",
        "ApplicationException",
    ];

    /// Exception types that leak system internals (dangerous)
    ///
    /// These exceptions can contain sensitive information about database schema,
    /// file paths, class names, or other implementation details that should not
    /// be exposed to end users.
    const DANGEROUS_EXCEPTIONS: &'static [&'static str] = &[
        "SQLException",
        "IOException",
        "FileNotFoundException",
        "NullPointerException",
        "ClassCastException",
        "NoSuchMethodException",
        "ClassNotFoundException",
        "OutOfMemoryError",
        "StackOverflowError",
    ];

    /// Check if this is in a main() method (CLI tool context)
    ///
    /// CLI tools can safely print stack traces to stderr without security concerns,
    /// as they're not exposed over the network.
    fn is_in_main_method(node: &SyntaxNode) -> bool {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == SyntaxKind::METHOD_DECL {
                let method_text = parent.text().to_string();
                return method_text.contains("public static void main(String")
                    || method_text.contains("public static void main(String[]");
            }
            current = parent.parent();
        }
        false
    }

    /// Normalize an exception type name by:
    /// 1. Stripping package qualifiers (java.lang.IllegalArgumentException -> IllegalArgumentException)
    /// 2. Removing generic type parameters (ValidationException<User> -> ValidationException)
    fn normalize_exception_type(type_name: &str) -> String {
        let mut normalized = type_name.trim().to_string();

        // Strip generic type parameters: ValidationException<User> -> ValidationException
        if let Some(generic_start) = normalized.find('<') {
            normalized = normalized[..generic_start].to_string();
        }

        // Strip package qualifiers: java.lang.IllegalArgumentException -> IllegalArgumentException
        if let Some(last_dot) = normalized.rfind('.') {
            normalized = normalized[last_dot + 1..].to_string();
        }

        normalized
    }

    /// Check if exception name matches common custom validation patterns
    fn looks_like_custom_validation_exception(type_name: &str) -> bool {
        // Custom exceptions often have these suffixes
        type_name.ends_with("ValidationException")
            || type_name.ends_with("BusinessException")
            || type_name.ends_with("DomainException")
            || type_name.ends_with("ApplicationException")
            || type_name.ends_with("BadRequestException")
            || type_name.ends_with("InvalidException")
            || type_name.ends_with("ConflictException")
            || type_name.ends_with("NotFoundException")
    }

    /// Check if this code is in a catch block for a safe validation exception
    ///
    /// Safe validation exceptions (IllegalArgumentException, ValidationException, etc.)
    /// contain developer-controlled messages that are designed to be shown to users.
    /// Returning getMessage() for these is typically safe.
    ///
    /// Handles:
    /// - Fully qualified names: catch (java.lang.IllegalArgumentException e)
    /// - Generic parameters: catch (ValidationException<User> e)
    /// - Multi-catch: catch (IllegalArgumentException | IllegalStateException e)
    /// - Custom exceptions: catch (PaymentValidationException e)
    ///
    /// Returns:
    /// - `true` if in a catch block for a SAFE validation exception
    /// - `false` if in a catch block for a DANGEROUS exception
    /// - `false` if in a catch block for generic Exception/Throwable (too broad)
    /// - `false` if not in a catch block
    fn is_safe_exception_type(node: &SyntaxNode) -> bool {
        let mut current = node.parent();
        while let Some(parent) = current {
            let text = parent.text().to_string();

            // Look for catch clause pattern: catch (ExceptionType varName)
            if text.contains("catch") && text.contains('(') {
                // Extract the exception type from the catch clause
                // Pattern: catch (ExceptionType ...)
                if let Some(catch_pos) = text.find("catch") {
                    let after_catch = &text[catch_pos..];
                    if let Some(paren_start) = after_catch.find('(') {
                        let after_paren = &after_catch[paren_start + 1..];
                        if let Some(paren_end) = after_paren.find(')') {
                            let catch_params = after_paren[..paren_end].trim();

                            // Handle multi-catch: IllegalArgumentException | IllegalStateException
                            // Check ALL types in the union - ALL must be safe for this to be safe
                            if catch_params.contains('|') {
                                let all_safe = catch_params.split('|').all(|type_part| {
                                    // Extract just the type name (remove variable name)
                                    let exception_type =
                                        type_part.split_whitespace().next().unwrap_or("");
                                    let normalized = Self::normalize_exception_type(exception_type);
                                    Self::is_exception_safe(&normalized)
                                });

                                return all_safe;
                            }

                            // Single exception type - extract it
                            let exception_type =
                                catch_params.split_whitespace().next().unwrap_or("");

                            // Normalize the exception type
                            let normalized = Self::normalize_exception_type(exception_type);

                            return Self::is_exception_safe(&normalized);
                        }
                    }
                }
            }

            current = parent.parent();
        }
        false
    }

    /// Check if a single exception type (already normalized) is safe to expose getMessage()
    fn is_exception_safe(exception_type: &str) -> bool {
        // Check if it's a known safe validation exception
        if Self::SAFE_VALIDATION_EXCEPTIONS.contains(&exception_type) {
            return true;
        }

        // Check if it's a known dangerous exception
        if Self::DANGEROUS_EXCEPTIONS.contains(&exception_type) {
            return false;
        }

        // If it's exactly "Exception" or "Throwable", it's too broad (unsafe)
        if exception_type == "Exception" || exception_type == "Throwable" {
            return false;
        }

        // Check if it looks like a custom validation exception
        if Self::looks_like_custom_validation_exception(exception_type) {
            return true;
        }

        // Unknown exception type - be conservative and treat as unsafe
        false
    }

    /// Admin path indicators to check for in request mappings
    const ADMIN_PATH_INDICATORS: &'static [&'static str] = &[
        "/admin",
        "/internal",
        "/diagnostics",
        "/debug",
        "/management",
        "/actuator",
    ];

    /// Get a class-level Spring or JAX-RS request path if present.
    ///
    /// Walks up to find the containing CLASS_DECL, then extracts the
    /// @RequestMapping path value if present.
    ///
    /// Examples:
    /// - `@RequestMapping("/api/admin")` → returns `Some("/api/admin")`
    /// - `@RequestMapping(value = "/api")` → returns `Some("/api")`
    /// - No @RequestMapping → returns `None`
    fn get_class_request_mapping(node: &SyntaxNode) -> Option<String> {
        let mut current = node.parent();

        while let Some(parent) = current {
            if parent.kind() == SyntaxKind::CLASS_DECL {
                let class_text = Self::adjacent_modifier_text(&parent);

                // Look for @RequestMapping or @Path at class level.
                if let Some(mapping_pos) = class_text
                    .find("@RequestMapping")
                    .or_else(|| class_text.find("@Path"))
                {
                    let after_mapping = &class_text[mapping_pos..];

                    // Find the path value - could be:
                    // @RequestMapping("/path")
                    // @RequestMapping(value = "/path")
                    // @RequestMapping(path = "/path")
                    if let Some(quote_pos) =
                        after_mapping.find('"').or_else(|| after_mapping.find('\''))
                    {
                        let after_quote = &after_mapping[quote_pos + 1..];
                        if let Some(end_quote) =
                            after_quote.find('"').or_else(|| after_quote.find('\''))
                        {
                            let path = &after_quote[..end_quote];
                            return Some(path.to_string());
                        }
                    }
                }
                break;
            }
            current = parent.parent();
        }
        None
    }

    /// Check if the file is in an admin-related package
    ///
    /// Detects admin package structure like:
    /// - `*.admin.*`
    /// - `*.internal.*`
    /// - `*.diagnostics.*`
    fn is_admin_package(context: &CheckContext) -> bool {
        let file_path = context.file.to_string_lossy();
        file_path.contains("/admin/")
            || file_path.contains("/internal/")
            || file_path.contains("/diagnostics/")
    }

    /// Check if a path contains admin indicators
    fn path_contains_admin_indicator(path: &str) -> bool {
        Self::ADMIN_PATH_INDICATORS
            .iter()
            .any(|indicator| path.contains(indicator))
    }

    /// Check if this is in an admin/internal endpoint
    ///
    /// Admin endpoints are restricted to authorized users and may expose
    /// detailed diagnostics for troubleshooting.
    ///
    /// Checks in order:
    /// 1. Package structure (*.admin.*, *.internal.*, etc.)
    /// 2. Class-level @RequestMapping with admin paths
    /// 3. Method-level mapping annotations with admin paths
    /// 4. @PreAuthorize with admin roles
    fn is_admin_endpoint(node: &SyntaxNode, context: &CheckContext) -> bool {
        // First check: Package structure
        if Self::is_admin_package(context) {
            return true;
        }

        // Second check: Class-level @RequestMapping
        if let Some(class_path) = Self::get_class_request_mapping(node) {
            if Self::path_contains_admin_indicator(&class_path) {
                return true;
            }
        }

        // Third check: Method-level checks
        let mut current = node.parent();

        while let Some(parent) = current {
            if parent.kind() == SyntaxKind::METHOD_DECL {
                let mut combined_text = parent.text().to_string();
                combined_text.push_str(&Self::adjacent_modifier_text(&parent));

                // Check for admin role requirements (@PreAuthorize annotations)
                let has_admin_role = combined_text.contains("@PreAuthorize")
                    && (combined_text.contains("ADMIN") || combined_text.contains("Admin"));
                let has_jax_rs_admin_role = (combined_text.contains("@RolesAllowed")
                    || combined_text.contains("@Secured"))
                    && (combined_text.contains("ADMIN") || combined_text.contains("Admin"));

                if has_admin_role || has_jax_rs_admin_role {
                    return true;
                }

                // Check for admin/internal/debug/diagnostics paths in HTTP mapping annotations
                let mapping_annotations = vec![
                    "@GetMapping",
                    "@PostMapping",
                    "@PutMapping",
                    "@DeleteMapping",
                    "@PatchMapping",
                    "@RequestMapping",
                    "@Path",
                ];

                for mapping in mapping_annotations {
                    if let Some(mapping_pos) = combined_text.find(mapping) {
                        // Find the path within this mapping annotation (look for opening paren or quote)
                        let after_mapping = &combined_text[mapping_pos..];
                        if let Some(path_start) =
                            after_mapping.find('"').or_else(|| after_mapping.find('\''))
                        {
                            let path_section = &after_mapping[path_start..];
                            // Check if any admin path appears near the start of this mapping
                            if Self::path_contains_admin_indicator(
                                &path_section[..path_section.len().min(100)],
                            ) {
                                return true;
                            }
                        }
                        // Found a mapping - if it had admin paths, we would have returned true above
                        // Continue checking other annotations (don't break early)
                    }
                }

                // Stop at method level - we already checked class level above
                break;
            }
            current = parent.parent();
        }
        false
    }

    /// Check if this looks like a web handler method
    fn is_web_handler(node: &SyntaxNode) -> bool {
        let text = Self::adjacent_modifier_text(node);

        // Spring MVC annotations
        text.contains("@RequestMapping")
            || text.contains("@GetMapping")
            || text.contains("@PostMapping")
            || text.contains("@PutMapping")
            || text.contains("@DeleteMapping")
            || text.contains("@PatchMapping")
            || text.contains("@ExceptionHandler")
            || text.contains("@RestController")
            || text.contains("@Controller")
            // JAX-RS annotations
            || text.contains("@Path")
            || text.contains("@GET")
            || text.contains("@POST")
            || text.contains("@PUT")
            || text.contains("@DELETE")
    }

    fn is_http_response_return(text: &str) -> bool {
        text.contains("ResponseEntity")
            || (text.contains("Response.")
                && (text.contains(".entity(")
                    || text.contains("Response.ok(")
                    || text.contains("Response.serverError(")))
    }

    /// Check if the containing method/class is a web component
    fn is_in_web_context(node: &SyntaxNode) -> bool {
        // Check containing method
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == SyntaxKind::METHOD_DECL || parent.kind() == SyntaxKind::CLASS_DECL {
                if Self::is_web_handler(&parent) {
                    return true;
                }
                if parent.kind() == SyntaxKind::CLASS_DECL {
                    break;
                }
            }
            current = parent.parent();
        }
        false
    }
}

impl Check for StackTraceExposureCheck {
    fn name(&self) -> &'static str {
        "stack-trace-exposure"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::EXPR_STMT,   // For statement-level printStackTrace()
            SyntaxKind::RETURN_STMT, // For return statements with exception details
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let mut findings = Vec::new();
        let text = node.text().to_string();

        // Skip if in main() method (CLI tools)
        if Self::is_in_main_method(node) {
            return vec![];
        }

        // Skip if in admin/internal endpoint
        if Self::is_admin_endpoint(node, context) {
            return vec![];
        }

        match node.kind() {
            // Only check EXPR_STMT for printStackTrace() to avoid duplicate findings
            // (EXPR_STMT is the parent of METHOD_CALL_EXPR in statement context)
            SyntaxKind::EXPR_STMT
                if text.contains(".printStackTrace()") && Self::is_in_web_context(node) =>
            {
                // Check for printStackTrace()
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Stack trace exposure: printStackTrace() writes full stack traces to stderr. \
                            In web applications, this can expose sensitive system information including file paths, \
                            class names, and internal structure to attackers who have access to logs."
                    .to_string();

                let mut finding =
                    Finding::new(VulnType::StackTraceExposure, message, location, code);
                finding.severity = Severity::Medium;
                finding.remediation =
                    "Use a logger to log exceptions server-side only (logger.error(\"Error\", e)). \
                            Return generic error messages to users without stack traces."
                        .to_string();
                findings.push(finding);
            }

            SyntaxKind::RETURN_STMT
                // Check for returning exception details to client
                if text.contains("return") && Self::is_http_response_return(&text) => {
                    for method in Self::DANGEROUS_EXCEPTION_METHODS {
                        if text.contains(&format!(".{method}()")) {
                            // Only flag if in public web endpoint
                            if Self::is_in_web_context(node)
                                && !Self::is_admin_endpoint(node, context)
                            {
                                let location = context.node_location(node);
                                let code = context.get_code_snippet(node);
                                let message = format!(
                                    "Stack trace exposure: Returning exception.{method}() to client. \
                                    Exception messages can reveal sensitive information about system internals, \
                                    database schema, file paths, or business logic."
                                );

                                let mut finding = Finding::new(
                                    VulnType::StackTraceExposure,
                                    message,
                                    location,
                                    code,
                                );
                                finding.severity = Severity::Medium;
                                finding.remediation =
                                    "Return generic error messages to users. Log detailed exceptions server-side. \
                                    Example: return ResponseEntity.status(500).body(\"An error occurred\");"
                                        .to_string();
                                findings.push(finding);
                                break;
                            }
                        }
                    }

                    // Also check for getMessage()
                    if text.contains(".getMessage()")
                        && Self::is_in_web_context(node)
                        && !Self::is_admin_endpoint(node, context)
                    {
                        // Suppress finding if this is a safe validation exception
                        if Self::is_safe_exception_type(node) {
                            // Safe validation exceptions (IllegalArgumentException, etc.)
                            // have developer-controlled messages meant for users
                            return findings;
                        }

                        let location = context.node_location(node);
                        let code = context.get_code_snippet(node);
                        let message = "Stack trace exposure: Returning exception.getMessage() to client. \
                                Exception messages can reveal sensitive information about system internals, \
                                database schema, file paths, or business logic."
                                .to_string();

                        let mut finding =
                            Finding::new(VulnType::StackTraceExposure, message, location, code);
                        finding.severity = Severity::Medium;
                        finding.remediation =
                                "Return generic error messages to users. Log detailed exceptions server-side. \
                                Example: return ResponseEntity.status(500).body(\"An error occurred\");"
                                    .to_string();
                        findings.push(finding);
                    }
                }

            _ => {}
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dangerous_methods() {
        assert!(StackTraceExposureCheck::DANGEROUS_EXCEPTION_METHODS.contains(&"printStackTrace"));
        assert!(StackTraceExposureCheck::DANGEROUS_EXCEPTION_METHODS.contains(&"toString"));
        assert!(StackTraceExposureCheck::DANGEROUS_EXCEPTION_METHODS.contains(&"getStackTrace"));
    }

    #[test]
    fn test_safe_validation_exceptions() {
        assert!(StackTraceExposureCheck::SAFE_VALIDATION_EXCEPTIONS
            .contains(&"IllegalArgumentException"));
        assert!(
            StackTraceExposureCheck::SAFE_VALIDATION_EXCEPTIONS.contains(&"ValidationException")
        );
        assert!(StackTraceExposureCheck::SAFE_VALIDATION_EXCEPTIONS
            .contains(&"EntityNotFoundException"));
        // New additions
        assert!(StackTraceExposureCheck::SAFE_VALIDATION_EXCEPTIONS
            .contains(&"ResourceNotFoundException"));
        assert!(StackTraceExposureCheck::SAFE_VALIDATION_EXCEPTIONS.contains(&"BusinessException"));
    }

    #[test]
    fn test_dangerous_exceptions() {
        assert!(StackTraceExposureCheck::DANGEROUS_EXCEPTIONS.contains(&"SQLException"));
        assert!(StackTraceExposureCheck::DANGEROUS_EXCEPTIONS.contains(&"IOException"));
        assert!(StackTraceExposureCheck::DANGEROUS_EXCEPTIONS.contains(&"NullPointerException"));
    }

    #[test]
    fn test_normalize_exception_type_strips_package() {
        assert_eq!(
            StackTraceExposureCheck::normalize_exception_type("java.lang.IllegalArgumentException"),
            "IllegalArgumentException"
        );
        assert_eq!(
            StackTraceExposureCheck::normalize_exception_type("com.example.ValidationException"),
            "ValidationException"
        );
    }

    #[test]
    fn test_normalize_exception_type_strips_generics() {
        assert_eq!(
            StackTraceExposureCheck::normalize_exception_type("ValidationException<User>"),
            "ValidationException"
        );
        assert_eq!(
            StackTraceExposureCheck::normalize_exception_type("Map<String, Object>"),
            "Map"
        );
    }

    #[test]
    fn test_normalize_exception_type_both() {
        assert_eq!(
            StackTraceExposureCheck::normalize_exception_type(
                "com.example.ValidationException<User>"
            ),
            "ValidationException"
        );
        assert_eq!(
            StackTraceExposureCheck::normalize_exception_type(
                "java.util.concurrent.ExecutionException<Result>"
            ),
            "ExecutionException"
        );
    }

    #[test]
    fn test_normalize_exception_type_simple() {
        assert_eq!(
            StackTraceExposureCheck::normalize_exception_type("IllegalArgumentException"),
            "IllegalArgumentException"
        );
        assert_eq!(
            StackTraceExposureCheck::normalize_exception_type("SQLException"),
            "SQLException"
        );
    }

    #[test]
    fn test_looks_like_custom_validation_exception() {
        assert!(
            StackTraceExposureCheck::looks_like_custom_validation_exception(
                "PaymentValidationException"
            )
        );
        assert!(
            StackTraceExposureCheck::looks_like_custom_validation_exception(
                "OrderBusinessException"
            )
        );
        assert!(
            StackTraceExposureCheck::looks_like_custom_validation_exception(
                "UserNotFoundException"
            )
        );
        assert!(
            StackTraceExposureCheck::looks_like_custom_validation_exception(
                "OrderInvalidException"
            )
        );
        assert!(!StackTraceExposureCheck::looks_like_custom_validation_exception("SQLException"));
        assert!(!StackTraceExposureCheck::looks_like_custom_validation_exception("IOException"));
    }

    #[test]
    fn test_is_exception_safe_known_safe() {
        assert!(StackTraceExposureCheck::is_exception_safe(
            "IllegalArgumentException"
        ));
        assert!(StackTraceExposureCheck::is_exception_safe(
            "ValidationException"
        ));
        assert!(StackTraceExposureCheck::is_exception_safe(
            "ResourceNotFoundException"
        ));
        assert!(StackTraceExposureCheck::is_exception_safe(
            "BusinessException"
        ));
    }

    #[test]
    fn test_is_exception_safe_known_dangerous() {
        assert!(!StackTraceExposureCheck::is_exception_safe("SQLException"));
        assert!(!StackTraceExposureCheck::is_exception_safe("IOException"));
        assert!(!StackTraceExposureCheck::is_exception_safe(
            "NullPointerException"
        ));
    }

    #[test]
    fn test_is_exception_safe_generic_exception() {
        assert!(!StackTraceExposureCheck::is_exception_safe("Exception"));
        assert!(!StackTraceExposureCheck::is_exception_safe("Throwable"));
    }

    #[test]
    fn test_is_exception_safe_custom_validation() {
        assert!(StackTraceExposureCheck::is_exception_safe(
            "PaymentValidationException"
        ));
        assert!(StackTraceExposureCheck::is_exception_safe(
            "OrderBusinessException"
        ));
        assert!(StackTraceExposureCheck::is_exception_safe(
            "UserNotFoundException"
        ));
    }

    #[test]
    fn test_is_exception_safe_unknown() {
        // Unknown exceptions should be treated as unsafe (conservative)
        assert!(!StackTraceExposureCheck::is_exception_safe(
            "SomeRandomException"
        ));
        assert!(!StackTraceExposureCheck::is_exception_safe("MyCustomError"));
    }
}
