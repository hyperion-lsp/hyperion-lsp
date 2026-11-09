// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, Severity, VulnType};
use std::path::Path;

#[test]
fn test_thread_stop() {
    let source = r#"
public class ThreadTest {
    public void stopThread(Thread thread) {
        thread.stop();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ThreadTest.java"));

    assert!(!findings.is_empty(), "Should detect Thread.stop()");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::DangerousMethod);
    assert!(finding.is_some(), "Should find DangerousMethod");
    assert_eq!(finding.unwrap().severity, Severity::Medium);
}

#[test]
fn test_thread_suspend() {
    let source = r#"
public class ThreadTest {
    public void pauseThread(Thread thread) {
        thread.suspend();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ThreadTest.java"));

    assert!(!findings.is_empty(), "Should detect Thread.suspend()");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::DangerousMethod);
    assert!(finding.is_some(), "Should find DangerousMethod");
}

#[test]
fn test_thread_resume() {
    let source = r#"
public class ThreadTest {
    public void continueThread(Thread thread) {
        thread.resume();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ThreadTest.java"));

    assert!(!findings.is_empty(), "Should detect Thread.resume()");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::DangerousMethod);
    assert!(finding.is_some(), "Should find DangerousMethod");
}

#[test]
fn test_thread_destroy() {
    let source = r#"
public class ThreadTest {
    public void killThread(Thread thread) {
        thread.destroy();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ThreadTest.java"));

    assert!(!findings.is_empty(), "Should detect Thread.destroy()");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::DangerousMethod);
    assert!(finding.is_some(), "Should find DangerousMethod");
}

#[test]
fn test_thread_interrupt_safe() {
    let source = r#"
public class ThreadTest {
    public void stopThread(Thread thread) {
        thread.interrupt();  // Safe alternative
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ThreadTest.java"));

    let dangerous_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::DangerousMethod)
        .count();
    assert_eq!(dangerous_findings, 0, "Should not flag Thread.interrupt()");
}

#[test]
fn test_system_exit() {
    let source = r#"
public class AppTest {
    public void shutdown() {
        System.exit(0);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AppTest.java"));

    assert!(!findings.is_empty(), "Should detect System.exit()");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::DangerousMethod);
    assert!(finding.is_some(), "Should find DangerousMethod");
    assert_eq!(finding.unwrap().severity, Severity::Medium);
}

#[test]
fn test_system_exit_with_code() {
    let source = r#"
public class AppTest {
    public void handleError() {
        System.exit(1);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AppTest.java"));

    assert!(!findings.is_empty(), "Should detect System.exit(1)");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::DangerousMethod);
    assert!(finding.is_some(), "Should find DangerousMethod");
}

#[test]
fn test_runtime_halt() {
    let source = r#"
public class AppTest {
    public void emergencyShutdown() {
        Runtime.getRuntime().halt(1);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AppTest.java"));

    assert!(!findings.is_empty(), "Should detect Runtime.halt()");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::DangerousMethod);
    assert!(finding.is_some(), "Should find DangerousMethod");
    assert_eq!(finding.unwrap().severity, Severity::High);
}

#[test]
fn test_class_forname_reflection() {
    let source = r#"
public class ReflectionTest {
    public Object loadClass(String className) throws Exception {
        Class<?> clazz = Class.forName(className);
        return clazz.newInstance();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ReflectionTest.java"));

    assert!(!findings.is_empty(), "Should detect Class.forName()");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::UnsafeReflection);
    assert!(finding.is_some(), "Should find UnsafeReflection");
    // Medium severity: the class name comes from a method parameter, but there's no
    // indication in the file that it's from user input (no HttpServletRequest, etc.)
    assert_eq!(finding.unwrap().severity, Severity::Medium);
}

#[test]
fn test_method_invoke_reflection() {
    let source = r#"
public class ReflectionTest {
    public Object invokeMethod(Object obj, String methodName) throws Exception {
        Method method = obj.getClass().getMethod(methodName);
        return method.invoke(obj);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ReflectionTest.java"));

    assert!(!findings.is_empty(), "Should detect Method.invoke()");
    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::UnsafeReflection);
    assert!(finding.is_some(), "Should find UnsafeReflection");
    // Medium severity: the method name comes from a parameter, but there's no
    // indication in the file that it's from user input (no HttpServletRequest, etc.)
    assert_eq!(finding.unwrap().severity, Severity::Medium);
}

#[test]
fn test_newinstance_reflection() {
    let source = r#"
public class ReflectionTest {
    public Object createInstance(String className) throws Exception {
        Class<?> clazz = Class.forName(className);
        return clazz.newInstance();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ReflectionTest.java"));

    assert!(
        !findings.is_empty(),
        "Should detect newInstance() with reflection"
    );
    let reflection_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::UnsafeReflection)
        .count();
    assert!(
        reflection_findings >= 1,
        "Should find at least one UnsafeReflection"
    );
}

#[test]
fn test_safe_reflection_with_allowlist() {
    let source = r#"
public class ReflectionTest {
    private static final Set<String> ALLOWED_CLASSES = Set.of(
        "com.example.SafeClass1",
        "com.example.SafeClass2"
    );

    public Object loadClass(String className) throws Exception {
        if (!ALLOWED_CLASSES.contains(className)) {
            throw new SecurityException("Class not allowed");
        }
        Class<?> clazz = Class.forName(className);
        return clazz.newInstance();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ReflectionTest.java"));

    // Still should detect reflection usage - the check can't verify allowlist correctness
    let reflection_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::UnsafeReflection)
        .count();
    // This is expected - we warn about reflection even with allowlist present
    assert!(
        reflection_findings > 0,
        "Should still warn about reflection usage"
    );
}

#[test]
fn test_constructor_newinstance() {
    let source = r#"
public class ReflectionTest {
    public Object createObject(String className) throws Exception {
        Class<?> clazz = Class.forName(className);
        Constructor<?> constructor = clazz.getConstructor();
        return constructor.newInstance();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ReflectionTest.java"));

    assert!(
        !findings.is_empty(),
        "Should detect Constructor.newInstance()"
    );
    let reflection_findings = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::UnsafeReflection)
        .count();
    assert!(
        reflection_findings >= 1,
        "Should find UnsafeReflection for Constructor.newInstance()"
    );
}
