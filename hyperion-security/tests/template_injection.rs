// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_velocity_template_injection() {
    let source = r#"
public class TemplateRenderer {
    public void renderTemplate(HttpServletRequest request) {
        String templateStr = request.getParameter("template");
        VelocityEngine ve = new VelocityEngine();
        ve.evaluate(context, writer, "UserTemplate", templateStr);  // CRITICAL
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("TemplateRenderer.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect template injection in Velocity"
    );
    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::TemplateInjection),
        "Should detect TemplateInjection vuln type"
    );
    assert_eq!(
        findings[0].severity,
        hyperion_security::Severity::Critical,
        "Template injection should be Critical severity"
    );
}

#[test]
fn test_velocity_merge_template_injection() {
    let source = r#"
public class TemplateRenderer {
    public void renderTemplate(@RequestParam String template) {
        VelocityEngine ve = new VelocityEngine();
        ve.merge(template, context, writer);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("TemplateRenderer.java"));

    assert!(
        !findings.is_empty(),
        "Should detect template injection in Velocity.merge"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::TemplateInjection));
}

#[test]
fn test_spel_injection() {
    let source = r#"
public class ExpressionEval {
    public Object evaluateExpression(@RequestParam String expr) {
        ExpressionParser parser = new SpelExpressionParser();
        return parser.parseExpression(expr).getValue();  // RCE risk
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ExpressionEval.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SpEL injection from parseExpression"
    );
    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::TemplateInjection),
        "SpEL should be detected as TemplateInjection"
    );
    assert_eq!(findings[0].severity, hyperion_security::Severity::Critical);
}

#[test]
fn test_freemarker_template_injection() {
    let source = r#"
public class FreemarkerController {
    public void render(HttpServletRequest request) throws Exception {
        String templateName = request.getParameter("template");
        Configuration config = new Configuration();
        Template template = config.getTemplate(templateName);
        template.process(dataModel, writer);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("FreemarkerController.java"));

    assert!(
        !findings.is_empty(),
        "Should detect Freemarker template injection"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::TemplateInjection));
}

#[test]
fn test_thymeleaf_template_injection() {
    let source = r#"
public class ThymeleafController {
    public void render(@PathVariable String template) {
        TemplateEngine templateEngine = new TemplateEngine();
        templateEngine.process(template, context, writer);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ThymeleafController.java"));

    assert!(
        !findings.is_empty(),
        "Should detect Thymeleaf template injection"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::TemplateInjection));
}

#[test]
fn test_safe_template_usage_no_false_positive() {
    // Template name from constant, not user input
    let source = r#"
public class SafeTemplateRenderer {
    public void renderTemplate() {
        String templateName = "userProfile.ftl";
        Configuration config = new Configuration();
        Template template = config.getTemplate(templateName);
        template.process(dataModel, writer);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SafeTemplateRenderer.java"));

    assert!(
        findings.is_empty(),
        "Should not detect template injection for constant template names"
    );
}

#[test]
fn test_multi_step_template_injection() {
    // Test data flow tracking through assignment
    let source = r#"
public class TemplateController {
    public void render(HttpServletRequest request) {
        String userInput = request.getParameter("expr");
        String expression = userInput;
        ExpressionParser parser = new SpelExpressionParser();
        parser.parseExpression(expression);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("TemplateController.java"));

    assert!(
        !findings.is_empty(),
        "Should detect template injection through data flow"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::TemplateInjection));

    // Verify data flow is tracked
    let finding = &findings[0];
    assert!(
        !finding.dataflow.is_empty(),
        "Should have data flow path tracked"
    );
}
