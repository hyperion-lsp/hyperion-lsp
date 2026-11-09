// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner};
use std::path::Path;

fn main() {
    let source = r#"
public class CryptoTest {
    public void encrypt() {
        Cipher cipher = Cipher.getInstance("AES/ECB/PKCS5Padding");
        cipher.init(Cipher.ENCRYPT_MODE, key);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CryptoTest.java"));

    println!("Total findings: {}", findings.len());
    for finding in &findings {
        println!("  - {:?}: {}", finding.vuln_type, finding.message);
        println!("    Code: {}", finding.code_snippet);
    }
}
