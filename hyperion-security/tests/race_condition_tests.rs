// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::needless_collect)]

//! Phase 2: TOCTOU Context Awareness Tests (FP Reduction Round 6)
//!
//! These tests verify that we skip TOCTOU findings in benign contexts while
//! still detecting security-relevant file operation race conditions.

use hyperion_parser::{Lexer, Parser};
use hyperion_security::checks::{CheckContext, CheckRegistry};
use hyperion_security::finding::{Severity, VulnType};
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

#[test]
fn test_toctou_cache_operation_skipped() {
    let code = r#"
        public class CacheManager {
            private File cacheDir;

            public void ensureCache() {
                if (!cacheDir.exists()) {
                    cacheDir.mkdirs();
                }
            }

            public void cleanCache() throws IOException {
                File tempFile = new File(cacheDir, "temp.cache");
                if (tempFile.exists()) {
                    tempFile.delete();
                }
            }
        }
    "#;

    let findings = check_code(code);
    let toctou_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition && f.message.to_lowercase().contains("toctou")
        })
        .collect();

    assert_eq!(
        toctou_findings.len(),
        0,
        "Cache directory TOCTOU should be skipped"
    );
}

#[test]
fn test_toctou_user_controlled_path_detected() {
    let code = r#"
        @RestController
        public class FileUploadController {
            @PostMapping("/upload")
            public void uploadFile(@RequestParam String filename) throws IOException {
                File uploadFile = new File("/uploads", filename);
                if (!uploadFile.exists()) {
                    uploadFile.createNewFile();
                }
                FileOutputStream fos = new FileOutputStream(uploadFile);
                fos.close();
            }
        }
    "#;

    let findings = check_code(code);
    let toctou_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition && f.message.to_lowercase().contains("toctou")
        })
        .collect();

    assert_eq!(
        toctou_findings.len(),
        1,
        "User-controlled path TOCTOU should be detected"
    );

    let finding = &toctou_findings[0];
    assert_eq!(finding.severity, Severity::High, "Should be high severity");
    assert!(
        finding.confidence > 0.5,
        "Should have high confidence for user-controlled paths"
    );
}

#[test]
fn test_toctou_config_file_skipped() {
    let code = r#"
        public class ConfigLoader {
            private static final String CONFIG_PATH = System.getProperty("app.config", "/etc/app/config.properties");

            public void loadConfig() throws IOException {
                File configFile = new File(CONFIG_PATH);
                if (configFile.exists()) {
                    FileInputStream fis = new FileInputStream(configFile);
                    loadProperties(fis);
                    fis.close();
                }
            }

            private void loadProperties(FileInputStream fis) {
                // Load properties
            }
        }
    "#;

    let findings = check_code(code);
    let toctou_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition && f.message.to_lowercase().contains("toctou")
        })
        .collect();

    assert_eq!(
        toctou_findings.len(),
        0,
        "Config file TOCTOU should be skipped"
    );
}

#[test]
fn test_toctou_regular_file_operation_detected() {
    let code = r#"
        public class FileHandler {
            public String readFile(File file) throws IOException {
                if (file.exists()) {
                    FileInputStream fis = new FileInputStream(file);
                    return new String(fis.readAllBytes());
                }
                return null;
            }
        }
    "#;

    let findings = check_code(code);
    let toctou_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition && f.message.to_lowercase().contains("toctou")
        })
        .collect();

    assert_eq!(
        toctou_findings.len(),
        1,
        "Regular file TOCTOU should still be detected"
    );

    let finding = &toctou_findings[0];
    assert_eq!(
        finding.severity,
        Severity::Medium,
        "Should be medium severity for non-user-controlled"
    );
}

#[test]
fn test_toctou_system_out_logging_skipped() {
    let code = r#"
        public class DebugHandler {
            public void checkFile(File dataFile) {
                if (dataFile.exists()) {
                    System.out.println("File exists: " + dataFile.getAbsolutePath());
                } else {
                    System.out.println("File not found");
                }
            }
        }
    "#;

    let findings = check_code(code);
    let toctou_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition && f.message.to_lowercase().contains("toctou")
        })
        .collect();

    assert_eq!(
        toctou_findings.len(),
        0,
        "System.out.println logging should skip TOCTOU detection"
    );
}

#[test]
fn test_toctou_multipart_file_upload_detected() {
    let code = r#"
        @RestController
        public class UploadController {
            @PostMapping("/api/upload")
            public void handleUpload(@RequestParam("file") MultipartFile uploadedFile) throws IOException {
                File targetFile = new File("/uploads", uploadedFile.getOriginalFilename());
                if (!targetFile.exists()) {
                    targetFile.createNewFile();
                }
                FileOutputStream fos = new FileOutputStream(targetFile);
                fos.write(uploadedFile.getBytes());
                fos.close();
            }
        }
    "#;

    let findings = check_code(code);
    let toctou_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition && f.message.to_lowercase().contains("toctou")
        })
        .collect();

    assert_eq!(
        toctou_findings.len(),
        1,
        "MultipartFile upload with TOCTOU should be detected"
    );

    let finding = &toctou_findings[0];
    assert_eq!(finding.severity, Severity::High, "Should be high severity");
}

#[test]
fn test_toctou_application_yml_config_skipped() {
    let code = r#"
        public class YamlConfigLoader {
            public void loadYamlConfig() throws IOException {
                File configFile = new File("application.yml");
                if (configFile.exists()) {
                    FileInputStream fis = new FileInputStream(configFile);
                    // Parse YAML
                    fis.close();
                }
            }
        }
    "#;

    let findings = check_code(code);
    let toctou_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition && f.message.to_lowercase().contains("toctou")
        })
        .collect();

    assert_eq!(
        toctou_findings.len(),
        0,
        "application.yml config loading should be skipped"
    );
}

// ============================================================================
// Phase 3: Local Variable Race Conditions Tests (FP Reduction Round 6)
// ============================================================================

#[test]
fn test_local_variable_race_skipped() {
    let code = r#"
        import java.util.List;
        import java.util.ArrayList;

        public class ItemProcessor {
            void processItems() {
                List<String> items = new ArrayList<>();
                items.addAll(getItems());

                if (!items.isEmpty()) {
                    String first = items.get(0);
                    System.out.println(first);
                }
            }

            private List<String> getItems() {
                return new ArrayList<>();
            }
        }
    "#;

    let findings = check_code(code);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition
                && f.message.to_lowercase().contains("collection")
        })
        .collect();

    assert_eq!(
        race_findings.len(),
        0,
        "Local variable race should be skipped - items is declared locally"
    );
}

#[test]
fn test_instance_field_race_detected() {
    let code = r#"
        import java.util.List;

        public class Cache {
            private List<String> cachedItems;

            public String getFirst() {
                if (!cachedItems.isEmpty()) {
                    return cachedItems.get(0);
                }
                return null;
            }
        }
    "#;

    let findings = check_code(code);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition
                && f.message.to_lowercase().contains("collection")
        })
        .collect();

    assert_eq!(
        race_findings.len(),
        1,
        "Instance field race should be detected - cachedItems is a field"
    );

    let finding = &race_findings[0];
    assert_eq!(finding.severity, Severity::Medium);
}

#[test]
fn test_synchronized_method_race_skipped() {
    let code = r#"
        import java.util.List;

        public class Cache {
            private List<String> cachedItems;

            public synchronized String getFirst() {
                if (!cachedItems.isEmpty()) {
                    return cachedItems.get(0);
                }
                return null;
            }
        }
    "#;

    let findings = check_code(code);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition
                && f.message.to_lowercase().contains("collection")
        })
        .collect();

    assert_eq!(
        race_findings.len(),
        0,
        "Synchronized method should be skipped - proper synchronization"
    );
}

#[test]
fn test_concurrent_collection_race_skipped() {
    let code = r#"
        import java.util.concurrent.ConcurrentHashMap;
        import java.util.Map;

        public class Cache {
            private ConcurrentHashMap<String, String> items;

            public String getFirst() {
                if (!items.isEmpty()) {
                    return items.values().iterator().next();
                }
                return null;
            }
        }
    "#;

    let findings = check_code(code);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition
                && f.message.to_lowercase().contains("collection")
        })
        .collect();

    assert_eq!(
        race_findings.len(),
        0,
        "Concurrent collection should be skipped - thread-safe by design"
    );
}

#[test]
fn test_val_local_variable_race_skipped() {
    let code = r#"
        import java.util.List;

        public class KotlinStyleProcessor {
            void process() {
                val items = getItems();

                if (!items.isEmpty()) {
                    val first = items.get(0);
                    System.out.println(first);
                }
            }

            private List<String> getItems() {
                return null;
            }
        }
    "#;

    let findings = check_code(code);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition
                && f.message.to_lowercase().contains("collection")
        })
        .collect();

    assert_eq!(
        race_findings.len(),
        0,
        "Kotlin-style 'val' local variable should be skipped"
    );
}

#[test]
fn test_request_parameter_local_assignment_race_skipped() {
    let code = r#"
        import java.util.List;
        import org.springframework.web.bind.annotation.*;

        @RestController
        public class Controller {
            @PostMapping("/process")
            public void process(@RequestBody Request request) {
                List<String> data = request.getData();

                if (!data.isEmpty()) {
                    String first = data.get(0);
                    System.out.println(first);
                }
            }
        }

        class Request {
            public List<String> getData() {
                return null;
            }
        }
    "#;

    let findings = check_code(code);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f.vuln_type == VulnType::RaceCondition
                && f.message.to_lowercase().contains("collection")
        })
        .collect();

    assert_eq!(
        race_findings.len(),
        0,
        "Local variable from request parameter getter should be skipped"
    );
}
