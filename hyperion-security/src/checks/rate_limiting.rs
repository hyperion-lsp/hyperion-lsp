// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Missing rate limiting detection (CWE-799)
//!
//! Detects API endpoints without rate limiting protection, especially
//! sensitive endpoints like authentication and payment processing.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_parser::syntax::{AstNode, ClassDecl, SyntaxKind, SyntaxNode};

pub struct MissingRateLimitingCheck;

/// Classification of endpoint types by risk level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointType {
    /// Health checks and monitoring endpoints (should not be rate limited)
    HealthCheck,
    /// Internal/admin endpoints (often have other protections)
    Internal,
    /// Authentication endpoints (login, signin)
    Authentication,
    /// Password management (reset, change password)
    PasswordManagement,
    /// User registration endpoints
    Registration,
    /// Payment processing endpoints
    Payment,
    /// Read-like operations (search, query, filter) despite using POST
    ReadLike,
    /// Generic state-changing operations
    StateChanging,
}

/// Priority levels for rate limiting enforcement
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EndpointPriority {
    Skip,     // Don't report (health checks)
    Info,     // Informational only
    Low,      // Nice to have
    Medium,   // Standard endpoints
    High,     // Important (registration)
    Critical, // Must have (auth, payment)
}

impl MissingRateLimitingCheck {
    /// HTTP verbs that typically modify state
    const WRITE_VERBS: &'static [&'static str] = &[
        "@PostMapping",
        "@PutMapping",
        "@DeleteMapping",
        "@PatchMapping",
        "@POST",
        "@PUT",
        "@DELETE",
        "@PATCH",
    ];

    /// Endpoint patterns that are read-like despite using POST/PUT/DELETE
    /// These use POST for complex query parameters, not state modification
    const READ_LIKE_PATTERNS: &'static [&'static str] = &[
        "/search",
        "/query",
        "/filter",
        "/find",
        "/list",
        "/get",
        "/fetch",
        "/export", // Read-only data export
        "/download",
        "/health",   // Health check endpoints
        "/metrics",  // Metrics endpoints
        "/status",   // Status check endpoints
        "/validate", // Validation endpoints
        "search(",
        "query(",
        "filter(",
        "find(",
        "list(",
        "export(",
        "validate(",
        "health(",
        "metrics(",
    ];

    /// Global rate limiting indicators that suggest application-wide protection
    const GLOBAL_RATE_LIMITING_PATTERNS: &'static [&'static str] = &[
        "RateLimitFilter",        // Custom filter class
        "RateLimiterRegistry",    // Resilience4j registry
        "RequestRateLimiter",     // Spring Cloud Gateway
        "addFilterBefore",        // Filter chain registration
        "FilterRegistrationBean", // Servlet filter registration
        "@EnableRateLimiting",    // Rate limiting annotation
        "Bucket4j",               // Popular rate limiting library
        ".rateLimiter(",          // Resilience4j configuration
        "RateLimiterConfig",      // Configuration class
    ];

    /// Check if an endpoint looks like a read operation despite using a write verb
    fn is_read_like_endpoint(text: &str) -> bool {
        let lower = text.to_lowercase();
        Self::READ_LIKE_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
    }

    /// Check if the class or project has global rate limiting indicators
    ///
    /// This detects application-wide rate limiting configured via:
    /// - Security filter chains (RateLimitFilter)
    /// - Resilience4j configuration
    /// - Spring Cloud Gateway rate limiting
    /// - Bucket4j integration
    fn has_global_rate_limiting_indicators(text: &str) -> bool {
        Self::GLOBAL_RATE_LIMITING_PATTERNS
            .iter()
            .any(|pattern| text.contains(pattern))
    }

    /// Determine endpoint type from annotations and method name
    fn classify_endpoint(text: &str) -> EndpointType {
        let text_lower = text.to_lowercase();

        // Check for health checks and monitoring endpoints (highest priority - skip these)
        let is_health = text_lower.contains("/health")
            || text_lower.contains("/actuator")
            || text_lower.contains("/metrics")
            || text_lower.contains("/prometheus")
            || text_lower.contains("/swagger")
            || text_lower.contains("/api-docs")
            || text_lower.contains("/ping")
            || text_lower.contains("/ready")
            || text_lower.contains("/live");

        if is_health {
            return EndpointType::HealthCheck;
        }

        // Check for internal/admin endpoints
        let is_internal = text_lower.contains("/internal/")
            || text_lower.contains("/admin/")
            || text_lower.contains("internalcontroller")
            || text_lower.contains("admincontroller");

        if is_internal {
            return EndpointType::Internal;
        }

        // Check for authentication/authorization endpoints (critical)
        let is_auth = text_lower.contains("/login")
            || text_lower.contains("/signin")
            || text_lower.contains("/auth/")
            || text_lower.contains("/authenticate")
            || text_lower.contains("authenticate(")
            || text_lower.contains("login(");

        if is_auth {
            return EndpointType::Authentication;
        }

        // Check for password-related endpoints (critical)
        let is_password = text_lower.contains("/reset-password")
            || text_lower.contains("/forgot-password")
            || text_lower.contains("/change-password")
            || text_lower.contains("/password")
            || text_lower.contains("resetpassword(")
            || text_lower.contains("changepassword(");

        if is_password {
            return EndpointType::PasswordManagement;
        }

        // Check for registration endpoints (high)
        let is_registration = text_lower.contains("/register")
            || text_lower.contains("/signup")
            || text_lower.contains("/create-account")
            || text_lower.contains("register(")
            || text_lower.contains("createaccount(");

        if is_registration {
            return EndpointType::Registration;
        }

        // Check for payment endpoints (critical)
        let is_payment = text_lower.contains("/payment")
            || text_lower.contains("/charge")
            || text_lower.contains("/purchase")
            || text_lower.contains("/transaction")
            || text_lower.contains("/checkout")
            || text_lower.contains("/billing")
            || text_lower.contains("processpayment(")
            || text_lower.contains("createcharge(")
            || text_lower.contains("createcheckout(");

        if is_payment {
            return EndpointType::Payment;
        }

        // Check if it's a read-like operation
        if Self::is_read_like_endpoint(text) {
            return EndpointType::ReadLike;
        }

        // Default to state-changing
        EndpointType::StateChanging
    }

    /// Map endpoint type to priority and severity
    fn calculate_priority(
        endpoint_type: EndpointType,
        http_method: &str,
    ) -> (EndpointPriority, Severity, f32) {
        match endpoint_type {
            // Health checks and read-like operations (search, query, filter, export, validate)
            // should not be flagged. Read-like endpoints use POST for complex parameters
            // but are read operations, not state changes.
            EndpointType::HealthCheck | EndpointType::ReadLike => {
                (EndpointPriority::Skip, Severity::Info, 0.0)
            }

            EndpointType::Internal => (EndpointPriority::Info, Severity::Low, 30.0),

            EndpointType::Authentication
            | EndpointType::PasswordManagement
            | EndpointType::Payment => (EndpointPriority::Critical, Severity::High, 95.0),

            EndpointType::Registration => (EndpointPriority::High, Severity::High, 85.0),

            EndpointType::StateChanging => {
                // Differentiate by HTTP method
                match http_method.to_uppercase().as_str() {
                    "POST" | "PUT" | "DELETE" | "PATCH" => {
                        (EndpointPriority::Medium, Severity::Medium, 60.0)
                    }
                    "GET" => (EndpointPriority::Low, Severity::Low, 40.0),
                    _ => (EndpointPriority::Low, Severity::Low, 30.0),
                }
            }
        }
    }

    /// Extract HTTP method from annotation text
    fn extract_http_method(text: &str) -> String {
        if text.contains("@PostMapping") || text.contains("@POST") {
            "POST".to_string()
        } else if text.contains("@PutMapping") || text.contains("@PUT") {
            "PUT".to_string()
        } else if text.contains("@DeleteMapping") || text.contains("@DELETE") {
            "DELETE".to_string()
        } else if text.contains("@PatchMapping") || text.contains("@PATCH") {
            "PATCH".to_string()
        } else if text.contains("@GetMapping") || text.contains("@GET") {
            "GET".to_string()
        } else {
            "UNKNOWN".to_string()
        }
    }
}

impl Check for MissingRateLimitingCheck {
    fn name(&self) -> &'static str {
        "missing-rate-limiting"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[SyntaxKind::CLASS_DECL]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let text = node.text().to_string();

        // Check if this class has HTTP endpoint methods with write verbs
        let has_write_verb = Self::WRITE_VERBS.iter().any(|verb| text.contains(verb));

        if !has_write_verb {
            return Vec::new();
        }

        // Check for rate limiting annotations or implementations within this specific class
        let has_rate_limiting = text.contains("@RateLimiter")
            || text.contains("@RateLimit")
            || text.contains("@Throttle")
            || text.contains("Bucket4j")
            || text.contains("RateLimiter")
            || text.contains("Resilience4j")
            || text.contains("bucket.tryConsume")
            || text.contains("rateLimiter.acquire")
            || text.contains("checkRateLimit(")
            || text.contains("verifyRateLimit(")
            || text.contains("@RateLimited");

        if has_rate_limiting {
            return Vec::new();
        }

        // Check for global rate limiting indicators in the entire file
        // (e.g., SecurityConfig with RateLimitFilter in the same file)
        let has_global_rate_limiting = Self::has_global_rate_limiting_indicators(context.source);

        // Classify endpoint to determine appropriate severity
        let endpoint_type = Self::classify_endpoint(&text);

        // Extract HTTP method for priority calculation
        let http_method = Self::extract_http_method(&text);

        // Calculate priority and severity based on endpoint type and method
        let (priority, mut severity, mut confidence) =
            Self::calculate_priority(endpoint_type, &http_method);

        // Skip reporting for low-priority endpoints
        if priority == EndpointPriority::Skip {
            return Vec::new();
        }

        // If global rate limiting is detected, handle based on priority
        if has_global_rate_limiting {
            match priority {
                EndpointPriority::Skip => return Vec::new(),
                EndpointPriority::Info | EndpointPriority::Low | EndpointPriority::Medium => {
                    // Suppress - likely covered by global filter
                    return Vec::new();
                }
                EndpointPriority::High | EndpointPriority::Critical => {
                    // Keep critical findings but reduce severity and confidence
                    severity = Severity::Medium;
                    confidence = 30.0;
                }
            }
        }

        // Analyze the whole class, but anchor the warning to its declaration name.
        let location = ClassDecl::cast(node.clone())
            .and_then(|class| class.name())
            .map_or_else(
                || context.node_location(node),
                |name| context.range_location(name.text_range()),
            );
        let code = context.get_code_snippet(node);

        let (mut message, remediation) = match priority {
            EndpointPriority::Critical => match endpoint_type {
                EndpointType::Authentication => (
                    "CRITICAL: Authentication endpoint lacks rate limiting. \
                     This endpoint is highly vulnerable to brute force attacks, credential stuffing, and account enumeration."
                        .to_string(),
                    "IMMEDIATE ACTION REQUIRED: Add aggressive rate limiting (e.g., 5 requests per minute per IP). \
                     Example: @RateLimit(requests = 5, duration = 60, unit = SECONDS). \
                     Consider exponential backoff after failed attempts.".to_string(),
                ),
                EndpointType::PasswordManagement => (
                    "CRITICAL: Password management endpoint lacks rate limiting. \
                     This endpoint is highly vulnerable to brute force attacks and denial of service."
                        .to_string(),
                    "IMMEDIATE ACTION REQUIRED: Add strict rate limiting (e.g., 3 attempts per hour per user). \
                     Example: @RateLimit(requests = 3, duration = 3600, unit = SECONDS). \
                     Consider requiring email verification for password reset.".to_string(),
                ),
                EndpointType::Payment => (
                    "CRITICAL: Payment endpoint lacks rate limiting. \
                     This endpoint is vulnerable to abuse and denial of service."
                        .to_string(),
                    "IMMEDIATE ACTION REQUIRED: Add rate limiting (e.g., 10 transactions per minute per user). \
                     Example: @RateLimit(requests = 10, duration = 60, unit = SECONDS). \
                     Consider additional fraud detection mechanisms.".to_string(),
                ),
                EndpointType::HealthCheck | EndpointType::Internal | EndpointType::Registration | EndpointType::ReadLike | EndpointType::StateChanging => unreachable!(),
            },

            EndpointPriority::High => (
                "High-value endpoint lacks rate limiting. \
                 Registration endpoints are vulnerable to automated account creation and spam."
                    .to_string(),
                "Add rate limiting (e.g., 10 registrations per hour per IP). \
                 Example: @RateLimit(requests = 10, duration = 3600, unit = SECONDS). \
                 Consider CAPTCHA for additional protection.".to_string(),
            ),

            EndpointPriority::Medium => (
                format!("State-changing endpoint '{http_method}' lacks rate limiting. \
                        Rate limiting recommended to prevent resource exhaustion."),
                "Consider rate limiting based on expected usage patterns. \
                 Example: @RateLimit(requests = 1000, duration = 3600, unit = SECONDS)".to_string(),
            ),

            EndpointPriority::Low => (
                format!("Read endpoint '{http_method}' lacks rate limiting. \
                        Consider rate limiting if this endpoint performs expensive operations."),
                "Evaluate if rate limiting is needed based on operation cost and abuse risk.".to_string(),
            ),

            EndpointPriority::Info => (
                "Internal endpoint lacks rate limiting. \
                 Verify that network-level protections are in place."
                    .to_string(),
                "Evaluate if rate limiting is needed. Internal endpoints often have IP restrictions or VPN requirements.".to_string(),
            ),

            EndpointPriority::Skip => {
                // Should be unreachable due to early return above
                return Vec::new();
            }
        };

        // If global rate limiting is detected, add note
        if has_global_rate_limiting {
            message.push_str(
                " Note: Global rate limiting configuration detected (e.g., RateLimitFilter). \
                 Verify this critical endpoint is actually protected by the global filter.",
            );
        }

        let mut finding = Finding::new(VulnType::MissingRateLimiting, message, location, code);
        finding.severity = severity;
        finding.remediation = remediation;
        finding.confidence = confidence;

        vec![finding]
    }
}
