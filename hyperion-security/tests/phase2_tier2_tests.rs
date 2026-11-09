// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::float_cmp, clippy::needless_collect)]

//! Phase 2 (Tier 2) vulnerability detection tests
//!
//! Tests for the 5 medium-complexity pattern-based checks:
//! 1. Enhanced IDOR detection
//! 2. Race condition / TOCTOU detection
//! 3. Enhanced SSTI detection
//! 4. Enhanced XXE detection
//! 5. Insufficient logging detection

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
// IDOR Detection Tests
// ============================================================================

#[test]
fn test_idor_path_variable_without_authz() {
    let source = r#"
        @RestController
        public class UserController {
            @GetMapping("/user/{id}")
            public User getUser(@PathVariable Long id) {
                return userRepository.findById(id).orElse(null);
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert!(
        !idor_findings.is_empty(),
        "Should detect IDOR with @PathVariable"
    );
}

#[test]
fn test_idor_path_param_jax_rs() {
    let source = r#"
        @Path("/users")
        public class UserResource {
            @GET
            @Path("/{userId}")
            public User getUser(@PathParam("userId") String userId) {
                return userService.getById(userId);
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert!(
        !idor_findings.is_empty(),
        "Should detect IDOR with @PathParam"
    );
}

#[test]
fn test_idor_account_access() {
    let source = r#"
        @RestController
        public class AccountController {
            @GetMapping("/account/{accountId}")
            public Account getAccount(@PathVariable Long accountId) {
                return accountRepository.findById(accountId).get();
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert!(
        !idor_findings.is_empty(),
        "Should detect IDOR with accountId"
    );
}

#[test]
fn test_no_idor_with_preauthorize() {
    let source = r#"
        @RestController
        public class UserController {
            @GetMapping("/user/{id}")
            @PreAuthorize("@accessControl.canAccessUser(#id, authentication)")
            public User getUser(@PathVariable Long id) {
                return userRepository.findById(id).orElse(null);
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert!(
        idor_findings.is_empty(),
        "Should not flag IDOR when @PreAuthorize is present"
    );
}

#[test]
fn test_no_idor_with_ownership_check() {
    let source = r#"
        @RestController
        public class DocumentController {
            @GetMapping("/document/{id}")
            public Document getDocument(@PathVariable Long id, Authentication auth) {
                Document doc = documentRepository.findById(id).get();
                if (!checkOwnership(doc, auth)) {
                    throw new AccessDeniedException("Not authorized");
                }
                return doc;
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert!(
        idor_findings.is_empty(),
        "Should not flag IDOR when ownership check is present"
    );
}

// ============================================================================
// Phase 3.1: IDOR Service Layer Authorization Detection Tests
// ============================================================================

#[test]
fn test_idor_service_layer_delegation_low_severity() {
    // Service layer delegation - authorization may exist in service
    // Phase 3.2: Also verify confidence is 30%
    let source = r#"
        @RestController
        public class UserController {
            @Autowired
            private UserService userService;

            @GetMapping("/users/{userId}")
            public User getUser(@PathVariable Long userId) {
                return userService.getUser(userId);
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert_eq!(
        idor_findings.len(),
        1,
        "Should detect potential IDOR with service delegation"
    );
    assert_eq!(
        idor_findings[0].severity,
        hyperion_security::finding::Severity::Low,
        "Service layer delegation should result in LOW severity (authorization may exist in service)"
    );
    assert_eq!(
        idor_findings[0].confidence, 0.30,
        "Phase 3.2: Service layer delegation should have 30% confidence (low confidence)"
    );
    assert!(
        idor_findings[0].message.contains("IDOR Review Required"),
        "Phase 3.2: Message should indicate this is a review item, not a confirmed vulnerability"
    );
}

#[test]
fn test_idor_direct_repository_access_high_severity() {
    // Direct repository access from controller - HIGH risk
    // Phase 3.2: Also verify confidence is 85% (high confidence for direct access)
    let source = r#"
        @RestController
        public class UserController {
            @Autowired
            private UserRepository userRepository;

            @GetMapping("/users/{userId}")
            public User getUser(@PathVariable Long userId) {
                return userRepository.findById(userId).orElse(null);
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert_eq!(idor_findings.len(), 1, "Should detect IDOR");
    assert_eq!(
        idor_findings[0].severity,
        hyperion_security::finding::Severity::High,
        "Direct repository access should be HIGH severity"
    );
    assert_eq!(
        idor_findings[0].confidence, 0.85,
        "Phase 3.2: Direct repository access should have 85% confidence (high confidence)"
    );
}

#[test]
fn test_idor_suppressed_with_check_access() {
    // Controller calls checkAccess - should suppress
    let source = r#"
        @RestController
        public class DocumentController {
            @GetMapping("/documents/{documentId}")
            public Document getDocument(@PathVariable Long documentId, Authentication auth) {
                checkAccess(documentId, auth);
                return documentService.getDocument(documentId);
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert!(
        idor_findings.is_empty(),
        "Should suppress IDOR when checkAccess is present"
    );
}

#[test]
fn test_idor_suppressed_with_verify_ownership() {
    // Controller calls verifyOwnership - should suppress
    let source = r#"
        @RestController
        public class OrderController {
            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                verifyOwnership(orderId);
                return orderService.getOrder(orderId);
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert!(
        idor_findings.is_empty(),
        "Should suppress IDOR when verifyOwnership is present"
    );
}

#[test]
fn test_idor_suppressed_with_is_authorized() {
    // Controller calls isAuthorized - should suppress
    let source = r#"
        @RestController
        public class ProfileController {
            @GetMapping("/profiles/{profileId}")
            public Profile getProfile(@PathVariable Long profileId, Principal principal) {
                if (!isAuthorized(profileId, principal)) {
                    throw new AccessDeniedException();
                }
                return profileService.getProfile(profileId);
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert!(
        idor_findings.is_empty(),
        "Should suppress IDOR when isAuthorized is present"
    );
}

#[test]
fn test_idor_suppressed_with_has_permission() {
    // Controller calls hasPermission - should suppress
    let source = r#"
        @RestController
        public class AccountController {
            @GetMapping("/accounts/{accountId}")
            public Account getAccount(@PathVariable Long accountId) {
                if (!hasPermission(accountId, "READ")) {
                    throw new ForbiddenException();
                }
                return accountRepository.findById(accountId).get();
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert!(
        idor_findings.is_empty(),
        "Should suppress IDOR when hasPermission is present"
    );
}

#[test]
fn test_idor_service_with_find_method() {
    // Service layer with find method - LOW severity
    // Note: Using /orders instead of /products because /products is now classified as
    // a public resource endpoint (Phase 15) and is correctly suppressed
    let source = r#"
        @RestController
        public class OrderController {
            @Autowired
            private OrderService orderService;

            @GetMapping("/orders/{orderId}")
            public Order getOrder(@PathVariable Long orderId) {
                return orderService.findOrder(orderId);
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert_eq!(idor_findings.len(), 1, "Should detect potential IDOR");
    assert_eq!(
        idor_findings[0].severity,
        hyperion_security::finding::Severity::Low,
        "Service layer find method should be LOW severity"
    );
}

#[test]
fn test_idor_mixed_repository_and_service() {
    // Mixed pattern: service AND repository - repository takes precedence (HIGH)
    let source = r#"
        @RestController
        public class MixedController {
            @Autowired
            private UserService userService;

            @Autowired
            private UserRepository userRepository;

            @GetMapping("/users/{userId}")
            public User getUser(@PathVariable Long userId) {
                // Direct repository access
                return userRepository.findById(userId).orElse(null);
            }
        }
    "#;

    let findings = check_code(source);
    let idor_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Idor)
        .collect();

    assert_eq!(idor_findings.len(), 1, "Should detect IDOR");
    assert_eq!(
        idor_findings[0].severity,
        hyperion_security::finding::Severity::High,
        "Direct repository findById should override service presence and be HIGH severity"
    );
}

// ============================================================================
// Race Condition / TOCTOU Detection Tests
// ============================================================================

#[test]
fn test_race_file_exists_then_read() {
    let source = r#"
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

    let findings = check_code(source);
    let race_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .collect();

    assert!(
        !race_findings.is_empty(),
        "Should detect TOCTOU with file.exists() + FileInputStream"
    );
}

#[test]
fn test_race_balance_check_then_withdraw() {
    let source = r#"
        public class BankAccount {
            public void withdraw(double amount) {
                if (getBalance() >= amount) {
                    setBalance(getBalance() - amount);
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
        "Should detect race condition in balance check + withdraw"
    );
}

#[test]
fn test_no_race_with_synchronized() {
    let source = r#"
        public class BankAccount {
            public synchronized void withdraw(double amount) {
                if (getBalance() >= amount) {
                    setBalance(getBalance() - amount);
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
        "Should not flag race when synchronized is used"
    );
}

#[test]
fn test_no_race_with_transactional() {
    let source = r#"
        public class AccountService {
            @Transactional
            public void withdraw(Long accountId, double amount) {
                Account account = accountRepository.findById(accountId).get();
                if (account.getBalance() >= amount) {
                    account.withdraw(amount);
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
        "Should not flag race when @Transactional is used"
    );
}

#[test]
fn test_race_collection_check_then_act() {
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

    assert!(
        !race_findings.is_empty(),
        "Should detect race condition with isEmpty() check"
    );
}

// ============================================================================
// SSTI Detection Tests
// ============================================================================

#[test]
fn test_ssti_thymeleaf_dynamic_template() {
    let source = r##"
        @Controller
        public class TemplateController {
            @GetMapping("/render")
            public String render(@RequestParam String userInput, Model model) {
                String template = "<div th:text=\"${" + userInput + "}\"></div>";
                return templateEngine.process(template, context);
            }
        }
    "##;

    let findings = check_code(source);
    let ssti_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::TemplateInjection)
        .collect();

    assert!(
        !ssti_findings.is_empty(),
        "Should detect SSTI with Thymeleaf dynamic template"
    );
}

#[test]
fn test_ssti_freemarker_user_input() {
    let source = r#"
        public class ReportGenerator {
            public String generate(String userTemplate) {
                Template template = cfg.getTemplate(userTemplate);
                StringWriter writer = new StringWriter();
                template.process(dataModel, writer);
                return writer.toString();
            }
        }
    "#;

    let findings = check_code(source);
    let ssti_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::TemplateInjection)
        .collect();

    assert!(
        !ssti_findings.is_empty(),
        "Should detect SSTI with FreeMarker user template"
    );
}

#[test]
fn test_ssti_velocity_evaluate() {
    let source = r##"
        public class VelocityRenderer {
            public String render(String userInput) {
                VelocityContext context = new VelocityContext();
                String template = "#set($x = " + userInput + ")";
                StringWriter writer = new StringWriter();
                velocityEngine.evaluate(context, writer, "template", template);
                return writer.toString();
            }
        }
    "##;

    let findings = check_code(source);
    let ssti_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::TemplateInjection)
        .collect();

    assert!(
        !ssti_findings.is_empty(),
        "Should detect SSTI with Velocity evaluate"
    );
}

#[test]
fn test_el_injection() {
    let source = r##"
        public class ExpressionEvaluator {
            public Object evaluate(String userExpression) {
                ExpressionFactory factory = ExpressionFactory.newInstance();
                ValueExpression expr = factory.createValueExpression(
                    elContext, "${" + userExpression + "}", Object.class
                );
                return expr.getValue(elContext);
            }
        }
    "##;

    let findings = check_code(source);
    let el_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::ExpressionLanguageInjection)
        .collect();

    assert!(
        !el_findings.is_empty(),
        "Should detect Expression Language injection"
    );
}

// ============================================================================
// Enhanced XXE Detection Tests
// ============================================================================

#[test]
fn test_xxe_document_builder_no_hardening() {
    let source = r#"
        public class XmlParser {
            public Document parse(InputStream xml) throws Exception {
                DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();
                DocumentBuilder builder = factory.newDocumentBuilder();
                return builder.parse(xml);
            }
        }
    "#;

    let findings = check_code(source);
    let xxe_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::XxeInjection)
        .collect();

    assert!(
        !xxe_findings.is_empty(),
        "Should detect XXE with unsecured DocumentBuilderFactory"
    );
}

#[test]
fn test_no_xxe_with_hardening() {
    let source = r#"
        public class SecureXmlParser {
            public Document parse(InputStream xml) throws Exception {
                DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();
                factory.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true);
                factory.setFeature("http://xml.org/sax/features/external-general-entities", false);
                DocumentBuilder builder = factory.newDocumentBuilder();
                return builder.parse(xml);
            }
        }
    "#;

    let findings = check_code(source);
    let xxe_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::XxeInjection)
        .collect();

    assert!(
        xxe_findings.is_empty(),
        "Should not flag XXE when features are properly set"
    );
}

#[test]
fn test_xpath_injection() {
    let source = r#"
        public class XPathEvaluator {
            public NodeList search(String userInput) throws Exception {
                XPath xpath = XPathFactory.newInstance().newXPath();
                String expression = "/users/user[@name='" + userInput + "']";
                return (NodeList) xpath.evaluate(expression, document, XPathConstants.NODESET);
            }
        }
    "#;

    let findings = check_code(source);
    let xxe_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::XxeInjection)
        .collect();

    assert!(
        !xxe_findings.is_empty(),
        "Should detect XPath injection with user input"
    );
}

#[test]
fn test_xslt_injection() {
    let source = r#"
        public class XsltTransformer {
            public String transform(String userStylesheet, Document doc) throws Exception {
                TransformerFactory factory = TransformerFactory.newInstance();
                StreamSource xslt = new StreamSource(new StringReader(userStylesheet));
                Transformer transformer = factory.newTransformer(xslt);
                StringWriter writer = new StringWriter();
                transformer.transform(new DOMSource(doc), new StreamResult(writer));
                return writer.toString();
            }
        }
    "#;

    let findings = check_code(source);
    let xxe_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::XxeInjection)
        .collect();

    assert!(
        !xxe_findings.is_empty(),
        "Should detect XSLT injection with user stylesheet"
    );
}

#[test]
fn test_sax_parser_no_hardening() {
    let source = r#"
        public class SaxXmlParser {
            public void parse(InputStream xml) throws Exception {
                SAXParserFactory factory = SAXParserFactory.newInstance();
                SAXParser parser = factory.newSAXParser();
                parser.parse(xml, handler);
            }
        }
    "#;

    let findings = check_code(source);
    let xxe_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::XxeInjection)
        .collect();

    assert!(
        !xxe_findings.is_empty(),
        "Should detect XXE with unsecured SAXParserFactory"
    );
}

// ============================================================================
// Insufficient Logging Detection Tests
// ============================================================================

#[test]
fn test_insufficient_logging_auth_method() {
    let source = r#"
        public class AuthController {
            @PostMapping("/login")
            public ResponseEntity<String> login(@RequestBody LoginRequest request) {
                boolean success = authenticationManager.authenticate(request.getUsername(), request.getPassword());
                if (success) {
                    return ResponseEntity.ok(generateToken());
                }
                throw new AuthenticationException("Invalid credentials");
            }
        }
    "#;

    let findings = check_code(source);
    let logging_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::InsufficientLogging)
        .collect();

    assert!(
        !logging_findings.is_empty(),
        "Should detect missing logging in authentication"
    );
}

#[test]
fn test_no_insufficient_logging_with_logger() {
    let source = r#"
        public class AuthController {
            @PostMapping("/login")
            public ResponseEntity<String> login(@RequestBody LoginRequest request) {
                logger.info("Login attempt for user: {}", request.getUsername());
                boolean success = authenticationManager.authenticate(request.getUsername(), request.getPassword());
                if (success) {
                    logger.info("Login successful for user: {}", request.getUsername());
                    return ResponseEntity.ok(generateToken());
                }
                logger.warn("Login failed for user: {}", request.getUsername());
                throw new AuthenticationException("Invalid credentials");
            }
        }
    "#;

    let findings = check_code(source);
    let logging_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::InsufficientLogging)
        .collect();

    assert!(
        logging_findings.is_empty(),
        "Should not flag when logging is present"
    );
}

#[test]
fn test_insufficient_logging_delete_operation() {
    let source = r#"
        public class UserController {
            @DeleteMapping("/user/{id}")
            public ResponseEntity<Void> deleteUser(@PathVariable Long id) {
                userRepository.deleteById(id);
                return ResponseEntity.noContent().build();
            }
        }
    "#;

    let findings = check_code(source);
    let logging_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::InsufficientLogging)
        .collect();

    assert!(
        !logging_findings.is_empty(),
        "Should detect missing audit logging for delete operation"
    );
}

#[test]
fn test_insufficient_logging_security_exception() {
    let source = r#"
        public class GlobalExceptionHandler {
            @ExceptionHandler(AccessDeniedException.class)
            public ResponseEntity<String> handleAccessDenied(AccessDeniedException e) {
                return ResponseEntity.status(403).body("Access denied");
            }
        }
    "#;

    let findings = check_code(source);
    let logging_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::InsufficientLogging)
        .collect();

    assert!(
        !logging_findings.is_empty(),
        "Should detect missing logging in security exception handler"
    );
}

#[test]
fn test_insufficient_logging_authorization_check() {
    let source = r#"
        public class AccountController {
            @PutMapping("/account/{id}")
            @PreAuthorize("hasRole('ADMIN')")
            public Account updateAccount(@PathVariable Long id, @RequestBody Account updates) {
                Account account = accountRepository.findById(id).get();
                account.setBalance(updates.getBalance());
                return accountRepository.save(account);
            }
        }
    "#;

    let findings = check_code(source);
    let logging_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::InsufficientLogging)
        .collect();

    assert!(
        !logging_findings.is_empty(),
        "Should detect missing audit logging for authorized sensitive operation"
    );
}
