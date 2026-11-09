// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::float_cmp, clippy::needless_collect)]

//! Phase 3 Tier 3: Specialized security checks tests
//!
//! Tests for:
//! 1. Missing rate limiting detection (CWE-799)
//! 2. Null pointer DoS detection (CWE-476)

use hyperion_parser::{Lexer, Parser};
use hyperion_security::checks::{CheckContext, CheckRegistry};
use hyperion_security::finding::VulnType;
use std::path::Path;

fn scan_code(code: &str) -> Vec<hyperion_security::finding::Finding> {
    let lexer = Lexer::new(code);
    let tokens = lexer.collect::<Vec<_>>();
    let (green_node, _errors) = Parser::parse(tokens);
    let tree = hyperion_parser::SyntaxNode::new_root(green_node);

    let registry = CheckRegistry::new();
    let context = CheckContext::new(Path::new("Test.java"), code);
    registry.check_tree(&tree, &context)
}

#[test]
fn test_missing_rate_limiting_on_login_endpoint() {
    let code = r#"
        @RestController
        public class AuthController {
            @PostMapping("/login")
            public ResponseEntity login(@RequestBody Credentials creds) {
                return authService.authenticate(creds);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Should detect missing rate limiting on login endpoint"
    );
    assert_eq!(
        rate_limit_findings[0].severity,
        hyperion_security::finding::Severity::High,
        "Login endpoints should have HIGH severity"
    );
}

#[test]
fn test_missing_rate_limiting_on_password_reset() {
    let code = r#"
        @RestController
        public class PasswordController {
            @PostMapping("/reset-password")
            public void resetPassword(@RequestParam String email) {
                passwordService.sendResetEmail(email);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Should detect missing rate limiting on password reset"
    );
    assert_eq!(
        rate_limit_findings[0].severity,
        hyperion_security::finding::Severity::High,
        "Password reset endpoints should have HIGH severity"
    );
}

#[test]
fn test_missing_rate_limiting_on_registration() {
    let code = r#"
        @RestController
        public class UserController {
            @PostMapping("/register")
            public User createUser(@RequestBody UserDTO user) {
                return userService.create(user);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Should detect missing rate limiting on registration"
    );
    assert_eq!(
        rate_limit_findings[0].severity,
        hyperion_security::finding::Severity::High,
        "Registration endpoints should have HIGH severity (Phase 2.2 context-aware severity)"
    );
}

#[test]
fn test_missing_rate_limiting_on_payment() {
    let code = r#"
        @RestController
        public class PaymentController {
            @PostMapping("/payment")
            public PaymentResponse processPayment(@RequestBody PaymentRequest request) {
                return paymentService.charge(request);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Should detect missing rate limiting on payment endpoint"
    );
    assert_eq!(
        rate_limit_findings[0].severity,
        hyperion_security::finding::Severity::High,
        "Payment endpoints should have HIGH severity"
    );
}

#[test]
fn test_missing_rate_limiting_on_generic_post() {
    let code = r#"
        @RestController
        public class ItemController {
            @PostMapping("/items")
            public Item createItem(@RequestBody ItemDTO item) {
                return itemService.create(item);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Should detect missing rate limiting on generic POST endpoint"
    );
    assert_eq!(
        rate_limit_findings[0].severity,
        hyperion_security::finding::Severity::Medium,
        "Generic POST endpoints should have MEDIUM severity (Phase 2.2 context-aware severity)"
    );
}

#[test]
fn test_rate_limiting_present_with_annotation() {
    let code = r#"
        @RestController
        public class AuthController {
            @RateLimiter(name = "login")
            @PostMapping("/login")
            public ResponseEntity login(@RequestBody Credentials creds) {
                return authService.authenticate(creds);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Should not flag when @RateLimiter is present"
    );
}

#[test]
fn test_rate_limiting_present_with_bucket4j() {
    let code = r#"
        @RestController
        public class AuthController {
            @PostMapping("/login")
            public ResponseEntity login(@RequestBody Credentials creds) {
                if (!bucket.tryConsume(1)) {
                    return ResponseEntity.status(429).build();
                }
                return authService.authenticate(creds);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Should not flag when Bucket4j is used"
    );
}

#[test]
fn test_rate_limiting_not_on_get_requests() {
    let code = r#"
        @RestController
        public class UserController {
            @GetMapping("/users/{id}")
            public User getUser(@PathVariable Long id) {
                return userService.findById(id);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Should not flag GET endpoints"
    );
}

// Null Pointer DoS Tests

#[test]
fn test_optional_get_without_ispresent() {
    let code = r#"
        @RestController
        public class UserController {
            @GetMapping("/user")
            public User getUser(@RequestParam String id) {
                Optional<User> user = userRepository.findById(id);
                return user.get();
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        !null_dos_findings.is_empty(),
        "Should detect Optional.get() without isPresent()"
    );
    assert!(null_dos_findings[0].message.contains("Optional.get()"));
}

#[test]
fn test_optional_with_orelse_safe() {
    let code = r#"
        @RestController
        public class UserController {
            @GetMapping("/user")
            public User getUser(@RequestParam String id) {
                Optional<User> user = userRepository.findById(id);
                return user.orElse(new User());
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        null_dos_findings.is_empty(),
        "Should not flag when orElse() is used"
    );
}

#[test]
fn test_optional_with_ispresent_safe() {
    let code = r#"
        @RestController
        public class UserController {
            @GetMapping("/user")
            public User getUser(@RequestParam String id) {
                Optional<User> user = userRepository.findById(id);
                if (user.isPresent()) {
                    return user.get();
                }
                throw new NotFoundException();
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        null_dos_findings.is_empty(),
        "Should not flag when isPresent() is checked"
    );
}

#[test]
fn test_optional_with_orelsethrow_safe() {
    let code = r#"
        @RestController
        public class UserController {
            @GetMapping("/user")
            public User getUser(@RequestParam String id) {
                return userRepository.findById(id)
                    .orElseThrow(() -> new NotFoundException());
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        null_dos_findings.is_empty(),
        "Should not flag when orElseThrow() is used"
    );
}

#[test]
fn test_request_param_optional_without_null_check() {
    let code = r#"
        @RestController
        public class UserController {
            @GetMapping("/search")
            public List<User> search(@RequestParam(required = false) String name) {
                int length = name.length();
                return userService.findByName(name);
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        !null_dos_findings.is_empty(),
        "Should detect optional @RequestParam used without null check"
    );
    assert!(null_dos_findings[0].message.contains("@RequestParam"));
}

#[test]
fn test_request_param_with_notnull_safe() {
    let code = r#"
        @RestController
        public class UserController {
            @GetMapping("/search")
            public List<User> search(@RequestParam @NotNull String name) {
                int length = name.length();
                return userService.findByName(name);
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        null_dos_findings.is_empty(),
        "Should not flag when @NotNull is present"
    );
}

#[test]
fn test_findbyid_get_without_check() {
    let code = r#"
        @RestController
        public class ItemController {
            @GetMapping("/item/{id}")
            public Item getItem(@PathVariable Long id) {
                Item item = itemService.findById(id).get();
                return item.getName();
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    // This should be caught by either the Optional.get() check or the findById check
    assert!(
        !null_dos_findings.is_empty(),
        "Should detect findById().get() without null check"
    );
}

#[test]
fn test_request_body_chained_calls_without_validation() {
    let code = r#"
        @RestController
        public class OrderController {
            @PostMapping("/order")
            public void createOrder(@RequestBody Order order) {
                String street = order.getAddress().getStreet();
                processOrder(street);
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        !null_dos_findings.is_empty(),
        "Should detect chained method calls on @RequestBody without validation"
    );
}

#[test]
fn test_request_body_with_valid_annotation_safe() {
    let code = r#"
        @RestController
        public class OrderController {
            @PostMapping("/order")
            public void createOrder(@Valid @RequestBody Order order) {
                String street = order.getAddress().getStreet();
                processOrder(street);
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        null_dos_findings.is_empty(),
        "Should not flag when @Valid is present"
    );
}

#[test]
fn test_request_body_with_null_check_safe() {
    let code = r#"
        @RestController
        public class OrderController {
            @PostMapping("/order")
            public void createOrder(@RequestBody Order order) {
                if (order.getAddress() != null) {
                    String street = order.getAddress().getStreet();
                    processOrder(street);
                }
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        null_dos_findings.is_empty(),
        "Should not flag when explicit null check is present"
    );
}

#[test]
fn test_not_in_endpoint_no_finding() {
    let code = r#"
        public class UserService {
            public User getUser(String id) {
                return userRepository.findById(id).get();
            }
        }
    "#;

    let findings = scan_code(code);
    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        null_dos_findings.is_empty(),
        "Should not flag null pointer issues outside of endpoints"
    );
}

#[test]
fn test_jax_rs_path_annotation() {
    let code = r#"
        @Path("/users")
        public class UserResource {
            @POST
            @Path("/login")
            public Response login(Credentials creds) {
                return authService.authenticate(creds);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Should detect missing rate limiting on JAX-RS endpoints"
    );
}

#[test]
fn test_multiple_issues_in_same_method() {
    let code = r#"
        @RestController
        public class AuthController {
            @PostMapping("/login")
            public User login(@RequestParam(required = false) String username) {
                Optional<User> user = userRepository.findByUsername(username);
                return user.get();
            }
        }
    "#;

    let findings = scan_code(code);

    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    let null_dos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::NullPointerDos)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Should detect missing rate limiting"
    );
    assert!(
        !null_dos_findings.is_empty(),
        "Should detect null pointer DoS issues"
    );
}

// Phase 1.2: HTTP Verb Awareness Tests - Read-like endpoints should NOT be flagged

#[test]
fn test_search_endpoint_not_flagged() {
    let code = r#"
        @RestController
        public class ProductController {
            @PostMapping("/products/search")
            public List<Product> searchProducts(@RequestBody SearchCriteria criteria) {
                return productService.search(criteria);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Search endpoints should NOT be flagged (read-like operation)"
    );
}

#[test]
fn test_query_endpoint_not_flagged() {
    let code = r#"
        @RestController
        public class ReportController {
            @PostMapping("/reports/query")
            public ReportData queryReport(@RequestBody ReportQuery query) {
                return reportService.query(query);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Query endpoints should NOT be flagged (read-like operation)"
    );
}

#[test]
fn test_filter_endpoint_not_flagged() {
    let code = r#"
        @RestController
        public class DataController {
            @PostMapping("/data/filter")
            public List<DataPoint> filterData(@RequestBody FilterCriteria criteria) {
                return dataService.filter(criteria);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Filter endpoints should NOT be flagged (read-like operation)"
    );
}

#[test]
fn test_export_endpoint_not_flagged() {
    let code = r#"
        @RestController
        public class ReportController {
            @PostMapping("/reports/export")
            public byte[] exportReport(@RequestBody ExportRequest request) {
                return reportService.export(request);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Export endpoints should NOT be flagged (read-like operation)"
    );
}

#[test]
fn test_health_endpoint_not_flagged() {
    let code = r#"
        @RestController
        public class HealthController {
            @PostMapping("/health/check")
            public HealthStatus checkHealth(@RequestBody HealthRequest request) {
                return healthService.check(request);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Health check endpoints should NOT be flagged (read-like operation)"
    );
}

#[test]
fn test_validate_endpoint_not_flagged() {
    let code = r#"
        @RestController
        public class ValidationController {
            @PostMapping("/data/validate")
            public ValidationResult validateData(@RequestBody DataRequest request) {
                return validationService.validate(request);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Validation endpoints should NOT be flagged (read-like operation)"
    );
}

#[test]
fn test_method_name_search_not_flagged() {
    let code = r#"
        @RestController
        public class UserController {
            @PostMapping("/users")
            public List<User> searchUsers(@RequestBody UserCriteria criteria) {
                return userService.search(criteria);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Methods with search() should NOT be flagged (read-like operation)"
    );
}

// Verify that TRUE state-changing operations are still caught

#[test]
fn test_purchase_endpoint_still_flagged() {
    let code = r#"
        @RestController
        public class OrderController {
            @PostMapping("/orders/purchase")
            public Order purchaseOrder(@RequestBody OrderRequest request) {
                return orderService.create(request);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Purchase endpoints SHOULD be flagged (state-changing)"
    );
}

#[test]
fn test_delete_endpoint_still_flagged() {
    let code = r#"
        @RestController
        public class UserController {
            @DeleteMapping("/users/{id}")
            public void deleteUser(@PathVariable Long id) {
                userService.delete(id);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "DELETE endpoints SHOULD be flagged (state-changing)"
    );
}

#[test]
fn test_create_endpoint_still_flagged() {
    let code = r#"
        @RestController
        public class ItemController {
            @PostMapping("/items/create")
            public Item createItem(@RequestBody ItemDTO item) {
                return itemService.create(item);
            }
        }
    "#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Create endpoints SHOULD be flagged (state-changing)"
    );
    assert_eq!(
        rate_limit_findings[0].severity,
        hyperion_security::finding::Severity::Medium,
        "Generic create endpoints should have MEDIUM severity (Phase 2.2 - state-changing POST)"
    );
}

// Phase 11: Global rate limiting detection tests

#[test]
fn test_global_rate_limiting_suppresses_generic_endpoints() {
    // Generic endpoints should be suppressed when global RateLimitFilter is detected
    let code = r#"
package com.example;
import org.springframework.web.filter.OncePerRequestFilter;

public class RateLimitFilter extends OncePerRequestFilter {
    @Override
    protected void doFilterInternal(HttpServletRequest request, HttpServletResponse response, FilterChain chain) {
        // Rate limiting implementation
    }
}

@RestController
public class UserController {
    @PostMapping("/api/users")
    public User createUser(@RequestBody User user) {
        return userService.save(user);
    }

    @PutMapping("/api/users/{id}")
    public User updateUser(@PathVariable Long id, @RequestBody User user) {
        return userService.update(id, user);
    }
}
"#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    // Generic endpoints should be suppressed when RateLimitFilter is present
    assert!(
        rate_limit_findings.is_empty(),
        "Generic endpoints should be suppressed when RateLimitFilter is detected"
    );
}

#[test]
fn test_global_rate_limiting_keeps_auth_endpoints_with_note() {
    // Auth endpoints should still be flagged but with reduced severity and note
    let code = r#"
package com.example;
import org.springframework.web.filter.OncePerRequestFilter;

public class RateLimitFilter extends OncePerRequestFilter {
    @Override
    protected void doFilterInternal(HttpServletRequest request, HttpServletResponse response, FilterChain chain) {
        // Rate limiting implementation
    }
}

@RestController
public class AuthController {
    @PostMapping("/api/auth/login")
    public Token login(@RequestBody LoginRequest request) {
        return authService.authenticate(request);
    }
}
"#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    // Auth endpoints should still be flagged even with global rate limiting
    assert!(
        !rate_limit_findings.is_empty(),
        "Auth endpoints should still be flagged even with global rate limiting"
    );

    let finding = &rate_limit_findings[0];

    // Should be downgraded from High to Medium
    assert_eq!(
        finding.severity,
        hyperion_security::finding::Severity::Medium,
        "Severity should be reduced from High to Medium when global rate limiting exists"
    );

    // Should contain a note about global rate limiting
    assert!(
        finding
            .message
            .contains("Global rate limiting configuration detected"),
        "Message should note that global rate limiting was detected"
    );

    // Should have lower confidence
    assert_eq!(
        finding.confidence, 30.0,
        "Confidence should be lowered to 30% when global rate limiting exists"
    );
}

#[test]
fn test_global_rate_limiting_bucket4j_pattern() {
    // Test Bucket4j detection
    let code = r#"
package com.example;
import io.github.bucket4j.Bucket;

@Configuration
public class RateLimitConfig {
    @Bean
    public Bucket bucket() {
        return Bucket4j.builder()
            .addLimit(Bandwidth.simple(100, Duration.ofMinutes(1)))
            .build();
    }
}

@RestController
public class DataController {
    @PostMapping("/api/data")
    public Data createData(@RequestBody Data data) {
        return dataService.save(data);
    }
}
"#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Generic endpoints should be suppressed when Bucket4j is detected"
    );
}

#[test]
fn test_global_rate_limiting_resilience4j_pattern() {
    // Test Resilience4j RateLimiterRegistry detection
    let code = r#"
package com.example;
import io.github.resilience4j.ratelimiter.RateLimiterRegistry;

@Configuration
public class SecurityConfig {
    @Bean
    public RateLimiterRegistry rateLimiterRegistry() {
        return RateLimiterRegistry.ofDefaults();
    }
}

@RestController
public class ItemController {
    @PostMapping("/api/items")
    public Item createItem(@RequestBody Item item) {
        return itemService.save(item);
    }
}
"#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Generic endpoints should be suppressed when RateLimiterRegistry is detected"
    );
}

#[test]
fn test_global_rate_limiting_filter_registration() {
    // Test addFilterBefore pattern detection
    let code = r#"
package com.example;
import org.springframework.security.config.annotation.web.builders.HttpSecurity;

@Configuration
public class SecurityConfig {
    @Bean
    public SecurityFilterChain filterChain(HttpSecurity http) throws Exception {
        http.addFilterBefore(new RateLimitFilter(), UsernamePasswordAuthenticationFilter.class);
        return http.build();
    }
}

@RestController
public class OrderController {
    @PostMapping("/api/orders")
    public Order createOrder(@RequestBody Order order) {
        return orderService.create(order);
    }
}
"#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        rate_limit_findings.is_empty(),
        "Generic endpoints should be suppressed when addFilterBefore is detected"
    );
}

#[test]
fn test_global_rate_limiting_payment_endpoints_still_flagged() {
    // Payment endpoints should still be flagged with note
    let code = r#"
package com.example;

public class RateLimitFilter extends OncePerRequestFilter {
    // Global rate limiting
}

@RestController
public class PaymentController {
    @PostMapping("/api/payments")
    public PaymentResult processPayment(@RequestBody PaymentRequest request) {
        return paymentService.process(request);
    }
}
"#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Payment endpoints should still be flagged even with global rate limiting"
    );

    let finding = &rate_limit_findings[0];

    // Should be downgraded from High to Medium
    assert_eq!(
        finding.severity,
        hyperion_security::finding::Severity::Medium,
        "Payment endpoint severity should be reduced when global rate limiting exists"
    );

    assert!(
        finding
            .message
            .contains("Global rate limiting configuration detected"),
        "Message should note that global rate limiting was detected"
    );
}

#[test]
fn test_no_global_rate_limiting_auth_endpoints_high_severity() {
    // Without global rate limiting, auth endpoints should be High severity
    let code = r#"
package com.example;

@RestController
public class AuthController {
    @PostMapping("/api/auth/login")
    public Token login(@RequestBody LoginRequest request) {
        return authService.authenticate(request);
    }
}
"#;

    let findings = scan_code(code);
    let rate_limit_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::MissingRateLimiting)
        .collect();

    assert!(
        !rate_limit_findings.is_empty(),
        "Auth endpoints should be flagged when no rate limiting exists"
    );

    let finding = &rate_limit_findings[0];

    // Should be High severity without global rate limiting
    assert_eq!(
        finding.severity,
        hyperion_security::finding::Severity::High,
        "Auth endpoint should have High severity when no rate limiting exists"
    );

    // Should NOT contain note about global rate limiting
    assert!(
        !finding
            .message
            .contains("Global rate limiting configuration detected"),
        "Message should not mention global rate limiting when it doesn't exist"
    );

    // Should have default confidence (not lowered)
    assert_ne!(
        finding.confidence, 30.0,
        "Confidence should not be lowered when no global rate limiting exists"
    );
}
