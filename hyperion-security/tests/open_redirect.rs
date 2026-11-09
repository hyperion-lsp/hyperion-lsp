// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_servlet_send_redirect() {
    let source = r#"
public class RedirectController {
    public void redirect(@RequestParam String url, HttpServletResponse response) throws IOException {
        response.sendRedirect(url);  // Open redirect
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RedirectController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect open redirect in sendRedirect"
    );
    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::OpenRedirect),
        "Should detect OpenRedirect vuln type"
    );
    assert_eq!(
        findings[0].severity,
        hyperion_security::Severity::Medium,
        "Open redirect should be Medium severity"
    );
}

#[test]
fn test_servlet_redirect_from_request() {
    let source = r#"
public class LoginController {
    public void login(HttpServletRequest request, HttpServletResponse response) throws IOException {
        String returnUrl = request.getParameter("returnUrl");
        // Authentication logic...
        response.sendRedirect(returnUrl);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("LoginController.java"));

    assert!(
        !findings.is_empty(),
        "Should detect open redirect from request parameter"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::OpenRedirect));
}

#[test]
fn test_spring_redirect_view_url() {
    let source = r#"
public class SpringRedirectController {
    public RedirectView redirect(@PathVariable String target) {
        RedirectView redirectView = new RedirectView();
        redirectView.setUrl(target);
        return redirectView;
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SpringRedirectController.java"));

    assert!(
        !findings.is_empty(),
        "Should detect open redirect in RedirectView.setUrl"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::OpenRedirect));
}

#[test]
fn test_model_and_view_redirect() {
    let source = r#"
public class MvcController {
    public ModelAndView redirectUser(HttpServletRequest request) {
        String url = request.getParameter("url");
        ModelAndView modelAndView = new ModelAndView();
        modelAndView.setRedirectUrl(url);
        return modelAndView;
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("MvcController.java"));

    assert!(
        !findings.is_empty(),
        "Should detect open redirect in ModelAndView.setRedirectUrl"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::OpenRedirect));
}

#[test]
fn test_jaxrs_see_other() {
    let source = r#"
public class JaxRsController {
    @GET
    public Response redirect(@QueryParam("url") String redirectUrl) {
        return Response.seeOther(URI.create(redirectUrl)).build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JaxRsController.java"));

    assert!(
        !findings.is_empty(),
        "Should detect open redirect in Response.seeOther"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::OpenRedirect));
}

#[test]
fn test_jaxrs_temporary_redirect() {
    let source = r#"
public class ApiController {
    public Response redirect(@RequestParam String location) {
        return Response.temporaryRedirect(URI.create(location)).build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ApiController.java"));

    assert!(
        !findings.is_empty(),
        "Should detect open redirect in Response.temporaryRedirect"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::OpenRedirect));
}

#[test]
fn test_jaxrs_created() {
    let source = r#"
public class ResourceController {
    public Response createResource(@HeaderParam("Location") String location) {
        return Response.created(URI.create(location)).build();
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ResourceController.java"));

    assert!(
        !findings.is_empty(),
        "Should detect open redirect in Response.created"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::OpenRedirect));
}

#[test]
fn test_safe_redirect_no_false_positive() {
    // Redirect to constant URL, not user input
    let source = r#"
public class SafeRedirectController {
    public void redirectToHome(HttpServletResponse response) throws IOException {
        response.sendRedirect("/home");
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SafeRedirectController.java"));

    assert!(
        findings.is_empty(),
        "Should not detect open redirect for constant URLs"
    );
}

#[test]
fn test_multi_step_redirect() {
    let source = r#"
public class LoginRedirect {
    public void afterLogin(HttpServletRequest request, HttpServletResponse response) throws IOException {
        String returnUrl = request.getParameter("returnUrl");
        String redirectTarget = returnUrl;
        response.sendRedirect(redirectTarget);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("LoginRedirect.java"));

    assert!(
        !findings.is_empty(),
        "Should detect open redirect through data flow"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::OpenRedirect));

    // Verify data flow tracking
    let finding = &findings[0];
    assert!(
        !finding.dataflow.is_empty(),
        "Should have data flow path tracked"
    );
}

#[test]
fn test_redirect_with_validation_pattern() {
    // This would be safe in practice, but our scanner might still flag it
    // In Phase 3+, we'd add sanitizer recognition
    let source = r#"
public class ValidatedRedirect {
    public void redirect(@RequestParam String url, HttpServletResponse response) throws IOException {
        // Validation logic would go here
        if (url.startsWith("/")) {
            response.sendRedirect(url);
        }
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ValidatedRedirect.java"));

    // For now, this will still trigger (conservative approach)
    // Future phases can add sanitizer recognition
    eprintln!("Findings: {:?}", findings);
}
