// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::needless_collect)]

//! Race condition false positive reduction - Phase 5: String Immutability
//!
//! Tests for Phase 5 of FP Reduction Round 5.
//! Verifies that immutable types (String, Integer, etc.) are not flagged for race conditions.

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

// ============================================================================
// String Immutability - Should NOT Flag (Phase 5)
// ============================================================================

#[test]
fn test_string_length_no_race_condition() {
    let source = r#"
        public class Validator {
            public boolean validate(String name) {
                if (name.length() > 0) {
                    return processName(name);
                }
                return false;
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        race_findings.is_empty(),
        "Should NOT flag String.length() as race condition (String is immutable)"
    );
}

#[test]
fn test_string_equals_check_no_race_condition() {
    let source = r#"
        public class AuthChecker {
            public void checkAuth(String token) {
                if (token != null && !token.isEmpty()) {
                    if (token.equals("valid")) {
                        grantAccess();
                    }
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        race_findings.is_empty(),
        "Should NOT flag String check-then-use as race condition"
    );
}

// ============================================================================
// Boxed Primitives - Should NOT Flag (Phase 5)
// ============================================================================

#[test]
fn test_integer_check_no_race_condition() {
    let source = r#"
        public class Counter {
            public void processCount(Integer count) {
                if (count > 0) {
                    processPositive(count);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        race_findings.is_empty(),
        "Should NOT flag Integer check as race condition (Integer is immutable)"
    );
}

#[test]
fn test_long_check_no_race_condition() {
    let source = r#"
        public class IdValidator {
            public void validate(Long id) {
                if (id != null && id > 0) {
                    processId(id);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        race_findings.is_empty(),
        "Should NOT flag Long check as race condition (Long is immutable)"
    );
}

#[test]
fn test_boolean_check_no_race_condition() {
    let source = r#"
        public class FeatureFlag {
            public void checkFlag(Boolean enabled) {
                if (enabled != null && enabled) {
                    enableFeature();
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        race_findings.is_empty(),
        "Should NOT flag Boolean check as race condition (Boolean is immutable)"
    );
}

// ============================================================================
// Big Numbers - Should NOT Flag (Phase 5)
// ============================================================================

#[test]
fn test_bigdecimal_check_no_race_condition() {
    let source = r#"
        public class PriceCalculator {
            public void calculate(BigDecimal price) {
                if (price.compareTo(BigDecimal.ZERO) > 0) {
                    applyDiscount(price);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        race_findings.is_empty(),
        "Should NOT flag BigDecimal check as race condition (BigDecimal is immutable)"
    );
}

#[test]
fn test_biginteger_check_no_race_condition() {
    let source = r#"
        public class MathProcessor {
            public void process(BigInteger value) {
                if (value.compareTo(BigInteger.ZERO) > 0) {
                    processPositive(value);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        race_findings.is_empty(),
        "Should NOT flag BigInteger check as race condition (BigInteger is immutable)"
    );
}

// ============================================================================
// Java Time API - Should NOT Flag (Phase 5)
// ============================================================================

#[test]
fn test_localdate_check_no_race_condition() {
    let source = r#"
        public class DateValidator {
            public void validate(LocalDate date) {
                if (date.isAfter(LocalDate.now())) {
                    scheduleFuture(date);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        race_findings.is_empty(),
        "Should NOT flag LocalDate check as race condition (LocalDate is immutable)"
    );
}

#[test]
fn test_instant_check_no_race_condition() {
    let source = r#"
        public class TimestampValidator {
            public void validate(Instant timestamp) {
                if (timestamp.isBefore(Instant.now())) {
                    processPast(timestamp);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        race_findings.is_empty(),
        "Should NOT flag Instant check as race condition (Instant is immutable)"
    );
}

// ============================================================================
// Other Immutable Types - Should NOT Flag (Phase 5)
// ============================================================================

#[test]
fn test_uuid_check_no_race_condition() {
    let source = r#"
        public class IdProcessor {
            public void process(UUID id) {
                if (id != null) {
                    processId(id);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        race_findings.is_empty(),
        "Should NOT flag UUID check as race condition (UUID is immutable)"
    );
}

// ============================================================================
// Mutable Collections - Still Flag (Regression Test)
// ============================================================================

#[test]
fn test_list_field_still_flagged() {
    // Verify that mutable collections are still flagged
    let source = r#"
        public class TaskQueue {
            private final List<Task> tasks;

            public TaskQueue(List<Task> tasks) {
                this.tasks = tasks;
            }

            public void processNext() {
                if (!tasks.isEmpty()) {
                    Task task = tasks.get(0);
                    tasks.remove(0);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        !race_findings.is_empty(),
        "Should STILL flag mutable collection race conditions (true positive preserved)"
    );
}

#[test]
fn test_map_field_still_flagged() {
    // Verify that mutable maps are still flagged
    let source = r#"
        public class Cache {
            private final Map<String, Object> cache;

            public Cache(Map<String, Object> cache) {
                this.cache = cache;
            }

            public Object retrieve(String key) {
                if (!cache.isEmpty() && cache.containsKey(key)) {
                    return cache.get(key);
                }
                return null;
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        !race_findings.is_empty(),
        "Should STILL flag mutable map race conditions (true positive preserved)"
    );
}
