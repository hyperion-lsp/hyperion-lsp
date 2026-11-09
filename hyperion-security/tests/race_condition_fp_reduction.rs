// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::float_cmp, clippy::needless_collect)]

//! Race condition false positive reduction tests
//!
//! Tests for Phase 1 false positive reduction in race condition check.
//! Verifies that local collections and database query results are not flagged.

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
// Local Collection - Should NOT Flag (False Positive Reduction)
// ============================================================================

#[test]
fn test_local_arraylist_no_race_condition() {
    let source = r#"
        public class TaskProcessor {
            public void process() {
                List<String> items = new ArrayList<>();
                items.add("test");
                if (!items.isEmpty()) {
                    String first = items.get(0);
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
        "Should NOT flag local ArrayList as race condition (false positive reduction)"
    );
}

#[test]
fn test_local_hashmap_no_race_condition() {
    let source = r#"
        public class DataHandler {
            public void handle() {
                Map<String, String> data = new HashMap<>();
                data.put("key", "value");
                if (!data.isEmpty()) {
                    String value = data.get("key");
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
        "Should NOT flag local HashMap as race condition"
    );
}

#[test]
fn test_list_of_no_race_condition() {
    let source = r#"
        public class DataProcessor {
            public void process() {
                List<String> items = List.of("a", "b", "c");
                if (!items.isEmpty()) {
                    String first = items.get(0);
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
        "Should NOT flag List.of() as race condition"
    );
}

#[test]
fn test_stream_collect_no_race_condition() {
    let source = r#"
        public class StreamProcessor {
            public void process(List<String> source) {
                List<String> filtered = source.stream()
                    .filter(s -> s.length() > 0)
                    .collect(Collectors.toList());
                if (!filtered.isEmpty()) {
                    String first = filtered.get(0);
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
        "Should NOT flag stream().collect() as race condition"
    );
}

// ============================================================================
// Database Query Results - Should NOT Flag (False Positive Reduction)
// ============================================================================

#[test]
fn test_repository_find_no_race_condition() {
    let source = r#"
        public class UserService {
            @Autowired
            private UserRepository userRepository;

            public void processUsers() {
                List<User> users = userRepository.findAll();
                if (!users.isEmpty()) {
                    User first = users.get(0);
                    process(first);
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
        "Should NOT flag repository.find results as race condition"
    );
}

#[test]
fn test_dao_find_no_race_condition() {
    let source = r#"
        public class OrderService {
            private OrderDao orderDao;

            public void processOrders() {
                List<Order> orders = orderDao.findByStatus("PENDING");
                if (!orders.isEmpty()) {
                    Order first = orders.get(0);
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
        "Should NOT flag dao.find results as race condition"
    );
}

#[test]
fn test_entity_manager_no_race_condition() {
    let source = r#"
        public class ProductService {
            @PersistenceContext
            private EntityManager entityManager;

            public void processProducts() {
                List<Product> products = entityManager
                    .createQuery("SELECT p FROM Product p", Product.class)
                    .getResultList();
                if (!products.isEmpty()) {
                    Product first = products.get(0);
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
        "Should NOT flag entityManager query results as race condition"
    );
}

#[test]
fn test_jdbc_template_no_race_condition() {
    let source = r#"
        public class ReportService {
            @Autowired
            private JdbcTemplate jdbcTemplate;

            public void generateReport() {
                List<Report> reports = jdbcTemplate.query(
                    "SELECT * FROM reports",
                    new ReportRowMapper()
                );
                if (!reports.isEmpty()) {
                    Report first = reports.get(0);
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
        "Should NOT flag jdbcTemplate.query results as race condition"
    );
}

// ============================================================================
// Immutable Collections - Should NOT Flag (False Positive Reduction)
// ============================================================================

#[test]
fn test_unmodifiable_list_no_race_condition() {
    let source = r#"
        public class ConfigService {
            public void loadConfig() {
                List<String> config = Collections.unmodifiableList(Arrays.asList("a", "b"));
                if (!config.isEmpty()) {
                    String first = config.get(0);
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
        "Should NOT flag Collections.unmodifiable as race condition"
    );
}

#[test]
fn test_empty_list_no_race_condition() {
    let source = r#"
        public class EmptyHandler {
            public void handle() {
                List<String> items = Collections.emptyList();
                if (!items.isEmpty()) {
                    String first = items.get(0);
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
        "Should NOT flag Collections.empty as race condition"
    );
}

#[test]
fn test_singleton_list_no_race_condition() {
    let source = r#"
        public class SingletonHandler {
            public void handle(String value) {
                List<String> items = Collections.singletonList(value);
                if (!items.isEmpty()) {
                    String first = items.get(0);
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
        "Should NOT flag Collections.singleton as race condition"
    );
}

// ============================================================================
// Shared State - Known Limitation (Trade-off for FP Reduction)
// ============================================================================

#[test]
fn test_class_field_still_detected() {
    // This test verifies that class field race conditions ARE still detected.
    // The method text does NOT include the field declaration (it's in a different scope),
    // so our helper won't see "new ArrayList" and won't suppress the finding.
    // This is GOOD - we want to flag shared class fields!
    let source = r#"
        public class QueueProcessor {
            private final List<Task> tasks = new ArrayList<>();

            public void processNext() {
                if (!tasks.isEmpty()) {
                    Task task = tasks.get(0);
                    tasks.remove(0);
                    process(task);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    // This SHOULD be flagged because:
    // 1. The method contains check-then-act pattern (isEmpty + get/remove)
    // 2. The method text does NOT contain "new ArrayList" (that's in class scope)
    // 3. Therefore our helper won't suppress it
    // 4. No synchronization is present
    assert!(
        !race_findings.is_empty(),
        "Should still flag race condition on class fields (true positive preserved)"
    );
}

#[test]
fn test_shared_field_without_initialization_still_detectable() {
    // When a class field is NOT initialized with a local pattern,
    // it can still be detected (though this requires the collection
    // to already exist as a field).
    let source = r#"
        public class QueueProcessor {
            private final List<Task> tasks;

            public QueueProcessor(List<Task> tasks) {
                this.tasks = tasks;
            }

            public void processNext() {
                if (!tasks.isEmpty()) {
                    Task task = tasks.get(0);
                    tasks.remove(0);
                    process(task);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    // This SHOULD be flagged because there's no local collection pattern
    assert!(
        !race_findings.is_empty(),
        "Should flag race condition when field is not initialized with local pattern"
    );
}

// ============================================================================
// Option 2 Bonus Improvements - Stream API and @Transactional
// ============================================================================

#[test]
fn test_stream_api_no_race_condition() {
    let source = r#"
        public class UserService {
            public void processUsers(@PathVariable Long userId) {
                List<User> users = userRepository.findAll();
                if (users.stream().anyMatch(u -> u.getId().equals(userId))) {
                    User user = users.stream()
                        .filter(u -> u.getId().equals(userId))
                        .findFirst()
                        .orElse(null);
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
        "Should NOT flag stream API usage as race condition (functional style)"
    );
}

#[test]
fn test_parallel_stream_no_race_condition() {
    let source = r#"
        public class DataProcessor {
            public void process() {
                List<Data> data = loadData();
                if (!data.isEmpty()) {
                    data.parallelStream()
                        .forEach(d -> process(d));
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
        "Should NOT flag parallel stream as race condition"
    );
}

#[test]
fn test_transactional_annotation_no_race_condition() {
    let source = r#"
        public class BalanceService {
            @Transactional
            public void updateBalance(Long userId, BigDecimal amount) {
                User user = userRepository.findById(userId).orElseThrow();
                if (user.getBalance().compareTo(amount) >= 0) {
                    user.setBalance(user.getBalance().subtract(amount));
                    userRepository.save(user);
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
        "Should NOT flag @Transactional methods as race condition (DB isolation)"
    );
}

// ============================================================================
// Verify Confidence is Set to 0.25
// ============================================================================

#[test]
fn test_confidence_is_25_percent() {
    // Create a scenario that WILL trigger the race condition check
    // (shared field without synchronization)
    let source = r#"
        public class QueueProcessor {
            private final List<Task> tasks;

            public QueueProcessor(List<Task> tasks) {
                this.tasks = tasks;
            }

            public void processNext() {
                if (!tasks.isEmpty()) {
                    Task task = tasks.get(0);
                    tasks.remove(0);
                    process(task);
                }
            }
        }
    "#;

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(!race_findings.is_empty(), "Should find race condition");

    // Verify confidence is set to 0.25 (25%)
    for finding in race_findings {
        assert_eq!(
            finding.confidence, 0.25,
            "Race condition confidence should be 0.25 (25%), got {}",
            finding.confidence
        );
    }
}
