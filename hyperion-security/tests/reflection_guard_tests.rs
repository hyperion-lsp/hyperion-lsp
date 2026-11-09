// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! A `getClass()` receiver is trusted, but a method name read from a request is not.

use hyperion_security::{ScanConfig, SecurityScanner, Severity, VulnType};
use std::path::Path;

fn reflection_findings(source: &str) -> usize {
    SecurityScanner::new(ScanConfig::default())
        .scan_source(source, Path::new("Dispatch.java"))
        .iter()
        .filter(|finding| finding.vuln_type == VulnType::UnsafeReflection)
        .count()
}

#[test]
fn method_name_from_a_request_is_flagged_even_on_a_getclass_receiver() {
    let source = "class Dispatch {\n    Object run(Object obj, HttpServletRequest request) throws Exception {\n        return obj.getClass().getMethod(request.getParameter(\"m\")).invoke(obj);\n    }\n}\n";
    assert!(reflection_findings(source) >= 1);
}

#[test]
fn a_framework_marker_elsewhere_in_the_file_does_not_hide_a_request_driven_call() {
    // The proxy marker in the file would suppress an ordinary forName, but this call
    // takes its class name from the request.
    let source = "class Dispatch {\n    static final Object handler = java.lang.reflect.Proxy.class;\n    Object run(HttpServletRequest request) throws Exception {\n        return Class.forName(request.getParameter(\"c\")).newInstance();\n    }\n}\n";
    assert!(reflection_findings(source) >= 1);
}

#[test]
fn a_request_driven_call_is_not_downgraded_for_mentioning_a_default() {
    // "defaultArgs" reads like configuration, but the arguments come from the request.
    let source = "class Dispatch {\n    Object run(Object obj, HttpServletRequest request) throws Exception {\n        Method method = obj.getClass().getMethod(request.getParameter(\"m\"));\n        return method.invoke(obj, request.getParameter(\"defaultArgs\"));\n    }\n}\n";
    let severities: Vec<Severity> = SecurityScanner::new(ScanConfig::default())
        .scan_source(source, Path::new("Dispatch.java"))
        .iter()
        .filter(|finding| finding.vuln_type == VulnType::UnsafeReflection)
        .map(|finding| finding.severity)
        .collect();
    assert!(!severities.is_empty());
    assert!(
        severities.iter().all(|severity| *severity != Severity::Low),
        "a request-driven call must not be downgraded to Low: {severities:?}"
    );
}
