// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Phase 2.2 False Positive Reduction
//!
//! Stack Trace Exposure - Response Context
//!
//! This test suite validates that the stack trace exposure check:
//! - Catches TRUE POSITIVES: Stack traces exposed in public web endpoints
//! - Suppresses FALSE POSITIVES: Stack traces in CLI tools, admin endpoints

use hyperion_parser::{Lexer, Parser};
use hyperion_security::checks::{CheckContext, CheckRegistry};
use hyperion_security::finding::VulnType;
use std::path::Path;

fn check_code(source: &str) -> Vec<hyperion_security::finding::Finding> {
    let lexer = Lexer::new(source);
    let tokens = lexer.collect::<Vec<_>>();
    let (green_node, _errors) = Parser::parse(tokens);
    let tree = hyperion_parser::SyntaxNode::new_root(green_node);

    let registry = CheckRegistry::new();
    let file = Path::new("Test.java");
    let context = CheckContext::new(file, source);

    registry.check_tree(&tree, &context)
}

fn get_stack_trace_findings(
    findings: &[hyperion_security::finding::Finding],
) -> Vec<&hyperion_security::finding::Finding> {
    findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::StackTraceExposure)
        .collect()
}

// =============================================================================
// FALSE POSITIVES - Should be suppressed
// =============================================================================

#[test]
fn test_fp_print_stack_trace_in_main_method() {
    let source = r#"
        public class CliTool {
            public static void main(String[] args) {
                try {
                    runCommand(args);
                } catch (Exception e) {
                    e.printStackTrace();
                    System.exit(1);
                }
            }

            private static void runCommand(String[] args) {
                // implementation
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag printStackTrace() in main() method (CLI tool). Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_exception_message_in_admin_endpoint() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;
        import org.springframework.security.access.prepost.PreAuthorize;

        @RestController
        public class AdminController {
            @GetMapping("/admin/diagnostics")
            @PreAuthorize("hasRole('ADMIN')")
            public ResponseEntity<String> getDiagnostics() {
                try {
                    return ResponseEntity.ok(system.diagnose());
                } catch (Exception e) {
                    return ResponseEntity.status(500).body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag exception details in admin endpoint with hasRole('ADMIN'). Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_admin_endpoint_double_quotes() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;
        import org.springframework.security.access.prepost.PreAuthorize;

        @RestController
        public class AdminController {
            @GetMapping("/admin/status")
            @PreAuthorize("hasRole(\"ADMIN\")")
            public ResponseEntity<String> getStatus() {
                try {
                    return ResponseEntity.ok(getSystemStatus());
                } catch (Exception e) {
                    return ResponseEntity.status(500).body(e.toString());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag exception in admin endpoint with hasRole(\"ADMIN\"). Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_internal_endpoint_stack_trace() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;

        @RestController
        public class InternalController {
            @GetMapping("/internal/debug")
            public String debugInfo() {
                try {
                    return service.getDebugInfo();
                } catch (Exception e) {
                    return e.toString();
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag stack trace in /internal/ endpoint. Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_diagnostics_endpoint() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class SystemController {
            @GetMapping("/diagnostics")
            public ResponseEntity<String> getDiagnostics() {
                try {
                    return ResponseEntity.ok(runDiagnostics());
                } catch (Exception e) {
                    return ResponseEntity.status(500).body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag exception in /diagnostics endpoint. Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_debug_endpoint() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;

        @RestController
        public class DebugController {
            @GetMapping("/debug")
            public String debug() {
                try {
                    return getDebugData();
                } catch (Exception e) {
                    return e.getMessage();
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag exception in /debug endpoint. Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_admin_path_with_role_admin_authority() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;
        import org.springframework.security.access.prepost.PreAuthorize;

        @RestController
        public class AdminController {
            @GetMapping("/admin/system/health")
            @PreAuthorize("hasAuthority('ROLE_ADMIN')")
            public ResponseEntity<String> getHealth() {
                try {
                    return ResponseEntity.ok(checkHealth());
                } catch (Exception e) {
                    return ResponseEntity.status(500).body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag exception in admin endpoint with hasAuthority('ROLE_ADMIN'). Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_cli_tool_with_nested_exception_handling() {
    let source = r#"
        public class DataProcessor {
            public static void main(String[] args) {
                try {
                    processData(args[0]);
                } catch (FileNotFoundException e) {
                    System.err.println("File not found");
                    e.printStackTrace();
                } catch (Exception e) {
                    System.err.println("Error processing data");
                    e.printStackTrace();
                    System.exit(1);
                }
            }

            private static void processData(String file) throws Exception {
                // implementation
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag printStackTrace() in CLI tool with multiple catch blocks. Found {} findings",
        stack_trace.len()
    );
}

// =============================================================================
// TRUE POSITIVES - Should be flagged
// =============================================================================

#[test]
fn test_tp_public_endpoint_returns_exception_message() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class UserController {
            @GetMapping("/users/{id}")
            public ResponseEntity<String> getUser(@PathVariable Long id) {
                try {
                    return ResponseEntity.ok(userService.getUser(id));
                } catch (Exception e) {
                    return ResponseEntity.status(500).body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert_eq!(
        stack_trace.len(),
        1,
        "Should flag returning exception.getMessage() in public endpoint"
    );
    assert!(
        stack_trace[0].message.contains("getMessage"),
        "Finding should mention getMessage"
    );
}

#[test]
fn test_tp_print_stack_trace_in_web_handler() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;

        @RestController
        public class ApiController {
            @PostMapping("/api/process")
            public void process(@RequestBody String data) {
                try {
                    service.process(data);
                } catch (Exception e) {
                    e.printStackTrace();
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert_eq!(
        stack_trace.len(),
        1,
        "Should flag printStackTrace() in web handler"
    );
    assert!(
        stack_trace[0].message.contains("printStackTrace"),
        "Finding should mention printStackTrace"
    );
}

#[test]
fn test_tp_exception_tostring_in_public_endpoint() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class OrderController {
            @PostMapping("/orders")
            public ResponseEntity<String> createOrder(@RequestBody String order) {
                try {
                    return ResponseEntity.ok(orderService.create(order));
                } catch (Exception e) {
                    return ResponseEntity.status(500).body(e.toString());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert_eq!(
        stack_trace.len(),
        1,
        "Should flag returning exception.toString() in public endpoint"
    );
    assert!(
        stack_trace[0].message.contains("toString"),
        "Finding should mention toString method"
    );
}

#[test]
fn test_tp_exception_getstacktrace_in_controller() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class ErrorController {
            @GetMapping("/data/{id}")
            public ResponseEntity<String> getData(@PathVariable Long id) {
                try {
                    return ResponseEntity.ok(dataService.get(id));
                } catch (Exception e) {
                    return ResponseEntity.status(500).body(e.getStackTrace());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert_eq!(
        stack_trace.len(),
        1,
        "Should flag returning exception.getStackTrace() in public endpoint"
    );
    assert!(
        stack_trace[0].message.contains("getStackTrace"),
        "Finding should mention getStackTrace method"
    );
}

#[test]
fn test_tp_exception_getcause_in_rest_controller() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class ProductController {
            @PutMapping("/products/{id}")
            public ResponseEntity<String> updateProduct(@PathVariable Long id) {
                try {
                    return ResponseEntity.ok(productService.update(id));
                } catch (Exception e) {
                    return ResponseEntity.status(500).body(e.getCause());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert_eq!(
        stack_trace.len(),
        1,
        "Should flag returning exception.getCause() in public endpoint"
    );
    assert!(
        stack_trace[0].message.contains("getCause"),
        "Finding should mention getCause method"
    );
}

#[test]
fn test_tp_public_api_with_catch_and_printstacktrace() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;

        @RestController
        public class PaymentController {
            @PostMapping("/payments/process")
            public String processPayment(@RequestBody String payment) {
                try {
                    return paymentService.process(payment);
                } catch (Exception e) {
                    e.printStackTrace();
                    return "Payment failed";
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert_eq!(
        stack_trace.len(),
        1,
        "Should flag printStackTrace() in public REST endpoint"
    );
}

#[test]
fn test_tp_jax_rs_endpoint_with_exception_message() {
    let source = r#"
        import javax.ws.rs.*;
        import javax.ws.rs.core.Response;

        @Path("/api")
        public class JaxRsController {
            @GET
            @Path("/data")
            public Response getData() {
                try {
                    return Response.ok(service.getData()).build();
                } catch (Exception e) {
                    return Response.status(500).entity(e.getMessage()).build();
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert_eq!(
        stack_trace.len(),
        1,
        "JAX-RS Response.status(...).entity(...) must not expose exception messages"
    );
    assert!(stack_trace[0].message.contains("getMessage"));
}

#[test]
fn test_tp_jax_rs_ok_response_with_exception_message() {
    let source = r#"
        import jakarta.ws.rs.*;
        import jakarta.ws.rs.core.Response;

        @Path("/api")
        public class JaxRsController {
            @GET
            @Path("/data")
            public Response getData() {
                try {
                    return Response.ok(service.getData()).build();
                } catch (Exception e) {
                    return Response.ok(e.getMessage()).build();
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert_eq!(stack_trace.len(), 1);
    assert!(stack_trace[0].message.contains("getMessage"));
}

#[test]
fn test_fp_jax_rs_admin_endpoint_with_exception_message() {
    let source = r#"
        import jakarta.annotation.security.RolesAllowed;
        import jakarta.ws.rs.*;
        import jakarta.ws.rs.core.Response;

        @Path("/admin")
        public class AdminResource {
            @GET
            @Path("/diagnostics")
            @RolesAllowed("ADMIN")
            public Response diagnostics() {
                try {
                    return Response.ok(service.status()).build();
                } catch (Exception e) {
                    return Response.serverError().entity(e.getMessage()).build();
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "restricted JAX-RS diagnostics endpoints should retain admin suppression"
    );
}

// =============================================================================
// Phase 5 Improvements - Exception Type Analysis
// =============================================================================

#[test]
fn test_fp_fully_qualified_exception_name() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class UserController {
            @PostMapping("/register")
            public ResponseEntity<?> register(@RequestBody User user) {
                try {
                    userService.register(user);
                    return ResponseEntity.ok("Success");
                } catch (java.lang.IllegalArgumentException e) {
                    return ResponseEntity.badRequest().body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag getMessage() for fully qualified IllegalArgumentException. Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_generic_validation_exception() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class PaymentController {
            @PostMapping("/process")
            public ResponseEntity<?> process(@RequestBody Payment payment) {
                try {
                    paymentService.process(payment);
                    return ResponseEntity.ok("Success");
                } catch (ValidationException<Payment> e) {
                    return ResponseEntity.badRequest().body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag getMessage() for ValidationException with generic parameter. Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_multi_catch_all_safe() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class OrderController {
            @PostMapping("/validate")
            public ResponseEntity<?> validate(@RequestBody Order order) {
                try {
                    orderService.validate(order);
                    return ResponseEntity.ok("Valid");
                } catch (IllegalArgumentException | IllegalStateException e) {
                    return ResponseEntity.badRequest().body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag getMessage() for multi-catch with all safe exception types. Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_tp_multi_catch_mixed_safe_and_dangerous() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class DataController {
            @GetMapping("/data")
            public ResponseEntity<?> getData() {
                try {
                    return ResponseEntity.ok(dataService.getData());
                } catch (IllegalArgumentException | SQLException e) {
                    return ResponseEntity.status(500).body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert_eq!(
        stack_trace.len(),
        1,
        "Should flag getMessage() for multi-catch with mixed safe/dangerous types"
    );
    assert!(
        stack_trace[0].message.contains("getMessage"),
        "Finding should mention getMessage"
    );
}

#[test]
fn test_fp_custom_validation_exception() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class PaymentController {
            @PostMapping("/charge")
            public ResponseEntity<?> charge(@RequestBody PaymentRequest request) {
                try {
                    paymentService.charge(request);
                    return ResponseEntity.ok("Success");
                } catch (PaymentValidationException e) {
                    return ResponseEntity.badRequest().body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag getMessage() for custom PaymentValidationException. Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_custom_business_exception() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class OrderController {
            @PostMapping("/submit")
            public ResponseEntity<?> submit(@RequestBody Order order) {
                try {
                    orderService.submit(order);
                    return ResponseEntity.ok("Submitted");
                } catch (OrderBusinessException e) {
                    return ResponseEntity.badRequest().body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag getMessage() for custom OrderBusinessException. Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_fp_resource_not_found_exception() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class UserController {
            @GetMapping("/users/{id}")
            public ResponseEntity<?> getUser(@PathVariable Long id) {
                try {
                    return ResponseEntity.ok(userService.getUser(id));
                } catch (ResourceNotFoundException e) {
                    return ResponseEntity.status(404).body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag getMessage() for ResourceNotFoundException. Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_tp_sql_exception_with_fully_qualified_name() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class QueryController {
            @GetMapping("/query")
            public ResponseEntity<?> executeQuery(@RequestParam String query) {
                try {
                    return ResponseEntity.ok(jdbcTemplate.query(query));
                } catch (java.sql.SQLException e) {
                    return ResponseEntity.status(500).body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert_eq!(
        stack_trace.len(),
        1,
        "Should flag getMessage() for fully qualified SQLException"
    );
    assert!(
        stack_trace[0].message.contains("getMessage"),
        "Finding should mention getMessage"
    );
}

#[test]
fn test_fp_conflict_exception() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;

        @RestController
        public class EmailController {
            @PostMapping("/change-email")
            public ResponseEntity<?> changeEmail(@RequestBody EmailRequest request) {
                try {
                    emailService.changeEmail(request);
                    return ResponseEntity.ok("Email changed");
                } catch (ConflictException e) {
                    return ResponseEntity.status(409).body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    assert!(
        stack_trace.is_empty(),
        "Should NOT flag getMessage() for ConflictException. Found {} findings",
        stack_trace.len()
    );
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_edge_case_non_web_class_with_printstacktrace() {
    let source = r#"
        public class DataProcessor {
            public void processFile(String filename) {
                try {
                    readFile(filename);
                } catch (Exception e) {
                    e.printStackTrace();
                }
            }

            private void readFile(String filename) {
                // implementation
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    // Non-web context (no @RestController, no main method)
    // Current implementation checks is_in_web_context, which should return false
    assert!(
        stack_trace.is_empty(),
        "Should NOT flag printStackTrace() in non-web utility class. Found {} findings",
        stack_trace.len()
    );
}

#[test]
fn test_edge_case_mixed_public_and_admin_endpoints() {
    let source = r#"
        import org.springframework.web.bind.annotation.*;
        import org.springframework.http.ResponseEntity;
        import org.springframework.security.access.prepost.PreAuthorize;

        @RestController
        public class MixedController {
            // Public endpoint - should be flagged
            @GetMapping("/api/public/data")
            public ResponseEntity<String> getPublicData() {
                try {
                    return ResponseEntity.ok(service.getData());
                } catch (Exception e) {
                    return ResponseEntity.status(500).body(e.getMessage());
                }
            }

            // Admin endpoint - should NOT be flagged
            @GetMapping("/admin/internal/data")
            @PreAuthorize("hasRole('ADMIN')")
            public ResponseEntity<String> getAdminData() {
                try {
                    return ResponseEntity.ok(service.getAdminData());
                } catch (Exception e) {
                    return ResponseEntity.status(500).body(e.getMessage());
                }
            }
        }
    "#;

    let findings = check_code(source);
    let stack_trace = get_stack_trace_findings(&findings);

    fn find_line_1_based(source: &str, needle: &str) -> u32 {
        for (i, line) in source.lines().enumerate() {
            if line.contains(needle) {
                return u32::try_from(i + 1).unwrap_or(1);
            }
        }
        panic!("Expected to find {needle:?} in source");
    }

    let admin_method_line = find_line_1_based(source, "getAdminData");

    assert_eq!(
        stack_trace.len(),
        1,
        "exactly the public endpoint should be flagged"
    );

    let has_public_finding = stack_trace
        .iter()
        .any(|f| f.source_location.line < admin_method_line);
    let has_admin_finding = stack_trace
        .iter()
        .any(|f| f.source_location.line >= admin_method_line);

    assert!(
        has_public_finding && !has_admin_finding,
        "the public endpoint must be reported without leaking suppression across method boundaries"
    );
}
