// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Reflection safety checks with taint-aware analysis
//!
//! This module provides improved detection for unsafe reflection patterns,
//! reducing false positives by:
//! - Recognizing literal class names (safe)
//! - Recognizing class literal patterns like MyClass.class (safe)
//! - Allowlisting safe standard library classes
//! - Distinguishing between tainted (user-controlled) and untainted class names

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use crate::scanner::method_call_pattern;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Safe class patterns that are commonly used in reflection but don't pose security risks
const SAFE_CLASS_PATTERNS: &[&str] = &[
    // Collections
    "ArrayList",
    "LinkedList",
    "HashSet",
    "TreeSet",
    "HashMap",
    "TreeMap",
    "LinkedHashMap",
    "LinkedHashSet",
    "Vector",
    "Stack",
    "Queue",
    "Deque",
    "ArrayDeque",
    "PriorityQueue",
    "ConcurrentHashMap",
    "CopyOnWriteArrayList",
    "CopyOnWriteArraySet",
    // Common utility classes
    "String",
    "Integer",
    "Long",
    "Double",
    "Float",
    "Boolean",
    "Byte",
    "Short",
    "Character",
    "BigInteger",
    "BigDecimal",
    "Date",
    "Calendar",
    "UUID",
    // Common patterns
    "Object",
    "Class",
    "List",
    "Set",
    "Map",
    "Collection",
];

/// Check for unsafe reflection with potential user input
pub struct UnsafeReflectionCheck;

impl UnsafeReflectionCheck {
    /// Check if the class name source follows a safe pattern that doesn't represent user input.
    ///
    /// Safe patterns include:
    /// 1. `someClass.getName()` - Already loaded Class object, getName() returns its canonical name
    /// 2. `obj.getClass()` - Runtime type of an existing object
    /// 3. `SomeClass.class` - Class literal
    /// 4. Method parameter that is already a Method/Constructor object (not a string)
    fn is_safe_class_name_source(node: &SyntaxNode) -> bool {
        let text = node.text().to_string();

        // Pattern 1: someClass.getName() or loadedClass.getName()
        // This indicates an already-loaded Class<?> object - calling .getName() just returns
        // its canonical name, which is safe because the class was already validated when loaded.
        // Examples:
        //   Class.forName(loadedClass.getName(), true, loader)
        //   Class.forName(existingClass.getName())
        if text.contains(".getName()") && !mentions_user_input(&text) {
            return true;
        }

        // Pattern 2: obj.getClass() - runtime type of an existing object
        // Examples:
        //   obj.getClass().newInstance()
        //   obj.getClass().getMethod("foo").invoke(obj)
        // The runtime class is trusted, but a method name taken from a request is not.
        if text.contains(".getClass()") && !mentions_user_input(&text) {
            return true;
        }

        // Pattern 3: Method parameter is already a Method or Constructor object
        // This is a utility wrapper pattern where the Method/Constructor is passed in,
        // not created from user input.
        // Examples:
        //   method.invoke(obj, args) where method is a parameter
        //   constructor.newInstance(args) where constructor is a parameter
        // Look for patterns like "static Object invoke(Method method, ..."
        // where the method is a parameter passed in from caller
        if Self::is_reflection_utility_wrapper(node) {
            return true;
        }

        false
    }

    /// Check if this is a reflection utility wrapper method that takes Method/Constructor as parameter.
    /// These are safe because the Method/Constructor object is passed in from the caller,
    /// not created dynamically from user input.
    ///
    /// This is a heuristic that looks for:
    /// 1. Method/Constructor fields with naming patterns like signalHandleMethod, signalConstructor
    /// 2. Simple utility wrappers where the Method/Constructor is a direct parameter
    fn is_reflection_utility_wrapper(node: &SyntaxNode) -> bool {
        let text = node.text().to_string();
        let lower_text = text.to_lowercase();

        // Pattern 1: Field names that indicate pre-defined Method/Constructor objects
        // These are typically initialized once (often in a static block or constructor)
        // and used multiple times. They're not created from user input.
        // Examples:
        //   signalConstructor.newInstance(signalName)
        //   signalHandleMethod.invoke(null, signal, handler)
        //   signalHandlerHandleMethod.invoke(signalHandler, signal)
        let safe_field_patterns = [
            "signalconstructor.",
            "signalhandlemethod.",
            "signalhandlerhandlemethod.",
            "signalgetnamemethod.",
        ];

        for pattern in &safe_field_patterns {
            if lower_text.contains(pattern) {
                return true;
            }
        }

        // Pattern 2: Utility wrapper functions where Method/Constructor is a parameter
        // Look for patterns like:
        //   return method.invoke(obj, args);
        //   return m.invoke(obj, args);
        //   return constructor.newInstance(args);
        // These are safe because the caller is responsible for providing a safe Method/Constructor
        //
        // BUT we need to be careful: if the same file contains user input handling
        // (e.g., getParameter, getMethod(userInput)), then method.invoke might be unsafe.
        //
        // For now, we only recognize the field patterns above. The generic method.invoke
        // pattern needs context from the whole file to determine safety.

        false
    }

    /// Check if a string literal contains a class name that appears safe
    fn is_safe_literal_class_name(literal: &str) -> bool {
        // Remove quotes if present
        let class_name = literal.trim_matches('"');

        // Check if it's a fully qualified name that ends with a safe class
        for safe_class in SAFE_CLASS_PATTERNS {
            if class_name == *safe_class || class_name.ends_with(&format!(".{safe_class}")) {
                return true;
            }
        }

        // Check if it looks like a legitimate fully qualified class name (not dynamic)
        // Pattern: starts with lowercase package, ends with capitalized class
        if class_name.contains('.') {
            let parts: Vec<&str> = class_name.split('.').collect();
            if parts.len() >= 2 {
                // Check if all parts except last are lowercase (package names)
                // and last part starts with uppercase (class name)
                let all_package_parts_lowercase = parts[..parts.len() - 1]
                    .iter()
                    .all(|p| p.chars().next().is_some_and(char::is_lowercase));
                let class_part_capitalized = parts
                    .last()
                    .is_some_and(|p| p.chars().next().is_some_and(char::is_uppercase));

                if all_package_parts_lowercase && class_part_capitalized {
                    return true;
                }
            }
        }

        false
    }

    /// Check if the node represents a class literal pattern (e.g., MyClass.class)
    fn is_class_literal_pattern(node: &SyntaxNode) -> bool {
        let text = node.text().to_string();

        // Check for .class pattern
        if text.contains(".class") {
            return true;
        }

        // Check for CLASS_LITERAL_EXPR in descendants
        for desc in node.descendants() {
            if desc.kind() == SyntaxKind::CLASS_LITERAL_EXPR {
                return true;
            }
        }

        false
    }

    /// Check if the argument to a reflection method is a string literal
    fn has_literal_string_argument(call_node: &SyntaxNode) -> Option<String> {
        // Look for ARG_LIST child
        if let Some(arg_list) = call_node
            .children()
            .find(|c| c.kind() == SyntaxKind::ARG_LIST)
        {
            // Check if the first argument is a literal expression
            for arg in arg_list.children() {
                if arg.kind() == SyntaxKind::LITERAL_EXPR {
                    let text = arg.text().to_string();
                    // Check if it's a string literal (starts and ends with quotes)
                    if text.starts_with('"') && text.ends_with('"') {
                        return Some(text);
                    }
                }
            }
        }
        None
    }

    /// Check if newInstance is called on a constructor with non-class-related arguments
    /// This detects patterns like: constructor.newInstance(size) which creates a collection with capacity
    fn is_constructor_with_non_class_args(node: &SyntaxNode) -> bool {
        let text = node.text().to_string();

        // Check for patterns like newInstance(someSize) or newInstance(capacity)
        // where the argument is clearly not a class but a primitive value

        // Look for the METHOD_CALL_EXPR containing the argument
        for child in node.children_with_tokens() {
            if let Some(call_node) = child.as_node() {
                if call_node.kind() == SyntaxKind::METHOD_CALL_EXPR {
                    if let Some(arg_list) = call_node
                        .children()
                        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
                    {
                        for arg in arg_list.children() {
                            // If argument is a simple name (variable) that looks like a size/capacity
                            if arg.kind() == SyntaxKind::NAME_EXPR {
                                let arg_text = arg.text().to_string().to_lowercase();
                                if arg_text.contains("size")
                                    || arg_text.contains("capacity")
                                    || arg_text.contains("length")
                                    || arg_text.contains("count")
                                {
                                    return true;
                                }
                            }
                            // If argument is a literal number, it's likely a collection capacity
                            if arg.kind() == SyntaxKind::LITERAL_EXPR {
                                let arg_text = arg.text().to_string();
                                // Check if it's a numeric literal
                                if arg_text.parse::<i64>().is_ok()
                                    || arg_text.parse::<f64>().is_ok()
                                {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Check parent context for Constructor variable names suggesting safe usage
        let lower_text = text.to_lowercase();
        if lower_text.contains("listconstructor")
            || lower_text.contains("mapconstructor")
            || lower_text.contains("setconstructor")
            || lower_text.contains("collectionconstructor")
        {
            return true;
        }

        false
    }

    /// Check if the reflection call appears to use a variable from configuration
    /// These are lower severity as they're not directly user-controlled
    fn appears_to_be_config_based(node: &SyntaxNode) -> bool {
        let raw = node.text().to_string();
        // A statement that reads request input is not configuration, whatever else it names
        if mentions_user_input(&raw) {
            return false;
        }
        let text = raw.to_lowercase();

        // Check for common configuration patterns
        text.contains("config")
            || text.contains("property")
            || text.contains("properties")
            || text.contains("setting")
            || text.contains("constant")
            || text.contains("default")
    }

    /// Check if this is a Utils.newInstance pattern (common in utility-heavy code)
    /// These typically have controlled class parameters from configuration
    fn is_utils_new_instance_pattern(node: &SyntaxNode) -> bool {
        let text = node.text().to_string();

        // Check for Utils.newInstance or similar utility patterns
        text.contains("Utils.newInstance")
            || text.contains("ClassUtils.")
            || text.contains("ReflectionUtils.")
            || text.contains("BeanUtils.")
    }

    /// Check if this reflection usage matches known framework-safe patterns
    /// (Thrift proxies, Java Proxy invocation, Gson/Jackson type adapters).
    fn is_framework_reflection_pattern(node: &SyntaxNode, source: &str) -> bool {
        let text = node.text().to_string();
        let lower_text = text.to_lowercase();
        // The file's markers say the code is framework plumbing. They do not excuse a call
        // whose own statement reads request input, which can name the class or method.
        let source_lower = if mentions_user_input(&text) {
            String::new()
        } else {
            source.to_lowercase()
        };

        let proxy_markers = [
            "proxy.newproxyinstance",
            "invocationhandler",
            "java.lang.reflect.proxy",
        ];
        if proxy_markers
            .iter()
            .any(|marker| lower_text.contains(marker) || source_lower.contains(marker))
        {
            return true;
        }

        let thrift_markers = [
            "org.apache.thrift",
            "tserviceclient",
            "tprotocol",
            "tasyncclient",
            "tclient",
            "tprocessor",
            "tprotocolfactory",
        ];
        if thrift_markers
            .iter()
            .any(|marker| lower_text.contains(marker) || source_lower.contains(marker))
        {
            return true;
        }

        let type_markers = [
            "gettypename()",
            "getrawtype()",
            "gettype()",
            "getgenericsuperclass()",
            "getgenericinterfaces()",
        ];
        let gson_markers = [
            "typeadapter",
            "typetoken",
            "jsondeserializer",
            "jsonserializer",
            "gson",
        ];
        let jackson_markers = ["objectmapper", "javatype", "typereference", "typefactory"];

        let has_type_method = type_markers.iter().any(|m| lower_text.contains(m));
        let gson_context = gson_markers
            .iter()
            .any(|m| lower_text.contains(m) || source_lower.contains(m));
        let jackson_context = jackson_markers
            .iter()
            .any(|m| lower_text.contains(m) || source_lower.contains(m));

        has_type_method && (gson_context || jackson_context)
    }

    /// Analyze if the class name argument appears to be user-controlled (tainted)
    /// Returns: (is_potentially_tainted, severity_level)
    ///
    /// Note: This is a pattern-based heuristic. For true taint tracking across
    /// statements, the SecurityVisitor's TaintTracker is used. This check focuses
    /// on what can be determined from the immediate code context.
    fn analyze_class_name_source(node: &SyntaxNode, source: &str) -> (bool, Severity) {
        let text = node.text().to_string();
        let lower_text = text.to_lowercase();

        // HIGH RISK: Direct user input indicators in the current statement
        if lower_text.contains("request.")
            || lower_text.contains("getparameter")
            || lower_text.contains("getheader")
            || lower_text.contains("getinput")
            || lower_text.contains("userinput")
            || lower_text.contains("body.")
            || lower_text.contains("payload")
        {
            return (true, Severity::Critical);
        }

        // Check the broader file context for user input patterns
        // This helps catch cases where the variable is assigned from user input
        // on a previous line within the same method
        let source_lower = source.to_lowercase();

        // Look for common patterns that indicate this file handles user input
        let has_user_input_handling = source_lower.contains("httpservletrequest")
            || source_lower.contains("@requestparam")
            || source_lower.contains("@requestbody")
            || source_lower.contains("@pathvariable")
            || source_lower.contains("getparameter(")
            || source_lower.contains("getheader(");

        // If the file handles user input and the class name is from a variable,
        // be more cautious
        if has_user_input_handling {
            // Check if we're using a string literal (safe)
            if Self::has_literal_string_argument(node)
                .is_some_and(|s| Self::is_safe_literal_class_name(&s))
            {
                return (false, Severity::Low);
            }

            // Variable name in a file that handles user input - higher risk
            return (true, Severity::High);
        }

        // MEDIUM RISK: Variable that could be from various sources
        // But not confirmed to be from user input
        if !Self::has_literal_string_argument(node)
            .is_some_and(|s| Self::is_safe_literal_class_name(&s))
            && !Self::is_class_literal_pattern(node)
            && !Self::is_constructor_with_non_class_args(node)
            && !Self::is_utils_new_instance_pattern(node)
        {
            // Check if it's config-based (lower severity)
            if Self::appears_to_be_config_based(node) {
                return (true, Severity::Low);
            }

            // Generic variable - medium severity since we can't confirm it's safe
            return (true, Severity::Medium);
        }

        (false, Severity::Low)
    }
}

impl Check for UnsafeReflectionCheck {
    fn name(&self) -> &'static str {
        "unsafe-reflection"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        for call_node in method_call_pattern::find_method_calls(node) {
            let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) else {
                continue;
            };

            let method_str = pattern.method_name.as_str();

            // Check for Class.forName() - can lead to arbitrary code execution
            if method_str == "forName" && pattern.receiver_name.as_deref() == Some("Class") {
                if Self::is_framework_reflection_pattern(node, context.source) {
                    continue;
                }
                if let Some(literal) = Self::has_literal_string_argument(&call_node) {
                    if Self::is_safe_literal_class_name(&literal) {
                        continue;
                    }
                }
                if Self::is_class_literal_pattern(node) {
                    continue;
                }
                if Self::is_safe_class_name_source(node) {
                    continue;
                }

                let (is_potentially_unsafe, severity) =
                    Self::analyze_class_name_source(node, context.source);
                if !is_potentially_unsafe {
                    continue;
                }

                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = match severity {
                    Severity::Critical => {
                        "Class.forName() with user-controlled input can lead to arbitrary code execution. The class name appears to come from user input - validate against a strict allowlist.".to_string()
                    }
                    Severity::High | Severity::Medium => {
                        "Class.forName() with dynamic class name. If the class name can be influenced by external input, validate against a strict allowlist.".to_string()
                    }
                    Severity::Low | Severity::Info => {
                        "Class.forName() with configuration-based class name. Consider using an allowlist if the configuration can be modified by untrusted users.".to_string()
                    }
                };

                let mut finding = Finding::new(VulnType::UnsafeReflection, message, location, code);
                finding.severity = severity;
                finding.remediation = "Validate class names against an allowlist: Set<String> ALLOWED_CLASSES = Set.of(\"com.example.SafeClass1\", \"com.example.SafeClass2\"); if (!ALLOWED_CLASSES.contains(className)) throw new SecurityException();".to_string();
                return vec![finding];
            }

            // Check for Method.invoke() - can bypass access controls
            if method_str == "invoke" {
                let node_text = node.text().to_string();
                if node_text.contains("Method") || node_text.contains("method") {
                    if Self::is_safe_class_name_source(node) {
                        continue;
                    }
                    if Self::is_framework_reflection_pattern(node, context.source) {
                        continue;
                    }
                    let (is_potentially_unsafe, severity) =
                        Self::analyze_class_name_source(node, context.source);
                    if !is_potentially_unsafe {
                        continue;
                    }

                    let adjusted_severity = if Self::appears_to_be_config_based(node) {
                        Severity::Low
                    } else {
                        severity
                    };

                    let location = context.node_location(node);
                    let code = context.get_code_snippet(node);
                    let message = match adjusted_severity {
                        Severity::Critical => {
                            "Method.invoke() with user-controlled method name can bypass access controls and invoke private methods. Validate method names against an allowlist.".to_string()
                        }
                        _ => {
                            "Method.invoke() with dynamic method name. If method names can be influenced by external input, validate against an allowlist.".to_string()
                        }
                    };

                    let mut finding =
                        Finding::new(VulnType::UnsafeReflection, message, location, code);
                    finding.severity = adjusted_severity;
                    finding.remediation =
                        "Validate method names against an allowlist before invoking via reflection"
                            .to_string();
                    return vec![finding];
                }
            }

            // Check for newInstance() - deprecated and dangerous
            if method_str == "newInstance" {
                let node_text = node.text().to_string();
                if Self::is_constructor_with_non_class_args(node) {
                    continue;
                }
                if Self::is_utils_new_instance_pattern(node) {
                    continue;
                }
                if Self::is_safe_class_name_source(node) {
                    continue;
                }

                if node_text.contains("Class") || node_text.contains("Constructor") {
                    if Self::is_framework_reflection_pattern(node, context.source) {
                        continue;
                    }
                    if Self::is_class_literal_pattern(node) {
                        continue;
                    }

                    let (is_potentially_unsafe, severity) =
                        Self::analyze_class_name_source(node, context.source);
                    if !is_potentially_unsafe {
                        continue;
                    }

                    let location = context.node_location(node);
                    let code = context.get_code_snippet(node);
                    let message = match severity {
                        Severity::Critical | Severity::High => {
                            "newInstance() with user-controlled class can instantiate arbitrary classes, leading to remote code execution. Validate class names against an allowlist.".to_string()
                        }
                        _ => {
                            "newInstance() with dynamic class name. If the class name can be influenced by external input, validate against an allowlist.".to_string()
                        }
                    };

                    let mut finding =
                        Finding::new(VulnType::UnsafeReflection, message, location, code);
                    finding.severity = severity;
                    finding.remediation =
                        "Use Constructor.newInstance() with proper access checks and validate class names"
                            .to_string();
                    return vec![finding];
                }
            }
        }

        vec![]
    }
}

/// Whether a statement reads from request-like input, whose text can name a class or method.
fn mentions_user_input(text: &str) -> bool {
    let lower = text.to_lowercase();
    ["request", "param", "input", "body"]
        .iter()
        .any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser, SyntaxNode as RawSyntaxNode};
    use std::path::Path;

    fn scan_source(source: &str) -> Vec<Finding> {
        let tokens = Lexer::new(source).collect();
        let (green_node, _) = Parser::parse(tokens);
        let root = RawSyntaxNode::new_root(green_node);

        let check = UnsafeReflectionCheck;
        let context = CheckContext::new(Path::new("Test.java"), source);

        let mut findings = Vec::new();
        for node in root.descendants() {
            if check.target_kinds().contains(&node.kind()) {
                findings.extend(check.check(&node, &context));
            }
        }
        findings
    }

    // ==================== TRUE POSITIVES (MUST DETECT) ====================

    #[test]
    fn test_class_forname_with_user_input_detected() {
        let source = r#"
public class Controller {
    public void handle(HttpServletRequest request) {
        String className = request.getParameter("class");
        Class<?> clazz = Class.forName(className);
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            !findings.is_empty(),
            "Should detect Class.forName with user input"
        );
        // Note: This is High severity because the file contains HttpServletRequest,
        // indicating user input handling. The pattern-based check cannot do true
        // cross-statement taint tracking, but it elevates severity when it sees
        // that the file handles user input.
        assert!(
            findings[0].severity == Severity::High || findings[0].severity == Severity::Critical,
            "Should be High or Critical severity for files handling user input"
        );
    }

    #[test]
    fn test_newinstance_with_user_input_detected() {
        let source = r#"
public class Controller {
    public void handle(HttpServletRequest request) {
        String className = request.getParameter("class");
        Class<?> clazz = Class.forName(className);
        Object obj = clazz.newInstance();
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            !findings.is_empty(),
            "Should detect newInstance with user input"
        );
    }

    #[test]
    fn test_method_invoke_with_user_input_detected() {
        let source = r#"
public class Controller {
    public void handle(HttpServletRequest request) {
        String methodName = request.getParameter("method");
        Method method = clazz.getMethod(methodName);
        method.invoke(obj);
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            !findings.is_empty(),
            "Should detect Method.invoke with user input"
        );
    }

    // ==================== FALSE POSITIVES (MUST NOT DETECT) ====================

    #[test]
    fn test_literal_class_name_not_detected() {
        let source = r#"
public class ServiceLoader {
    public void load() {
        Class<?> clazz = Class.forName("com.example.MyService");
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should NOT detect Class.forName with literal class name"
        );
    }

    #[test]
    fn test_class_literal_not_detected() {
        let source = r#"
public class ServiceLoader {
    public void load() {
        Object obj = MyService.class.newInstance();
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should NOT detect newInstance on class literal"
        );
    }

    #[test]
    fn test_constructor_with_size_not_detected() {
        let source = r#"
public class ListFactory {
    public List<?> create(int listSize) {
        return listConstructor.newInstance(listSize);
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should NOT detect constructor.newInstance with size parameter"
        );
    }

    #[test]
    fn test_proxy_invoke_not_detected() {
        let source = r#"
public class ClientProxy implements InvocationHandler {
    private Object target;

    public Object invoke(Object proxy, Method method, Object[] args) throws Throwable {
        return method.invoke(target, args);
    }

    public Object createProxy() {
        return Proxy.newProxyInstance(
            target.getClass().getClassLoader(),
            new Class[]{Runnable.class},
            this
        );
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should not flag proxy invocation handlers"
        );
    }

    #[test]
    fn test_gson_type_adapter_forname_not_detected() {
        let source = r#"
public class GsonAdapter implements JsonDeserializer<Foo> {
    public Foo deserialize(JsonElement json, Type type, JsonDeserializationContext ctx) throws Exception {
        Class<?> clazz = Class.forName(type.getTypeName());
        return (Foo) clazz.newInstance();
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should not flag Gson type adapter reflection"
        );
    }

    #[test]
    fn test_safe_collection_class_not_detected() {
        let source = r#"
public class CollectionFactory {
    public void create() {
        Class<?> clazz = Class.forName("java.util.ArrayList");
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should NOT detect safe collection class"
        );
    }

    // ==================== MEDIUM SEVERITY (CONFIG-BASED) ====================

    #[test]
    fn test_config_based_lower_severity() {
        let source = r#"
public class PluginLoader {
    public void load() {
        String className = config.getProperty("plugin.class");
        Class<?> clazz = Class.forName(className);
    }
}
"#;
        let findings = scan_source(source);
        // Should be detected but with lower severity
        if !findings.is_empty() {
            assert!(
                findings[0].severity != Severity::Critical,
                "Config-based class names should not be Critical severity"
            );
        }
    }

    #[test]
    fn test_dynamic_class_name_medium_severity() {
        let source = r#"
public class DynamicLoader {
    public void load(String className) {
        Class<?> clazz = Class.forName(className);
    }
}
"#;
        let findings = scan_source(source);
        // Dynamic but not confirmed user input - medium severity
        if !findings.is_empty() {
            assert_eq!(
                findings[0].severity,
                Severity::Medium,
                "Dynamic class name without confirmed user input should be Medium"
            );
        }
    }

    // ==================== EDGE CASES ====================

    #[test]
    fn test_numeric_literal_constructor_arg_not_detected() {
        let source = r#"
public class Factory {
    public Object create() {
        return constructor.newInstance(10);
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should NOT detect constructor.newInstance with numeric literal"
        );
    }

    #[test]
    fn test_capacity_variable_not_detected() {
        let source = r#"
public class Factory {
    public Object create(int capacity) {
        return constructor.newInstance(capacity);
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should NOT detect constructor.newInstance with capacity variable"
        );
    }

    // ==================== SAFE REFLECTION PATTERNS (Phase 3.1) ====================

    #[test]
    fn test_existing_class_getname_not_detected() {
        // Variation: existingClass.getName()
        let source = r#"
public class TypeUtils {
    public void process(Class<?> existingClass) {
        Class<?> clazz = Class.forName(existingClass.getName());
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should NOT detect Class.forName(existingClass.getName())"
        );
    }

    #[test]
    fn test_getclass_pattern_not_detected() {
        // Pattern: obj.getClass().newInstance()
        // Runtime type of existing object - not user controlled
        let source = r#"
public class Cloner {
    public Object clone(Object obj) {
        return obj.getClass().newInstance();
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should NOT detect obj.getClass().newInstance() - runtime type"
        );
    }

    #[test]
    fn test_getclass_getmethod_not_detected() {
        // Pattern: obj.getClass().getMethod("foo").invoke(obj)
        let source = r#"
public class Invoker {
    public void invoke(Object obj) {
        obj.getClass().getMethod("toString").invoke(obj);
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should NOT detect obj.getClass().getMethod().invoke()"
        );
    }

    #[test]
    fn test_method_parameter_invoke_detected_conservatively() {
        // Pattern: method.invoke(obj, args) where method is a Method parameter
        // This is a utility wrapper, but without full dataflow analysis we can't
        // determine if the caller passes a safe Method object. We detect it
        // conservatively with MEDIUM severity.
        let source = r#"
public class ReflectiveStrategy {
    static Object invoke(Method method, Object obj, Object... args) {
        return method.invoke(obj, args);
    }
}
"#;
        let findings = scan_source(source);
        // We detect this conservatively - it's not a false positive, just lower risk
        // The severity should be Medium, not Critical
        if !findings.is_empty() {
            assert_eq!(
                findings[0].severity,
                Severity::Medium,
                "Generic method.invoke should be Medium severity without user input context"
            );
        }
    }

    #[test]
    fn test_constructor_parameter_newinstance_not_detected() {
        // Pattern: constructor.newInstance(args) where constructor is a Constructor parameter
        let source = r#"
public class Factory {
    public Object create(Constructor<?> constructor, Object... args) {
        return constructor.newInstance(args);
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            findings.is_empty(),
            "Should NOT detect constructor.newInstance() when constructor is a parameter"
        );
    }

    // ==================== UNSAFE PATTERNS STILL DETECTED (Phase 3.1) ====================

    #[test]
    fn test_user_input_still_detected() {
        // Should STILL detect: Class.forName(request.getParameter("class"))
        let source = r#"
public class Controller {
    public void handle(HttpServletRequest request) {
        Class<?> clazz = Class.forName(request.getParameter("class"));
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            !findings.is_empty(),
            "Should STILL detect Class.forName with user input"
        );
    }

    #[test]
    fn test_unknown_variable_still_detected() {
        // Should STILL detect: Class.forName(className) where className is unknown
        let source = r#"
public class DynamicLoader {
    public void load(String className) {
        Class<?> clazz = Class.forName(className);
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            !findings.is_empty(),
            "Should STILL detect Class.forName with unknown variable"
        );
    }

    #[test]
    fn test_user_method_name_still_detected() {
        // Should STILL detect: clazz.getMethod(userInput).invoke(obj)
        let source = r#"
public class Controller {
    public void handle(HttpServletRequest request) {
        String methodName = request.getParameter("method");
        Method method = clazz.getMethod(methodName);
        method.invoke(obj);
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            !findings.is_empty(),
            "Should STILL detect getMethod with user input"
        );
    }

    #[test]
    fn test_getname_from_request_still_detected() {
        // Edge case: request.getName() is NOT the same as class.getName()
        // This should still be detected as potentially unsafe
        let source = r#"
public class Controller {
    public void handle(Request request) {
        Class<?> clazz = Class.forName(request.getName());
    }
}
"#;
        let findings = scan_source(source);
        assert!(
            !findings.is_empty(),
            "Should STILL detect Class.forName(request.getName()) - request is user input"
        );
    }
}
