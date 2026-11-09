// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::finding::Location;
use hyperion_security::{diff_findings, Finding, VulnType};
use std::path::PathBuf;

#[test]
fn test_diff_findings_by_id() {
    let f1 = Finding::new(
        VulnType::SqlInjection,
        "SQL injection".to_string(),
        Location {
            file: PathBuf::from("A.java"),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 10,
        },
        "code".to_string(),
    );
    let f2 = Finding::new(
        VulnType::CommandInjection,
        "Command injection".to_string(),
        Location {
            file: PathBuf::from("A.java"),
            line: 2,
            column: 1,
            end_line: 2,
            end_column: 10,
        },
        "code".to_string(),
    );

    let diff = diff_findings(&[f1.clone(), f2.clone()], &[f1]);
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].id, f2.id);
}
