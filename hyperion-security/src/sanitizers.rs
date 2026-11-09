// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Sanitizer detection
//!
//! Identifies methods that sanitize tainted data for specific vulnerability types.
//! Sanitizers transform untrusted input into safe values or validate input constraints.

use crate::taint::VulnTypes;
use std::collections::HashSet;

/// Registry of known sanitizers
#[derive(Clone)]
pub struct SanitizerRegistry {
    sanitizers: Vec<SanitizerPattern>,
}

/// A sanitizer pattern matching receiver types and method names
#[derive(Debug, Clone)]
pub struct SanitizerPattern {
    /// Receiver types that provide this sanitization (e.g., "Integer", "PreparedStatement")
    pub receiver_types: HashSet<String>,
    /// Method names that perform sanitization (e.g., "parseInt", "setString")
    pub method_names: HashSet<String>,
    /// Vulnerability types that this sanitizer protects against
    pub sanitizes: VulnTypes,
}

impl Default for SanitizerRegistry {
    #[allow(deprecated)]
    fn default() -> Self {
        Self::new()
    }
}

impl SanitizerRegistry {
    /// Create an empty sanitizer registry
    ///
    /// Use `register_pattern()` to add patterns manually, or use the framework
    /// abstraction layer in the `frameworks` module for structured registration.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::sanitizers::SanitizerRegistry;
    /// use hyperion_security::taint::VulnTypes;
    /// use std::collections::HashSet;
    ///
    /// let mut registry = SanitizerRegistry::empty();
    /// let methods = ["parseInt"].iter().map(|s| s.to_string()).collect();
    /// let receivers = ["Integer"].iter().map(|s| s.to_string()).collect();
    /// registry.register_pattern(receivers, methods, VulnTypes::SQL_INJECTION);
    /// ```
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            sanitizers: Vec::new(),
        }
    }

    /// Create a new registry with all default sanitizers registered
    ///
    /// This is the legacy constructor that registers all hardcoded patterns.
    /// For new code, prefer using the framework abstraction layer.
    ///
    /// # Deprecated
    ///
    /// This method is deprecated in favor of using `FrameworkRegistry` for
    /// pluggable framework support. It will be removed in a future version.
    #[must_use]
    #[deprecated(
        since = "0.1.0",
        note = "Use FrameworkRegistry for pluggable framework support"
    )]
    pub fn new() -> Self {
        let mut registry = Self {
            sanitizers: Vec::new(),
        };
        registry.register_numeric_sanitizers();
        registry.register_length_sanitizers();
        registry.register_uuid_sanitizers();
        registry.register_prepared_statement_sanitizers();
        registry.register_encoding_sanitizers();
        registry.register_validation_sanitizers();
        registry.register_regex_sanitizers();
        registry
    }

    /// Register a sanitizer pattern
    ///
    /// This is the public API for frameworks to register their sanitizer patterns.
    ///
    /// # Arguments
    /// * `receiver_types` - Set of receiver type names (e.g., "Integer", "PreparedStatement")
    /// * `method_names` - Set of method names (e.g., "parseInt", "setString")
    /// * `sanitizes` - Vulnerability types that this sanitizer protects against
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::sanitizers::SanitizerRegistry;
    /// use hyperion_security::taint::VulnTypes;
    /// use std::collections::HashSet;
    ///
    /// let mut registry = SanitizerRegistry::empty();
    ///
    /// let methods: HashSet<String> = ["parseInt", "parseLong"]
    ///     .iter().map(|s| s.to_string()).collect();
    /// let receivers: HashSet<String> = ["Integer", "Long"]
    ///     .iter().map(|s| s.to_string()).collect();
    ///
    /// registry.register_pattern(
    ///     receivers,
    ///     methods,
    ///     VulnTypes::SQL_INJECTION | VulnTypes::XSS
    /// );
    /// ```
    pub fn register_pattern(
        &mut self,
        receiver_types: HashSet<String>,
        method_names: HashSet<String>,
        sanitizes: VulnTypes,
    ) {
        self.sanitizers.push(SanitizerPattern {
            receiver_types,
            method_names,
            sanitizes,
        });
    }

    /// Register core (non-framework) sanitizer patterns.
    ///
    /// This is used by the framework registry so that baseline sanitizers are available even when
    /// scanning via `FrameworkRegistry` (CLI path). Frameworks can still add their own sanitizers
    /// on top (e.g., Spring HtmlUtils).
    pub fn register_core_sanitizers(&mut self) {
        self.register_numeric_sanitizers();
        self.register_length_sanitizers();
        self.register_uuid_sanitizers();
        self.register_prepared_statement_sanitizers();
        self.register_encoding_sanitizers();
        self.register_validation_sanitizers();
        self.register_regex_sanitizers();
    }

    /// Register numeric parsing sanitizers
    ///
    /// Methods like Integer.parseInt, Long.parseLong, etc. convert strings to numbers.
    /// Since numeric types cannot contain SQL injection, command injection, XSS, etc.,
    /// these methods sanitize for all injection vulnerabilities.
    ///
    /// **Why numeric parsing sanitizes:**
    /// - `Integer.parseInt("123; DROP TABLE users")` throws NumberFormatException
    /// - Numeric types have no special characters or syntax that could be exploited
    /// - Database drivers and command interpreters treat numbers as values, not code
    fn register_numeric_sanitizers(&mut self) {
        let numeric_methods: HashSet<String> = [
            "parseInt",
            "parseLong",
            "parseShort",
            "parseByte",
            "parseFloat",
            "parseDouble",
            "valueOf",
            "decode",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let numeric_receivers: HashSet<String> = [
            "Integer",
            "Long",
            "Short",
            "Byte",
            "Float",
            "Double",
            "BigInteger",
            "BigDecimal",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        // Numeric parsing sanitizes all injection types because numbers can't contain code
        self.sanitizers.push(SanitizerPattern {
            receiver_types: numeric_receivers,
            method_names: numeric_methods,
            sanitizes: VulnTypes::SQL_INJECTION
                | VulnTypes::COMMAND_INJECTION
                | VulnTypes::PATH_TRAVERSAL
                | VulnTypes::XSS
                | VulnTypes::SSRF
                | VulnTypes::XXE_INJECTION
                | VulnTypes::LDAP_INJECTION
                | VulnTypes::LOG_INJECTION
                | VulnTypes::TEMPLATE_INJECTION
                | VulnTypes::NOSQL_INJECTION
                | VulnTypes::EXPRESSION_LANGUAGE_INJECTION
                | VulnTypes::EMAIL_HEADER_INJECTION
                | VulnTypes::OPEN_REDIRECT,
        });
    }

    /// Register length/size sanitizers
    ///
    /// Methods like StringUtils.length return numeric counts, which cannot
    /// contain injection payloads. Treat as safe for injection flows.
    fn register_length_sanitizers(&mut self) {
        let length_methods: HashSet<String> = ["length", "size"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let length_receivers: HashSet<String> = ["StringUtils", "CharSequence"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: length_receivers,
            method_names: length_methods,
            sanitizes: VulnTypes::SQL_INJECTION
                | VulnTypes::COMMAND_INJECTION
                | VulnTypes::PATH_TRAVERSAL
                | VulnTypes::XSS
                | VulnTypes::SSRF
                | VulnTypes::XXE_INJECTION
                | VulnTypes::LDAP_INJECTION
                | VulnTypes::LOG_INJECTION
                | VulnTypes::TEMPLATE_INJECTION
                | VulnTypes::NOSQL_INJECTION
                | VulnTypes::EXPRESSION_LANGUAGE_INJECTION
                | VulnTypes::EMAIL_HEADER_INJECTION
                | VulnTypes::OPEN_REDIRECT,
        });
    }

    /// Register UUID parsing sanitizers
    ///
    /// UUID parsing enforces a strict hex pattern, making it safe for
    /// injection sinks that expect identifiers (SQL, logs, paths).
    fn register_uuid_sanitizers(&mut self) {
        let uuid_methods: HashSet<String> = ["fromString", "nameUUIDFromBytes"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let uuid_receivers: HashSet<String> = std::iter::once("UUID".to_string()).collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: uuid_receivers,
            method_names: uuid_methods,
            sanitizes: VulnTypes::SQL_INJECTION
                | VulnTypes::COMMAND_INJECTION
                | VulnTypes::PATH_TRAVERSAL
                | VulnTypes::LOG_INJECTION,
        });
    }

    /// Register PreparedStatement parameter setters
    ///
    /// PreparedStatement uses parameterized queries which prevent SQL injection.
    /// The database driver ensures parameters are properly escaped and treated as values.
    ///
    /// **Why PreparedStatement.setString sanitizes:**
    /// ```java
    /// // UNSAFE: String concatenation
    /// stmt.execute("SELECT * FROM users WHERE id = " + userId);
    ///
    /// // SAFE: Parameterized query
    /// PreparedStatement ps = conn.prepareStatement("SELECT * FROM users WHERE id = ?");
    /// ps.setString(1, userId); // Database driver handles escaping
    /// ```
    ///
    /// The `setString` call cannot cause SQL injection because the SQL structure is
    /// already defined in `prepareStatement`. The parameter is always treated as a value.
    fn register_prepared_statement_sanitizers(&mut self) {
        let ps_methods: HashSet<String> = [
            "setString",
            "setInt",
            "setLong",
            "setDouble",
            "setFloat",
            "setBoolean",
            "setByte",
            "setShort",
            "setDate",
            "setTime",
            "setTimestamp",
            "setBytes",
            "setObject",
            "setBigDecimal",
            "setNull",
            "setBlob",
            "setClob",
            "setArray",
            "setRef",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let ps_receivers: HashSet<String> = ["PreparedStatement", "ps", "pstmt", "preparedStmt"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        // PreparedStatement sanitizes SQL injection only (not other injection types)
        self.sanitizers.push(SanitizerPattern {
            receiver_types: ps_receivers,
            method_names: ps_methods,
            sanitizes: VulnTypes::SQL_INJECTION,
        });
    }

    /// Register HTML/XML encoding sanitizers
    ///
    /// These libraries encode special characters to prevent XSS attacks.
    /// - `<` becomes `&lt;`
    /// - `>` becomes `&gt;`
    /// - `"` becomes `&quot;`
    ///
    /// **Common libraries:**
    /// - Apache Commons Text: `StringEscapeUtils.escapeHtml4/escapeXml`
    /// - OWASP Java Encoder: `Encode.forHtml/forJavaScript/forXml`
    /// - Spring Web: `HtmlUtils.htmlEscape`
    fn register_encoding_sanitizers(&mut self) {
        // Apache Commons Text / Commons Lang
        let commons_methods: HashSet<String> = [
            "escapeHtml",
            "escapeHtml4",
            "escapeXml",
            "escapeXml10",
            "escapeXml11",
            "escapeEcmaScript",
            "escapeJson",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let commons_receivers: HashSet<String> =
            std::iter::once("StringEscapeUtils".to_string()).collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: commons_receivers,
            method_names: commons_methods,
            sanitizes: VulnTypes::XSS | VulnTypes::XXE_INJECTION,
        });

        // OWASP Java Encoder
        let owasp_methods: HashSet<String> = [
            "forHtml",
            "forHtmlAttribute",
            "forHtmlContent",
            "forJavaScript",
            "forJavaScriptAttribute",
            "forJavaScriptBlock",
            "forJavaScriptSource",
            "forCssString",
            "forCssUrl",
            "forUri",
            "forUriComponent",
            "forXml",
            "forXmlAttribute",
            "forXmlContent",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let owasp_receivers: HashSet<String> = ["Encode", "Encoder"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: owasp_receivers,
            method_names: owasp_methods,
            sanitizes: VulnTypes::XSS | VulnTypes::XXE_INJECTION | VulnTypes::OPEN_REDIRECT,
        });

        // Spring HtmlUtils
        let spring_methods: HashSet<String> = ["htmlEscape", "htmlEscapeDecimal", "htmlEscapeHex"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let spring_receivers: HashSet<String> = std::iter::once("HtmlUtils".to_string()).collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: spring_receivers,
            method_names: spring_methods,
            sanitizes: VulnTypes::XSS,
        });

        // Google Guava
        let guava_methods: HashSet<String> = std::iter::once("escape".to_string()).collect();

        let guava_receivers: HashSet<String> = ["HtmlEscapers", "XmlEscapers", "UrlEscapers"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: guava_receivers,
            method_names: guava_methods,
            sanitizes: VulnTypes::XSS | VulnTypes::XXE_INJECTION | VulnTypes::OPEN_REDIRECT,
        });
    }

    /// Register path and file validation sanitizers
    ///
    /// These methods prevent path traversal attacks by:
    /// - Resolving `..` and `.` components
    /// - Converting to canonical (absolute) paths
    /// - Removing symbolic link indirection
    ///
    /// **Example:**
    /// ```java
    /// // UNSAFE: User input directly used
    /// String path = "/app/files/" + userInput;  // userInput = "../../etc/passwd"
    /// File f = new File(path);  // Path traversal!
    ///
    /// // SAFE: Canonical path validation
    /// File f = new File("/app/files/" + userInput).getCanonicalFile();
    /// if (!f.getPath().startsWith("/app/files/")) {
    ///     throw new SecurityException("Invalid path");
    /// }
    /// ```
    fn register_validation_sanitizers(&mut self) {
        // Path normalization methods
        let path_methods: HashSet<String> = ["normalize", "toAbsolutePath", "toRealPath"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let path_receivers: HashSet<String> =
            ["Path", "path"].iter().map(|s| (*s).to_string()).collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: path_receivers,
            method_names: path_methods,
            sanitizes: VulnTypes::PATH_TRAVERSAL,
        });

        // File canonical path methods
        let file_methods: HashSet<String> = ["getCanonicalPath", "getCanonicalFile"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let file_receivers: HashSet<String> =
            ["File", "file"].iter().map(|s| (*s).to_string()).collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: file_receivers,
            method_names: file_methods,
            sanitizes: VulnTypes::PATH_TRAVERSAL,
        });

        // URL validation (java.net.URI)
        let uri_methods: HashSet<String> = std::iter::once("normalize".to_string()).collect();

        let uri_receivers: HashSet<String> =
            ["URI", "uri"].iter().map(|s| (*s).to_string()).collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: uri_receivers,
            method_names: uri_methods,
            sanitizes: VulnTypes::SSRF | VulnTypes::OPEN_REDIRECT,
        });

        // URL/redirect validation helpers (heuristics)
        let url_methods: HashSet<String> = [
            "isValid",
            "isAllowed",
            "isAllowedHost",
            "isTrustedHost",
            "validateUrl",
            "validateRedirect",
            "validateHost",
            "checkHost",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let url_receivers: HashSet<String> = [
            "UrlValidator",
            "URLValidator",
            "UrlUtils",
            "UriUtils",
            "URL",
            "URI",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: url_receivers,
            method_names: url_methods,
            sanitizes: VulnTypes::SSRF | VulnTypes::OPEN_REDIRECT,
        });
    }

    /// Register regex sanitizers
    ///
    /// Pattern.quote escapes user input so it is treated as a literal in regexes,
    /// preventing regex injection and ReDoS issues from user-controlled patterns.
    fn register_regex_sanitizers(&mut self) {
        let pattern_methods: HashSet<String> = std::iter::once("quote".to_string()).collect();

        let pattern_receivers: HashSet<String> = ["Pattern", "pattern"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        self.sanitizers.push(SanitizerPattern {
            receiver_types: pattern_receivers,
            method_names: pattern_methods,
            sanitizes: VulnTypes::REDOS,
        });
    }

    /// Check if a method call is a sanitizer and return which vulnerabilities it sanitizes
    ///
    /// # Arguments
    /// * `receiver` - Optional receiver type/name (e.g., "Integer", "PreparedStatement")
    /// * `method` - Method name (e.g., "parseInt", "setString")
    ///
    /// # Returns
    /// `Some(VulnTypes)` if this is a known sanitizer, indicating which vulnerabilities
    /// it protects against. `None` if not a sanitizer.
    ///
    /// # Examples
    /// ```
    /// use hyperion_security::{SanitizerRegistry, VulnTypes};
    ///
    /// let registry = SanitizerRegistry::new();
    ///
    /// // Integer.parseInt sanitizes all injection types
    /// assert!(registry.is_sanitizer(Some("Integer"), "parseInt").is_some());
    ///
    /// // PreparedStatement.setString sanitizes SQL injection
    /// let sanitizes = registry.is_sanitizer(Some("PreparedStatement"), "setString").unwrap();
    /// assert!(sanitizes.contains(VulnTypes::SQL_INJECTION));
    ///
    /// // Not a sanitizer
    /// assert!(registry.is_sanitizer(Some("String"), "concat").is_none());
    /// ```
    #[must_use]
    pub fn is_sanitizer(&self, receiver: Option<&str>, method: &str) -> Option<VulnTypes> {
        for pattern in &self.sanitizers {
            if pattern.method_names.contains(method) {
                // If we have a receiver, check it matches
                if let Some(recv) = receiver {
                    if pattern.receiver_types.contains(recv) {
                        return Some(pattern.sanitizes);
                    }
                    // Fuzzy matching: check if receiver ends with type name
                    // e.g., "myPreparedStatement" matches "PreparedStatement"
                    for recv_type in &pattern.receiver_types {
                        if recv.ends_with(recv_type)
                            || recv.to_lowercase() == recv_type.to_lowercase()
                        {
                            return Some(pattern.sanitizes);
                        }
                    }
                } else {
                    // No receiver - could still be a sanitizer if method name is distinctive
                    // Be conservative here: only match if ALL patterns for this method have the same sanitization
                    // This handles static method calls like StringEscapeUtils.escapeHtml(...)
                    let matching_patterns: Vec<_> = self
                        .sanitizers
                        .iter()
                        .filter(|p| p.method_names.contains(method))
                        .collect();

                    if matching_patterns.len() == 1 {
                        return Some(matching_patterns[0].sanitizes);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frameworks::FrameworkRegistry;

    fn builtin_sanitizer_registry() -> SanitizerRegistry {
        let mut fw = FrameworkRegistry::with_builtin();
        fw.sanitizer_registry().clone()
    }

    #[test]
    fn test_numeric_sanitizers() {
        let registry = builtin_sanitizer_registry();

        // Integer.parseInt
        let sanitizes = registry.is_sanitizer(Some("Integer"), "parseInt").unwrap();
        assert!(sanitizes.contains(VulnTypes::SQL_INJECTION));
        assert!(sanitizes.contains(VulnTypes::COMMAND_INJECTION));
        assert!(sanitizes.contains(VulnTypes::XSS));
        assert!(sanitizes.contains(VulnTypes::PATH_TRAVERSAL));

        // Long.parseLong
        assert!(registry.is_sanitizer(Some("Long"), "parseLong").is_some());

        // Double.parseDouble
        assert!(registry
            .is_sanitizer(Some("Double"), "parseDouble")
            .is_some());

        // Float.parseFloat
        assert!(registry.is_sanitizer(Some("Float"), "parseFloat").is_some());

        // Short.parseShort
        assert!(registry.is_sanitizer(Some("Short"), "parseShort").is_some());

        // Byte.parseByte
        assert!(registry.is_sanitizer(Some("Byte"), "parseByte").is_some());

        // Integer.valueOf
        assert!(registry.is_sanitizer(Some("Integer"), "valueOf").is_some());

        // BigInteger
        assert!(registry
            .is_sanitizer(Some("BigInteger"), "parseInt")
            .is_some());
    }

    #[test]
    fn test_prepared_statement_sanitizers() {
        let registry = builtin_sanitizer_registry();

        // PreparedStatement.setString
        let sanitizes = registry
            .is_sanitizer(Some("PreparedStatement"), "setString")
            .unwrap();
        assert!(sanitizes.contains(VulnTypes::SQL_INJECTION));
        assert!(!sanitizes.contains(VulnTypes::XSS)); // Only sanitizes SQL

        // Other PreparedStatement setters
        assert!(registry
            .is_sanitizer(Some("PreparedStatement"), "setInt")
            .is_some());
        assert!(registry
            .is_sanitizer(Some("PreparedStatement"), "setLong")
            .is_some());
        assert!(registry
            .is_sanitizer(Some("PreparedStatement"), "setDouble")
            .is_some());
        assert!(registry
            .is_sanitizer(Some("PreparedStatement"), "setObject")
            .is_some());

        // Fuzzy matching
        assert!(registry.is_sanitizer(Some("ps"), "setString").is_some());
        assert!(registry.is_sanitizer(Some("pstmt"), "setString").is_some());
    }

    #[test]
    fn test_encoding_sanitizers() {
        let registry = builtin_sanitizer_registry();

        // Apache Commons
        let sanitizes = registry
            .is_sanitizer(Some("StringEscapeUtils"), "escapeHtml4")
            .unwrap();
        assert!(sanitizes.contains(VulnTypes::XSS));

        assert!(registry
            .is_sanitizer(Some("StringEscapeUtils"), "escapeXml")
            .is_some());

        // OWASP Encoder
        let sanitizes = registry.is_sanitizer(Some("Encode"), "forHtml").unwrap();
        assert!(sanitizes.contains(VulnTypes::XSS));

        assert!(registry
            .is_sanitizer(Some("Encode"), "forJavaScript")
            .is_some());
        assert!(registry.is_sanitizer(Some("Encode"), "forXml").is_some());

        // Spring HtmlUtils
        assert!(registry
            .is_sanitizer(Some("HtmlUtils"), "htmlEscape")
            .is_some());

        // Google Guava
        assert!(registry
            .is_sanitizer(Some("HtmlEscapers"), "escape")
            .is_some());
        assert!(registry
            .is_sanitizer(Some("XmlEscapers"), "escape")
            .is_some());
    }

    #[test]
    fn test_path_validation_sanitizers() {
        let registry = builtin_sanitizer_registry();

        // Path.normalize
        let sanitizes = registry.is_sanitizer(Some("Path"), "normalize").unwrap();
        assert!(sanitizes.contains(VulnTypes::PATH_TRAVERSAL));

        // File.getCanonicalPath
        let sanitizes = registry
            .is_sanitizer(Some("File"), "getCanonicalPath")
            .unwrap();
        assert!(sanitizes.contains(VulnTypes::PATH_TRAVERSAL));

        // URI.normalize
        let sanitizes = registry.is_sanitizer(Some("URI"), "normalize").unwrap();
        assert!(sanitizes.contains(VulnTypes::SSRF));
        assert!(sanitizes.contains(VulnTypes::OPEN_REDIRECT));
    }

    #[test]
    fn test_regex_sanitizers() {
        let registry = builtin_sanitizer_registry();

        let sanitizes = registry.is_sanitizer(Some("Pattern"), "quote").unwrap();
        assert!(sanitizes.contains(VulnTypes::REDOS));
    }

    #[test]
    fn test_uuid_sanitizers() {
        let registry = builtin_sanitizer_registry();

        let sanitizes = registry.is_sanitizer(Some("UUID"), "fromString").unwrap();
        assert!(sanitizes.contains(VulnTypes::SQL_INJECTION));
        assert!(sanitizes.contains(VulnTypes::LOG_INJECTION));
        assert!(sanitizes.contains(VulnTypes::PATH_TRAVERSAL));
    }

    #[test]
    fn test_non_sanitizers() {
        let registry = builtin_sanitizer_registry();

        // String methods are not sanitizers
        assert!(registry.is_sanitizer(Some("String"), "concat").is_none());
        assert!(registry.is_sanitizer(Some("String"), "toString").is_none());

        // Random method names
        assert!(registry.is_sanitizer(Some("Foo"), "bar").is_none());
        assert!(registry.is_sanitizer(None, "unknownMethod").is_none());
    }

    #[test]
    fn test_fuzzy_receiver_matching() {
        let registry = builtin_sanitizer_registry();

        // Variable names that end with type name
        assert!(registry
            .is_sanitizer(Some("myPreparedStatement"), "setString")
            .is_some());
        assert!(registry
            .is_sanitizer(Some("userInteger"), "parseInt")
            .is_some());

        // Case-insensitive matching
        assert!(registry.is_sanitizer(Some("integer"), "parseInt").is_some());
        assert!(registry
            .is_sanitizer(Some("preparedstatement"), "setString")
            .is_some());
    }
}
