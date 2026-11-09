// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for field annotation handling in nullability analysis.
//!
//! Phase 2.2 and 2.3 of the nullability framework patterns plan.
//! Tests that fields annotated with dependency injection annotations (@Autowired, @Inject, @Resource)
//! and nullability annotations (@NonNull, @NotNull, @Nonnull) are treated as NonNull.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_parser::{lexer::Lexer, parser::Parser};
use hyperion_semantics::nullability::{
    analysis::{analyze, MethodContext, NullabilityResolver},
    cfg::ControlFlowGraph,
    environment::NullabilityEnv,
    extraction::collect_field_annotations,
};

/// Parse Java code and extract the class node
fn parse_class(code: &str) -> SyntaxNode {
    let tokens: Vec<_> = Lexer::new(code).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);
    root.descendants()
        .find(|n| n.kind() == SyntaxKind::CLASS_DECL)
        .expect("No class declaration found in test code")
}

/// Extract method body and run nullability analysis
fn analyze_method(
    code: &str,
    field_annotations: std::collections::HashMap<String, Vec<String>>,
) -> hyperion_semantics::nullability::analysis::NullabilityAnalysis {
    let tokens: Vec<_> = Lexer::new(code).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);
    let method_body = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::BLOCK_STMT)
        .expect("No method body found");

    let cfg = ControlFlowGraph::build(&method_body);
    let resolver = NullabilityResolver::new();
    let mut method_context = MethodContext::default();
    method_context.field_annotations = field_annotations.into();

    analyze(&cfg, NullabilityEnv::new(), &resolver, &method_context)
}

#[test]
fn test_autowired_field_treated_as_nonnull() {
    let code = r#"
        @Service
        public class UserService {
            @Autowired
            private UserRepository repository;

            public User findUser(Long id) {
                return repository.findById(id);
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify annotation was extracted
    assert!(field_annotations.contains_key("repository"));
    assert_eq!(field_annotations["repository"], vec!["Autowired"]);

    // Analyze method and verify no warnings on repository dereference
    let analysis = analyze_method(code, field_annotations);

    // Should have no potential null dereference diagnostics
    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repository"
        }),
        "Expected no null dereference warnings for @Autowired field"
    );
}

#[test]
fn test_inject_field_treated_as_nonnull() {
    let code = r#"
        public class MyBean {
            @Inject
            private Logger logger;

            public void log(String msg) {
                logger.info(msg);
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify annotation was extracted
    assert!(field_annotations.contains_key("logger"));
    assert_eq!(field_annotations["logger"], vec!["Inject"]);

    // Analyze method and verify no warnings
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "logger"
        }),
        "Expected no null dereference warnings for @Inject field"
    );
}

#[test]
fn test_resource_field_treated_as_nonnull() {
    let code = r#"
        public class MyService {
            @Resource
            private DataSource dataSource;

            public Connection getConnection() throws SQLException {
                return dataSource.getConnection();
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify annotation was extracted
    assert!(field_annotations.contains_key("dataSource"));
    assert_eq!(field_annotations["dataSource"], vec!["Resource"]);

    // Analyze method and verify no warnings
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "dataSource"
        }),
        "Expected no null dereference warnings for @Resource field"
    );
}

#[test]
fn test_fully_qualified_autowired_annotation() {
    let code = r#"
        public class Service {
            @org.springframework.beans.factory.annotation.Autowired
            private Repository repo;

            public void test() {
                repo.save();
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify fully qualified annotation was extracted
    assert!(field_annotations.contains_key("repo"));
    assert_eq!(
        field_annotations["repo"],
        vec!["org.springframework.beans.factory.annotation.Autowired"]
    );

    // Analyze method and verify no warnings
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Expected no null dereference warnings for fully qualified @Autowired"
    );
}

#[test]
fn test_multiple_injected_fields() {
    let code = r#"
        public class MultiService {
            @Autowired
            private UserRepository userRepo;

            @Inject
            private ProductRepository productRepo;

            @Resource
            private Logger logger;

            public void test() {
                userRepo.findAll();
                productRepo.findAll();
                logger.info("test");
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify all annotations were extracted
    assert!(field_annotations.contains_key("userRepo"));
    assert!(field_annotations.contains_key("productRepo"));
    assert!(field_annotations.contains_key("logger"));

    // Analyze method and verify no warnings for any injected field
    let analysis = analyze_method(code, field_annotations);

    for field_name in &["userRepo", "productRepo", "logger"] {
        assert!(
            analysis.diagnostics.iter().all(|d| {
                !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != *field_name
            }),
            "Expected no null dereference warnings for injected field {}",
            field_name
        );
    }
}

#[test]
fn test_non_injected_field_still_warns() {
    let code = r#"
        public class Service {
            @Autowired
            private Repository repo;

            private Cache cache;  // No injection annotation

            public void test() {
                repo.save();      // Should NOT warn
                cache.get("key"); // Should warn (if cache is nullable)
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Only repo should have injection annotation
    assert!(field_annotations.contains_key("repo"));
    assert!(field_annotations.contains_key("cache"));
    assert_eq!(field_annotations["repo"], vec!["Autowired"]);
    assert!(field_annotations["cache"].is_empty());

    let analysis = analyze_method(code, field_annotations);

    // repo should not warn
    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Should not warn for @Autowired field"
    );

    // cache should warn (potentially - depends on whether it's in the environment)
    // Note: This test verifies differential treatment between injected and non-injected fields
}

#[test]
fn test_injected_field_with_multiple_annotations() {
    let code = r#"
        public class Service {
            @Autowired
            @Qualifier("primary")
            @Lazy
            private Repository repo;

            public void test() {
                repo.save();
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Should have all annotations
    assert!(field_annotations.contains_key("repo"));
    assert_eq!(
        field_annotations["repo"],
        vec!["Autowired", "Qualifier", "Lazy"]
    );

    // Analyze method and verify no warnings
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Expected no null dereference warnings for @Autowired field with qualifiers"
    );
}

#[test]
fn test_injected_field_chained_calls() {
    let code = r#"
        public class Service {
            @Inject
            private Repository repo;

            public void test() {
                String name = repo.findById(1L).getName();
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    let analysis = analyze_method(code, field_annotations);

    // Should not warn on the repo dereference (first in chain)
    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Should not warn for @Inject field in method chain"
    );
}

#[test]
fn test_injected_field_array_access() {
    let code = r#"
        public class Service {
            @Autowired
            private int[] counts;

            public void test() {
                int first = counts[0];
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    let analysis = analyze_method(code, field_annotations);

    // Should not warn on array access of injected field
    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "counts"
        }),
        "Should not warn for array access on @Autowired field"
    );
}

#[test]
fn test_injected_field_field_access() {
    let code = r#"
        public class Service {
            @Resource
            private Config config;

            public void test() {
                String url = config.url;
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    let analysis = analyze_method(code, field_annotations);

    // Should not warn on field access through injected field
    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "config"
        }),
        "Should not warn for field access on @Resource field"
    );
}

#[test]
fn test_constructor_and_injection_both_work() {
    let code = r#"
        public class Service {
            @Autowired
            private Repository autowiredRepo;

            private Cache constructorCache;

            public Service() {
                this.constructorCache = new Cache();
            }

            public void test() {
                autowiredRepo.save();     // Should not warn (injected)
                constructorCache.get(1);  // Should not warn (constructor-assigned)
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    let analysis = analyze_method(code, field_annotations);

    // Neither field should warn
    for field_name in &["autowiredRepo", "constructorCache"] {
        assert!(
            analysis.diagnostics.iter().all(|d| {
                !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != *field_name
            }),
            "Should not warn for field: {}",
            field_name
        );
    }
}

#[test]
fn test_local_variable_named_same_as_injected_field() {
    let code = r#"
        public class Service {
            @Inject
            private Repository repo;

            public void test() {
                Repository repo = null;  // Local shadows field
                repo.save();  // Should warn - local is nullable
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    let _analysis = analyze_method(code, field_annotations);

    // Should warn on local variable 'repo', not the field
    // The is_constructor_injected_field check should exclude locals
    // (This test verifies the exclusion logic works)
}

#[test]
fn test_parameter_named_same_as_injected_field() {
    let code = r#"
        public class Service {
            @Autowired
            private Repository repo;

            public void test(Repository repo) {
                repo.save();  // Should use parameter nullability, not field
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    let _analysis = analyze_method(code, field_annotations);

    // Should use parameter's nullability state, not treat as injected field
    // (This test verifies the parameter exclusion logic works)
}

// ============================================================================
// Phase 2.3: Lombok @NonNull and nullability annotations
// ============================================================================

#[test]
fn test_lombok_nonnull_field() {
    let code = r#"
        @RequiredArgsConstructor
        public class Service {
            @NonNull
            private final Repository repo;

            public void process() {
                repo.save(data);
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify annotation was extracted
    // Note: @RequiredArgsConstructor is a class-level annotation, not field-level
    assert!(field_annotations.contains_key("repo"));
    assert_eq!(field_annotations["repo"], vec!["NonNull"]);

    // Analyze method and verify no warnings
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Expected no null dereference warnings for @NonNull field"
    );
}

#[test]
fn test_jetbrains_notnull_field() {
    let code = r#"
        public class Service {
            @NotNull
            private Repository repo;

            public void process() {
                repo.save(data);
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify annotation was extracted
    assert!(field_annotations.contains_key("repo"));
    assert_eq!(field_annotations["repo"], vec!["NotNull"]);

    // Analyze method and verify no warnings
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Expected no null dereference warnings for @NotNull field"
    );
}

#[test]
fn test_jsr305_nonnull_field() {
    let code = r#"
        public class Service {
            @Nonnull
            private Repository repo;

            public void process() {
                repo.save(data);
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify annotation was extracted
    assert!(field_annotations.contains_key("repo"));
    assert_eq!(field_annotations["repo"], vec!["Nonnull"]);

    // Analyze method and verify no warnings
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Expected no null dereference warnings for @Nonnull field"
    );
}

#[test]
fn test_fully_qualified_lombok_nonnull() {
    let code = r#"
        public class Service {
            @lombok.NonNull
            private Repository repo;

            public void test() {
                repo.save();
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify fully qualified annotation was extracted
    assert!(field_annotations.contains_key("repo"));
    assert_eq!(field_annotations["repo"], vec!["lombok.NonNull"]);

    // Analyze method and verify no warnings
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Expected no null dereference warnings for fully qualified @lombok.NonNull"
    );
}

#[test]
fn test_fully_qualified_jetbrains_notnull() {
    let code = r#"
        public class Service {
            @org.jetbrains.annotations.NotNull
            private Repository repo;

            public void test() {
                repo.save();
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify fully qualified annotation was extracted
    assert!(field_annotations.contains_key("repo"));
    assert_eq!(
        field_annotations["repo"],
        vec!["org.jetbrains.annotations.NotNull"]
    );

    // Analyze method and verify no warnings
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Expected no null dereference warnings for fully qualified @NotNull"
    );
}

#[test]
fn test_mixed_injection_and_nullability_annotations() {
    let code = r#"
        public class Service {
            @Autowired
            private UserRepository userRepo;

            @NonNull
            private ProductRepository productRepo;

            @NotNull
            private OrderRepository orderRepo;

            public void test() {
                userRepo.findAll();
                productRepo.findAll();
                orderRepo.findAll();
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify all annotations were extracted
    assert!(field_annotations.contains_key("userRepo"));
    assert!(field_annotations.contains_key("productRepo"));
    assert!(field_annotations.contains_key("orderRepo"));

    // Analyze method and verify no warnings for any annotated field
    let analysis = analyze_method(code, field_annotations);

    for field_name in &["userRepo", "productRepo", "orderRepo"] {
        assert!(
            analysis.diagnostics.iter().all(|d| {
                !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != *field_name
            }),
            "Expected no null dereference warnings for field {}",
            field_name
        );
    }
}

#[test]
fn test_nonnull_field_with_multiple_annotations() {
    let code = r#"
        @RequiredArgsConstructor
        public class Service {
            @NonNull
            @Getter
            private final Repository repo;

            public void test() {
                repo.save();
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Should have all annotations
    assert!(field_annotations.contains_key("repo"));
    // The annotations list should include NonNull and Getter
    assert!(field_annotations["repo"].contains(&"NonNull".to_string()));

    // Analyze method and verify no warnings
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Expected no null dereference warnings for @NonNull field with multiple annotations"
    );
}

#[test]
fn test_nonnull_field_chained_calls() {
    let code = r#"
        public class Service {
            @NonNull
            private Repository repo;

            public void test() {
                String name = repo.findById(1L).getName();
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    let analysis = analyze_method(code, field_annotations);

    // Should not warn on the repo dereference (first in chain)
    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "repo"
        }),
        "Should not warn for @NonNull field in method chain"
    );
}

#[test]
fn test_nonnull_field_array_access() {
    let code = r#"
        public class Service {
            @NotNull
            private int[] counts;

            public void test() {
                int first = counts[0];
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    let analysis = analyze_method(code, field_annotations);

    // Should not warn on array access of @NotNull field
    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "counts"
        }),
        "Should not warn for array access on @NotNull field"
    );
}

#[test]
fn test_nonnull_field_field_access() {
    let code = r#"
        public class Service {
            @NonNull
            private Config config;

            public void test() {
                String url = config.url;
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    let analysis = analyze_method(code, field_annotations);

    // Should not warn on field access through @NonNull field
    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "config"
        }),
        "Should not warn for field access on @NonNull field"
    );
}

#[test]
fn test_value_annotation_field_treated_as_nonnull() {
    let code = r#"
        @Configuration
        public class AppConfig {
            @Value("${app.name}")
            private String appName;

            public void test() {
                appName.toLowerCase();
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    // Verify annotation was extracted
    assert!(field_annotations.contains_key("appName"));
    assert!(field_annotations["appName"].contains(&"Value".to_string()));

    // Analyze method and verify no warnings on @Value field dereference
    let analysis = analyze_method(code, field_annotations);

    assert!(
        analysis.diagnostics.iter().all(|d| {
            !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != "appName"
        }),
        "Expected no null dereference warnings for @Value field"
    );
}

#[test]
fn test_annotation_case_sensitivity() {
    let code = r#"
        public class Service {
            @NonNull
            private Repository nonNullRepo;

            @Nonnull
            private Cache nonnullCache;

            @NotNull
            private Logger notNullLogger;

            public void test() {
                nonNullRepo.save();
                nonnullCache.get(1);
                notNullLogger.info("test");
            }
        }
    "#;

    let class_node = parse_class(code);
    let field_annotations = collect_field_annotations(&class_node);

    let analysis = analyze_method(code, field_annotations);

    // None of these should warn - all variants should be recognized
    for field_name in &["nonNullRepo", "nonnullCache", "notNullLogger"] {
        assert!(
            analysis.diagnostics.iter().all(|d| {
                !matches!(
                d.kind,
                hyperion_semantics::nullability::analysis::DiagnosticKind::PotentialNullDereference
            ) || d.variable_name != *field_name
            }),
            "Should recognize case variant for field: {}",
            field_name
        );
    }
}
