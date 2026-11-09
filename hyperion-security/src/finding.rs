// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Security finding data structures

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Severity level of a finding
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Low => write!(f, "LOW"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::High => write!(f, "HIGH"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Type of vulnerability
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VulnType {
    SqlInjection,
    CommandInjection,
    PathTraversal,
    Xss,
    Ssrf,
    XxeInjection,
    LdapInjection,
    LogInjection,
    UnsafeDeserialization,
    HardcodedSecret,
    WeakCrypto,
    InsecureRandom,
    // Phase 2: New taint sinks
    TemplateInjection,
    NoSqlInjection,
    ExpressionLanguageInjection,
    EmailHeaderInjection,
    OpenRedirect,
    // Phase 3: Pattern-based checks
    WeakEncryptionMode,
    HardcodedCryptoIv,
    WeakHashAlgorithm,
    SmallKeySize,
    InsecureTls,
    EmptyPasswordCheck,
    HardcodedCredentials,
    DangerousMethod,
    UnsafeReflection,
    // Phase 4: Framework-specific checks
    CsrfDisabled,
    InsecurePermitAll,
    WeakPasswordEncoding,
    InsecureDeserializationConfig,
    InsecureCookie,
    MissingSecurityHeader,
    HqlInjection,
    // Phase 7: Additional sinks
    ReDoS,
    // Phase 8: Auth and token checks
    JwtMisuse,
    MissingAccessControl,
    // Phase 9: Additional pattern checks
    SensitiveDataLogging,
    MassAssignment,
    StackTraceExposure,
    HardcodedInternalUrl,
    // Phase 2 Tier 2
    Idor,
    RaceCondition,
    InsufficientLogging,
    // Phase 3 Tier 3
    MissingRateLimiting,
    NullPointerDos,
    // Phase 4 Final
    VulnerableDependency,
}

impl VulnType {
    #[must_use]
    pub const fn default_severity(self) -> Severity {
        match self {
            Self::SqlInjection
            | Self::CommandInjection
            | Self::UnsafeDeserialization
            | Self::TemplateInjection
            | Self::InsecureTls
            | Self::EmptyPasswordCheck
            | Self::UnsafeReflection
            | Self::InsecureDeserializationConfig
            | Self::HqlInjection
            | Self::VulnerableDependency => Severity::Critical,

            Self::PathTraversal
            | Self::Xss
            | Self::Ssrf
            | Self::XxeInjection
            | Self::LdapInjection
            | Self::HardcodedSecret
            | Self::NoSqlInjection
            | Self::ExpressionLanguageInjection
            | Self::EmailHeaderInjection
            | Self::WeakEncryptionMode
            | Self::HardcodedCryptoIv
            | Self::HardcodedCredentials
            | Self::CsrfDisabled
            | Self::InsecurePermitAll
            | Self::WeakPasswordEncoding
            | Self::ReDoS
            | Self::JwtMisuse
            | Self::MissingAccessControl
            | Self::SensitiveDataLogging
            | Self::MassAssignment
            | Self::Idor => Severity::High,

            Self::LogInjection
            | Self::WeakCrypto
            | Self::OpenRedirect
            | Self::WeakHashAlgorithm
            | Self::SmallKeySize
            | Self::DangerousMethod
            | Self::InsecureCookie
            | Self::StackTraceExposure
            | Self::HardcodedInternalUrl
            | Self::RaceCondition
            | Self::NullPointerDos => Severity::Medium,

            Self::InsecureRandom
            | Self::MissingSecurityHeader
            | Self::InsufficientLogging
            | Self::MissingRateLimiting => Severity::Low,
        }
    }

    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::SqlInjection => "SQL Injection - User input used in SQL query",
            Self::CommandInjection => "Command Injection - User input used in system command",
            Self::PathTraversal => "Path Traversal - User input used in file path",
            Self::Xss => "Cross-Site Scripting - User input output without encoding",
            Self::Ssrf => "Server-Side Request Forgery - User input used in URL",
            Self::XxeInjection => "XML External Entity Injection - Unsafe XML parsing",
            Self::LdapInjection => "LDAP Injection - User input used in LDAP query",
            Self::LogInjection => "Log Injection - User input written to logs",
            Self::UnsafeDeserialization => "Unsafe Deserialization - Deserializing untrusted data",
            Self::HardcodedSecret => "Hardcoded Secret - Sensitive data in source code",
            Self::WeakCrypto => "Weak Cryptography - Using weak or broken algorithms",
            Self::InsecureRandom => "Insecure Random - Using predictable random values",
            Self::TemplateInjection => "Template Injection - User input in template expression",
            Self::NoSqlInjection => "NoSQL Injection - User input in NoSQL query",
            Self::ExpressionLanguageInjection => "Expression Language Injection - User input in expression evaluation",
            Self::EmailHeaderInjection => "Email Header Injection - User input in email headers",
            Self::OpenRedirect => "Open Redirect - User input used in redirect URL",
            // Phase 3
            Self::WeakEncryptionMode => "Weak Encryption Mode - Using ECB or other insecure cipher modes",
            Self::HardcodedCryptoIv => "Hardcoded IV - Using static or predictable initialization vector",
            Self::WeakHashAlgorithm => "Weak Hash Algorithm - Using MD5, SHA-1, or other broken hash functions",
            Self::SmallKeySize => "Small Key Size - Using insufficient key length for encryption",
            Self::InsecureTls => "Insecure TLS - Using weak protocols or disabling certificate validation",
            Self::EmptyPasswordCheck => "Empty Password Check - Allowing authentication with empty password",
            Self::HardcodedCredentials => "Hardcoded Credentials - Sensitive credentials in source code",
            Self::DangerousMethod => "Dangerous Method - Using deprecated or unsafe API methods",
            Self::UnsafeReflection => "Unsafe Reflection - Using reflection APIs with untrusted input",
            // Phase 4
            Self::CsrfDisabled => "CSRF Protection Disabled - Cross-Site Request Forgery protection is turned off",
            Self::InsecurePermitAll => "Insecure PermitAll - Unauthenticated access to sensitive endpoints",
            Self::WeakPasswordEncoding => "Weak Password Encoding - Using insecure password hashing algorithm",
            Self::InsecureDeserializationConfig => "Insecure Deserialization Config - Unsafe Jackson ObjectMapper configuration",
            Self::InsecureCookie => "Insecure Cookie - Cookie missing security flags",
            Self::MissingSecurityHeader => "Missing Security Header - HTTP security header not set",
            Self::HqlInjection => "HQL/JPQL Injection - Query injection through string concatenation",
            // Phase 7
            Self::ReDoS => "Regular Expression DoS - User input used in regex pattern can cause denial of service",
            // Phase 8
            Self::JwtMisuse => "JWT Misuse - Token verification influenced by untrusted headers",
            Self::MissingAccessControl => "Missing Access Control - Object access without authorization check",
            // Phase 9
            Self::SensitiveDataLogging => "Sensitive Data in Logs - Passwords, tokens, or secrets logged to application logs",
            Self::MassAssignment => "Mass Assignment - Binding untrusted data to model objects without field filtering",
            Self::StackTraceExposure => "Stack Trace Exposure - Exposing exception details to end users",
            Self::HardcodedInternalUrl => "Hardcoded Internal URL - Private IP addresses or internal domain names in source code",
            // Phase 2 Tier 2
            Self::Idor => "Insecure Direct Object Reference - User-supplied ID used without authorization check",
            Self::RaceCondition => "Race Condition / TOCTOU - Time-of-check to time-of-use vulnerability",
            Self::InsufficientLogging => "Insufficient Logging - Security-critical operation without audit trail",
            // Phase 3 Tier 3
            Self::MissingRateLimiting => "Missing Rate Limiting - API endpoint without rate limiting protection",
            Self::NullPointerDos => "Null Pointer DoS - Request handler vulnerable to denial of service via null pointer",
            // Phase 4 Final
            Self::VulnerableDependency => "Vulnerable Dependency - Known vulnerable library version with security issues",
        }
    }

    #[must_use]
    pub const fn cwe_id(self) -> &'static str {
        match self {
            Self::SqlInjection | Self::HqlInjection => "CWE-89",
            Self::CommandInjection => "CWE-78",
            Self::PathTraversal => "CWE-22",
            Self::Xss => "CWE-79",
            Self::Ssrf => "CWE-918",
            Self::XxeInjection => "CWE-611",
            Self::LdapInjection => "CWE-90",
            Self::LogInjection => "CWE-117",
            Self::UnsafeDeserialization | Self::InsecureDeserializationConfig => "CWE-502",
            Self::HardcodedSecret | Self::HardcodedCredentials => "CWE-798",
            Self::WeakCrypto | Self::WeakEncryptionMode | Self::WeakHashAlgorithm => "CWE-327",
            Self::InsecureRandom => "CWE-330",
            Self::TemplateInjection => "CWE-1336",
            Self::NoSqlInjection => "CWE-943",
            Self::ExpressionLanguageInjection => "CWE-917",
            Self::EmailHeaderInjection => "CWE-93",
            Self::OpenRedirect => "CWE-601",
            Self::HardcodedCryptoIv => "CWE-329",
            Self::SmallKeySize | Self::InsecureTls => "CWE-326",
            Self::EmptyPasswordCheck => "CWE-287",
            Self::DangerousMethod => "CWE-477",
            Self::UnsafeReflection => "CWE-470",
            Self::CsrfDisabled => "CWE-352",
            Self::InsecurePermitAll => "CWE-306",
            Self::WeakPasswordEncoding => "CWE-916",
            Self::InsecureCookie => "CWE-614",
            Self::MissingSecurityHeader => "CWE-16",
            Self::ReDoS => "CWE-1333",
            Self::JwtMisuse => "CWE-347",
            Self::MissingAccessControl | Self::Idor => "CWE-639",
            Self::SensitiveDataLogging => "CWE-532",
            Self::MassAssignment => "CWE-915",
            Self::StackTraceExposure => "CWE-209",
            Self::HardcodedInternalUrl => "CWE-1004",
            Self::RaceCondition => "CWE-367",
            Self::InsufficientLogging => "CWE-778",
            Self::MissingRateLimiting => "CWE-799",
            Self::NullPointerDos => "CWE-476",
            Self::VulnerableDependency => "CWE-937",
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SqlInjection => "sql_injection",
            Self::CommandInjection => "command_injection",
            Self::PathTraversal => "path_traversal",
            Self::Xss => "xss",
            Self::Ssrf => "ssrf",
            Self::XxeInjection => "xxe_injection",
            Self::LdapInjection => "ldap_injection",
            Self::LogInjection => "log_injection",
            Self::UnsafeDeserialization => "unsafe_deserialization",
            Self::HardcodedSecret => "hardcoded_secret",
            Self::WeakCrypto => "weak_crypto",
            Self::InsecureRandom => "insecure_random",
            Self::TemplateInjection => "template_injection",
            Self::NoSqlInjection => "no_sql_injection",
            Self::ExpressionLanguageInjection => "expression_language_injection",
            Self::EmailHeaderInjection => "email_header_injection",
            Self::OpenRedirect => "open_redirect",
            Self::WeakEncryptionMode => "weak_encryption_mode",
            Self::HardcodedCryptoIv => "hardcoded_crypto_iv",
            Self::WeakHashAlgorithm => "weak_hash_algorithm",
            Self::SmallKeySize => "small_key_size",
            Self::InsecureTls => "insecure_tls",
            Self::EmptyPasswordCheck => "empty_password_check",
            Self::HardcodedCredentials => "hardcoded_credentials",
            Self::DangerousMethod => "dangerous_method",
            Self::UnsafeReflection => "unsafe_reflection",
            Self::CsrfDisabled => "csrf_disabled",
            Self::InsecurePermitAll => "insecure_permit_all",
            Self::WeakPasswordEncoding => "weak_password_encoding",
            Self::InsecureDeserializationConfig => "insecure_deserialization_config",
            Self::InsecureCookie => "insecure_cookie",
            Self::MissingSecurityHeader => "missing_security_header",
            Self::HqlInjection => "hql_injection",
            Self::ReDoS => "redos",
            Self::JwtMisuse => "jwt_misuse",
            Self::MissingAccessControl => "missing_access_control",
            Self::SensitiveDataLogging => "sensitive_data_logging",
            Self::MassAssignment => "mass_assignment",
            Self::StackTraceExposure => "stack_trace_exposure",
            Self::HardcodedInternalUrl => "hardcoded_internal_url",
            Self::Idor => "idor",
            Self::RaceCondition => "race_condition",
            Self::InsufficientLogging => "insufficient_logging",
            Self::MissingRateLimiting => "missing_rate_limiting",
            Self::NullPointerDos => "null_pointer_dos",
            Self::VulnerableDependency => "vulnerable_dependency",
        }
    }
}

impl std::fmt::Display for VulnType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

/// A location in source code
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file.display(), self.line, self.column)
    }
}

/// A step in the data flow path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStep {
    pub location: Location,
    pub description: String,
    pub code_snippet: String,
}

/// A security finding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub vuln_type: VulnType,
    pub severity: Severity,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub rule_id: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub sink_kind: Option<String>,
    #[serde(default)]
    pub sanitizers: Vec<String>,
    pub message: String,
    pub source_location: Location,
    pub sink_location: Option<Location>,
    pub code_snippet: String,
    pub dataflow: Vec<FlowStep>,
    pub remediation: String,
}

impl Finding {
    /// Create a new finding with minimal information
    #[must_use]
    pub fn new(vuln_type: VulnType, message: String, location: Location, code: String) -> Self {
        let severity = vuln_type.default_severity();
        let id = format!("{}-{:04x}", vuln_type.cwe_id(), {
            // Use stable hash based on file path and line number
            // This ensures finding IDs are consistent across Rust versions
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(location.file.to_string_lossy().as_bytes());
            hasher.update(location.line.to_string().as_bytes());
            let result = hasher.finalize();
            // Take first 2 bytes for a 4-digit hex value
            u16::from_be_bytes([result[0], result[1]])
        });

        Self {
            id,
            vuln_type,
            severity,
            confidence: default_confidence(),
            rule_id: None,
            source_kind: None,
            sink_kind: None,
            sanitizers: Vec::new(),
            message,
            source_location: location,
            sink_location: None,
            code_snippet: code,
            dataflow: Vec::new(),
            remediation: Self::default_remediation(vuln_type),
        }
    }

    fn default_remediation(vuln_type: VulnType) -> String {
        match vuln_type {
            VulnType::SqlInjection => {
                "Use PreparedStatement with parameterized queries".to_string()
            }
            VulnType::CommandInjection => {
                "Avoid Runtime.exec() with user input, use allowlists".to_string()
            }
            VulnType::PathTraversal => {
                "Validate and canonicalize paths, use allowlists".to_string()
            }
            VulnType::Xss => "HTML-encode output using OWASP Encoder".to_string(),
            VulnType::Ssrf => "Validate URLs against allowlist of hosts".to_string(),
            VulnType::XxeInjection => "Disable external entities in XML parser".to_string(),
            VulnType::LdapInjection => "Use parameterized LDAP queries".to_string(),
            VulnType::LogInjection => "Sanitize log messages, remove newlines".to_string(),
            VulnType::UnsafeDeserialization => {
                "Avoid deserializing untrusted data, use allowlists".to_string()
            }
            VulnType::HardcodedSecret => {
                "Use environment variables or secret management".to_string()
            }
            VulnType::WeakCrypto => "Use strong algorithms (AES-256, SHA-256+)".to_string(),
            VulnType::InsecureRandom => "Use SecureRandom for security-sensitive values".to_string(),
            VulnType::TemplateInjection => {
                "Do not allow user input in template expressions. Use safe template escaping.".to_string()
            }
            VulnType::NoSqlInjection => {
                "Use parameterized queries or properly escape user input in NoSQL queries".to_string()
            }
            VulnType::ExpressionLanguageInjection => {
                "Avoid evaluating user-controlled expressions. Use allowlists if necessary.".to_string()
            }
            VulnType::EmailHeaderInjection => {
                "Validate and sanitize email header values. Remove newline characters.".to_string()
            }
            VulnType::OpenRedirect => {
                "Validate redirect URLs against an allowlist or use relative URLs only.".to_string()
            }
            VulnType::WeakEncryptionMode => {
                "Use AES/GCM/NoPadding or AES/CBC/PKCS5Padding with unique IV".to_string()
            }
            VulnType::HardcodedCryptoIv => {
                "Generate random IV using SecureRandom for each encryption".to_string()
            }
            VulnType::WeakHashAlgorithm => {
                "Use SHA-256, SHA-512, or SHA-3 for hashing".to_string()
            }
            VulnType::SmallKeySize => {
                "Use at least 256-bit keys for symmetric encryption, 2048+ bits for RSA".to_string()
            }
            VulnType::InsecureTls => {
                "Use TLSv1.2 or TLSv1.3, enable proper certificate validation".to_string()
            }
            VulnType::EmptyPasswordCheck => {
                "Require non-empty passwords and proper authentication".to_string()
            }
            VulnType::HardcodedCredentials => {
                "Use environment variables or secrets manager".to_string()
            }
            VulnType::DangerousMethod => {
                "Use safe alternatives (e.g., interrupt() instead of stop())".to_string()
            }
            VulnType::UnsafeReflection => {
                "Validate class/method names against allowlist".to_string()
            }
            // Phase 4
            VulnType::CsrfDisabled => {
                "Enable CSRF protection or use token-based authentication for stateless APIs".to_string()
            }
            VulnType::InsecurePermitAll => {
                "Require authentication using authenticated() or hasRole()".to_string()
            }
            VulnType::WeakPasswordEncoding => {
                "Use BCryptPasswordEncoder or Argon2PasswordEncoder".to_string()
            }
            VulnType::InsecureDeserializationConfig => {
                "Remove enableDefaultTyping() or use PolymorphicTypeValidator with allowlist".to_string()
            }
            VulnType::InsecureCookie => {
                "Set Secure and HttpOnly flags on cookies".to_string()
            }
            VulnType::MissingSecurityHeader => {
                "Add security headers using Spring Security configuration".to_string()
            }
            VulnType::HqlInjection => {
                "Use parameterized queries with setParameter()".to_string()
            }
            // Phase 7
            VulnType::ReDoS => {
                "Do not use user input as regex patterns. Pre-compile patterns or use safe alternatives.".to_string()
            }
            // Phase 8
            VulnType::JwtMisuse => {
                "Pin JWT algorithms, validate headers, and avoid fetching JWKs from untrusted URLs".to_string()
            }
            VulnType::MissingAccessControl => {
                "Enforce authorization checks (ownership/role) before returning sensitive objects".to_string()
            }
            VulnType::SensitiveDataLogging => {
                "Remove sensitive data from logs or use masking/redaction. Never log passwords, tokens, or API keys.".to_string()
            }
            VulnType::MassAssignment => {
                "Use @InitBinder to allowlist fields, or use separate DTOs with only safe fields exposed.".to_string()
            }
            VulnType::StackTraceExposure => {
                "Log full stack traces server-side only. Return generic error messages to users.".to_string()
            }
            VulnType::HardcodedInternalUrl => {
                "Use configuration files or environment variables for internal URLs and IP addresses.".to_string()
            }
            VulnType::Idor => {
                "Add authorization checks (ownership/role verification) before returning objects. Use @PreAuthorize or service-layer access control.".to_string()
            }
            VulnType::RaceCondition => {
                "Use atomic operations, proper synchronization, or file locking to prevent race conditions.".to_string()
            }
            VulnType::InsufficientLogging => {
                "Add audit logging for authentication, authorization decisions, and sensitive data operations.".to_string()
            }
            VulnType::MissingRateLimiting => {
                "Add rate limiting using @RateLimiter, Bucket4j, or Resilience4j. Configure appropriate limits based on endpoint sensitivity.".to_string()
            }
            VulnType::NullPointerDos => {
                "Add null checks or use Optional properly. Mark parameters with @NotNull/@Valid or check isPresent() before calling get().".to_string()
            }
            VulnType::VulnerableDependency => {
                "Upgrade to a patched version of the dependency. Check the CVE details for recommended versions.".to_string()
            }
        }
    }

    #[must_use]
    pub fn with_rule_id(mut self, rule_id: impl Into<String>) -> Self {
        self.rule_id = Some(rule_id.into());
        self
    }

    pub fn set_source_kind(&mut self, source_kind: Option<String>) {
        self.source_kind = source_kind;
    }

    pub fn set_sink_kind(&mut self, sink_kind: Option<String>) {
        self.sink_kind = sink_kind;
    }

    pub fn add_sanitizer(&mut self, sanitizer: impl Into<String>) {
        self.sanitizers.push(sanitizer.into());
    }
}

const fn default_confidence() -> f32 {
    0.5
}

/// A pattern signature for grouping similar findings
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FindingPattern {
    pub vuln_type: VulnType,
    pub severity: Severity,
    pub pattern_signature: String,
}

impl Finding {
    /// Extract a pattern signature from this finding for grouping
    #[must_use]
    pub fn extract_pattern(&self) -> FindingPattern {
        let signature = match self.vuln_type {
            VulnType::StackTraceExposure => {
                // Normalize: replace variable names with placeholders
                self.code_snippet
                    .replace("e.getMessage()", "EXCEPTION.getMessage()")
                    .replace("ex.getMessage()", "EXCEPTION.getMessage()")
                    .replace("exception.getMessage()", "EXCEPTION.getMessage()")
                    .replace("err.getMessage()", "EXCEPTION.getMessage()")
                    .replace("error.getMessage()", "EXCEPTION.getMessage()")
                    .replace("e.getStackTrace()", "EXCEPTION.getStackTrace()")
                    .replace("ex.getStackTrace()", "EXCEPTION.getStackTrace()")
                    .replace("exception.getStackTrace()", "EXCEPTION.getStackTrace()")
                    .replace("e.toString()", "EXCEPTION.toString()")
                    .replace("ex.toString()", "EXCEPTION.toString()")
                    .lines()
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            VulnType::MissingRateLimiting => {
                // Group by HTTP method + path pattern
                // Extract method and path from message
                let first_line = self.message.lines().next().unwrap_or("");
                format!("{:?}:{}", self.vuln_type, first_line)
            }
            VulnType::RaceCondition => {
                // Group by the collection type and operation pattern
                // Extract the first line of code snippet
                self.code_snippet
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            }
            VulnType::InsecureRandom => {
                // Group all java.util.Random usages together
                format!("{:?}:java.util.Random", self.vuln_type)
            }
            VulnType::SensitiveDataLogging => {
                // Group by what type of sensitive data is logged
                if self.message.contains("password") {
                    format!("{:?}:password", self.vuln_type)
                } else if self.message.contains("token") || self.message.contains("apiKey") {
                    format!("{:?}:token", self.vuln_type)
                } else if self.message.contains("secret") {
                    format!("{:?}:secret", self.vuln_type)
                } else {
                    format!("{:?}:generic", self.vuln_type)
                }
            }
            _ => {
                // Default: group by vuln_type and severity only
                format!("{:?}:{:?}", self.vuln_type, self.severity)
            }
        };

        FindingPattern {
            vuln_type: self.vuln_type,
            severity: self.severity,
            pattern_signature: signature,
        }
    }
}

/// A group of similar findings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingGroup {
    pub vuln_type: VulnType,
    pub severity: Severity,
    pub pattern: String,
    pub count: usize,
    pub locations: Vec<Location>,
    pub example_finding: Finding,
}

impl FindingGroup {
    /// Create a finding group from a list of findings with the same pattern.
    ///
    /// `findings` must not be empty; the grouping pass only creates groups with at least
    /// one finding.
    #[must_use]
    pub fn from_findings(pattern: FindingPattern, mut findings: Vec<Finding>) -> Self {
        // Sort findings by location for consistent output
        findings.sort_by(|a, b| {
            a.source_location
                .file
                .cmp(&b.source_location.file)
                .then(a.source_location.line.cmp(&b.source_location.line))
        });

        let count = findings.len();
        let locations: Vec<Location> = findings.iter().map(|f| f.source_location.clone()).collect();

        let example_finding = findings
            .into_iter()
            .next()
            .expect("findings vec is non-empty after grouping");

        Self {
            vuln_type: pattern.vuln_type,
            severity: pattern.severity,
            pattern: pattern.pattern_signature,
            count,
            locations,
            example_finding,
        }
    }
}

/// Group similar findings together for better readability
#[must_use]
pub fn group_findings(findings: &[Finding]) -> Vec<FindingGroup> {
    if findings.is_empty() {
        return Vec::new();
    }

    // Group findings by pattern
    let mut groups: HashMap<FindingPattern, Vec<Finding>> = HashMap::new();

    for finding in findings {
        let pattern = finding.extract_pattern();
        groups.entry(pattern).or_default().push(finding.clone());
    }

    // Convert to FindingGroup and sort by severity (descending) then count (descending)
    let mut grouped_findings: Vec<FindingGroup> = groups
        .into_iter()
        .map(|(pattern, findings)| FindingGroup::from_findings(pattern, findings))
        .collect();

    grouped_findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(b.count.cmp(&a.count))
            .then(a.vuln_type.as_str().cmp(b.vuln_type.as_str()))
    });

    grouped_findings
}

/// Return findings that are not present in the baseline (by finding id).
#[must_use]
pub fn diff_findings(current: &[Finding], baseline: &[Finding]) -> Vec<Finding> {
    if baseline.is_empty() {
        return current.to_vec();
    }

    let baseline_ids: HashSet<&str> = baseline.iter().map(|f| f.id.as_str()).collect();
    current
        .iter()
        .filter(|f| !baseline_ids.contains(f.id.as_str()))
        .cloned()
        .collect()
}
