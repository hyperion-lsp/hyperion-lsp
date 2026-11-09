// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_jsf_value_expression() {
    let source = r#"
public class JsfController {
    public void evaluateExpression(HttpServletRequest request) {
        String expr = request.getParameter("expression");
        ExpressionFactory factory = ExpressionFactory.newInstance();
        ValueExpression valueExpression = factory.createValueExpression(context, expr, String.class);
        valueExpression.getValue(context);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JsfController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect EL injection in ValueExpression.getValue"
    );
    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::ExpressionLanguageInjection),
        "Should detect ExpressionLanguageInjection vuln type"
    );
    assert_eq!(
        findings[0].severity,
        hyperion_security::Severity::High,
        "EL injection should be High severity"
    );
}

#[test]
fn test_jsf_method_expression() {
    let source = r#"
public class ManagedBean {
    public void invoke(@RequestParam String methodExpr) {
        ExpressionFactory factory = ExpressionFactory.newInstance();
        MethodExpression methodExpression = factory.createMethodExpression(context, methodExpr, null, new Class[]{});
        methodExpression.invoke(context, null);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ManagedBean.java"));

    assert!(
        !findings.is_empty(),
        "Should detect EL injection in MethodExpression.invoke"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::ExpressionLanguageInjection));
}

#[test]
fn test_jsf_set_value() {
    let source = r#"
public class ValueSetter {
    public void setValue(HttpServletRequest request) {
        String expr = request.getParameter("expr");
        ValueExpression valueExpression = factory.createValueExpression(context, expr, Object.class);
        valueExpression.setValue(context, "newValue");
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ValueSetter.java"));

    assert!(
        !findings.is_empty(),
        "Should detect EL injection in ValueExpression.setValue"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::ExpressionLanguageInjection));
}

#[test]
fn test_ognl_get_value() {
    let source = r#"
public class OgnlEvaluator {
    public void evaluate(@PathVariable String expression) throws Exception {
        Object root = new HashMap();
        Object result = Ognl.getValue(expression, root);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("OgnlEvaluator.java"));

    assert!(
        !findings.is_empty(),
        "Should detect OGNL injection in Ognl.getValue"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::ExpressionLanguageInjection));
}

#[test]
fn test_ognl_set_value() {
    let source = r#"
public class OgnlSetter {
    public void set(@RequestParam String expr, Object value) throws Exception {
        Object root = new HashMap();
        Ognl.setValue(expr, root, value);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("OgnlSetter.java"));

    assert!(
        !findings.is_empty(),
        "Should detect OGNL injection in Ognl.setValue"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::ExpressionLanguageInjection));
}

#[test]
fn test_ognl_parse_expression() {
    let source = r#"
public class StrutsAction {
    public void execute(HttpServletRequest request) throws Exception {
        String expression = request.getParameter("expr");
        Object tree = Ognl.parseExpression(expression);
        Ognl.getValue(tree, context, root);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("StrutsAction.java"));

    assert!(
        !findings.is_empty(),
        "Should detect OGNL injection in Ognl.parseExpression"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::ExpressionLanguageInjection));
}

#[test]
fn test_jexl_create_expression() {
    let source = r#"
public class JexlEvaluator {
    public void evaluate(@RequestParam String expr) {
        JexlEngine jexlEngine = new JexlBuilder().create();
        JexlExpression expression = jexlEngine.createExpression(expr);
        Object result = expression.evaluate(context);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JexlEvaluator.java"));

    assert!(
        !findings.is_empty(),
        "Should detect JEXL injection in createExpression"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::ExpressionLanguageInjection));
}

#[test]
fn test_jexl_evaluate() {
    let source = r#"
public class JexlService {
    public Object eval(HttpServletRequest request) {
        String expression = request.getParameter("expression");
        return jexlEngine.evaluate(expression);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JexlService.java"));

    assert!(
        !findings.is_empty(),
        "Should detect JEXL injection in evaluate"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::ExpressionLanguageInjection));
}

#[test]
fn test_safe_el_no_false_positive() {
    // Constant expression, no user input
    let source = r#"
public class SafeELController {
    public void evaluateConstant() {
        String expr = "constant_expression";
        ValueExpression valueExpression = factory.createValueExpression(context, expr, String.class);
        valueExpression.getValue(context);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SafeELController.java"));

    assert!(
        findings.is_empty(),
        "Should not detect EL injection for constant expressions"
    );
}

#[test]
fn test_multi_step_el_injection() {
    let source = r#"
public class ExpressionHandler {
    public void handle(HttpServletRequest request) throws Exception {
        String userInput = request.getParameter("expr");
        String expression = userInput;
        Object result = Ognl.getValue(expression, root);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ExpressionHandler.java"));

    assert!(
        !findings.is_empty(),
        "Should detect EL injection through data flow"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::ExpressionLanguageInjection));

    // Verify data flow tracking
    let finding = &findings[0];
    assert!(
        !finding.dataflow.is_empty(),
        "Should have data flow path tracked"
    );
}

#[test]
fn test_ognl_context_injection() {
    let source = r#"
public class OgnlContextHandler {
    public void process(@PathVariable String expr) throws Exception {
        OgnlContext context = new OgnlContext();
        ognl.getValue(expr, context, root);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("OgnlContextHandler.java"));

    assert!(
        !findings.is_empty(),
        "Should detect OGNL injection with OgnlContext"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::ExpressionLanguageInjection));
}
