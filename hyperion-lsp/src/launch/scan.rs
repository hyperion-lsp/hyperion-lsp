// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Syntax-level discovery of JVM entry points and test classes.
//!
//! Both scanners produce the same flat description of a source file: the
//! classes it declares (with their JVM binary names, `Outer$Inner`), and for
//! each class the methods that are launchable (`main`) or runnable (test
//! annotations). Ranges are byte offsets into the scanned text so a caller can
//! find the innermost declaration around an editor position.

use hyperion_parser::syntax::{SyntaxKind as J, SyntaxNode as JNode};
use hyperion_workspace::Language;

/// How a `main` method is launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainKind {
    /// `public static void main(String[])` (Java) or the Kotlin equivalents.
    Classic,
    /// JEP 512 shapes (instance `main`, no-arg `main`, non-public `main`),
    /// final in JDK 25 and preview in 21-24.
    Extended,
}

#[derive(Debug, Clone)]
pub(crate) struct ScannedMethod {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub main: Option<MainKind>,
    pub is_test: bool,
    /// Binary name to launch when it differs from the declaring class (Kotlin
    /// `companion object { @JvmStatic fun main }` lives on the outer class).
    pub launch_class: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScannedClass {
    /// Package-less JVM binary name, e.g. `Outer$Inner` or `AppKt`.
    pub binary_name: String,
    pub start: usize,
    pub end: usize,
    /// The class itself carries a test annotation (TestNG `@Test` on a class).
    pub class_level_test: bool,
    /// Synthetic Kotlin file facade holding top-level functions.
    pub is_facade: bool,
    pub methods: Vec<ScannedMethod>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScannedFile {
    pub language: Language,
    pub package: Option<String>,
    pub classes: Vec<ScannedClass>,
}

impl ScannedFile {
    pub(crate) fn qualify(&self, binary_name: &str) -> String {
        match &self.package {
            Some(package) if !package.is_empty() => format!("{package}.{binary_name}"),
            _ => binary_name.to_owned(),
        }
    }

    /// True when the class, or any class nested inside it, has a test.
    pub(crate) fn class_has_tests(&self, class: &ScannedClass) -> bool {
        let nested_prefix = format!("{}$", class.binary_name);
        self.classes.iter().any(|candidate| {
            (candidate.binary_name == class.binary_name
                || candidate.binary_name.starts_with(&nested_prefix))
                && (candidate.class_level_test
                    || candidate.methods.iter().any(|method| method.is_test))
        })
    }
}

/// Annotation simple names that make a method a test on JUnit 3-5, TestNG and
/// `kotlin.test`.
const TEST_ANNOTATIONS: &[&str] = &[
    "Test",
    "ParameterizedTest",
    "RepeatedTest",
    "TestFactory",
    "TestTemplate",
    "Theory",
];

fn is_test_annotation(simple_name: &str) -> bool {
    TEST_ANNOTATIONS.contains(&simple_name)
}

fn simple_annotation_name(text: &str) -> String {
    let head = text.trim_start_matches('@');
    let head = head.split('(').next().unwrap_or(head).trim();
    let head = head.rsplit(':').next().unwrap_or(head);
    head.rsplit('.').next().unwrap_or(head).trim().to_owned()
}

// ----------------------------------------------------------------------------
// Java
// ----------------------------------------------------------------------------

pub(crate) fn scan_java(source: &str) -> ScannedFile {
    let tokens = hyperion_parser::lexer::Lexer::new(source).collect::<Vec<_>>();
    let (green, _errors) = hyperion_parser::parser::Parser::parse(tokens);
    let root = JNode::new_root(green);
    let package = root
        .children()
        .find(|node| node.kind() == J::PACKAGE_DECL)
        .and_then(|node| {
            node.children()
                .find(|child| child.kind() == J::QUALIFIED_NAME)
                .map(|name| compact(&name.text().to_string()))
        });
    let mut classes = Vec::new();
    for child in root.children() {
        java_class(&child, None, &mut classes);
    }
    ScannedFile {
        language: Language::Java,
        package,
        classes,
    }
}

const fn is_java_type_decl(kind: J) -> bool {
    matches!(
        kind,
        J::CLASS_DECL | J::INTERFACE_DECL | J::ENUM_DECL | J::RECORD_DECL | J::ANNOTATION_DECL
    )
}

fn java_ident(node: &JNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind() == J::IDENT)
        .map(|token| token.text().to_owned())
}

/// Modifier list that precedes a declaration as a sibling node.
fn java_modifiers(node: &JNode) -> Option<JNode> {
    node.prev_sibling()
        .filter(|sibling| sibling.kind() == J::MODIFIER_LIST)
}

struct JavaModifiers {
    is_public: bool,
    is_static: bool,
    is_private: bool,
    annotations: Vec<String>,
    start: Option<usize>,
}

fn java_modifier_info(node: &JNode) -> JavaModifiers {
    let mut info = JavaModifiers {
        is_public: false,
        is_static: false,
        is_private: false,
        annotations: Vec::new(),
        start: None,
    };
    let Some(list) = java_modifiers(node) else {
        return info;
    };
    info.start = Some(usize::from(list.text_range().start()));
    for element in list.descendants_with_tokens() {
        match element {
            rowan::NodeOrToken::Token(token) => match token.kind() {
                J::PUBLIC_KW => info.is_public = true,
                J::STATIC_KW => info.is_static = true,
                J::PRIVATE_KW => info.is_private = true,
                _ => {}
            },
            rowan::NodeOrToken::Node(annotation) if annotation.kind() == J::ANNOTATION => {
                info.annotations
                    .push(simple_annotation_name(&annotation.text().to_string()));
            }
            rowan::NodeOrToken::Node(_) => {}
        }
    }
    info
}

fn java_class(node: &JNode, outer: Option<&str>, out: &mut Vec<ScannedClass>) {
    if !is_java_type_decl(node.kind()) {
        return;
    }
    let Some(name) = java_ident(node) else {
        return;
    };
    let binary_name = outer.map_or_else(|| name.clone(), |outer| format!("{outer}${name}"));
    let modifiers = java_modifier_info(node);
    let is_interface = matches!(node.kind(), J::INTERFACE_DECL | J::ANNOTATION_DECL);
    let index = out.len();
    out.push(ScannedClass {
        binary_name: binary_name.clone(),
        start: modifiers
            .start
            .unwrap_or_else(|| usize::from(node.text_range().start())),
        end: usize::from(node.text_range().end()),
        class_level_test: modifiers
            .annotations
            .iter()
            .any(|annotation| annotation == "Test"),
        is_facade: false,
        methods: Vec::new(),
    });
    for member in node.children() {
        if member.kind() == J::METHOD_DECL {
            if let Some(method) = java_method(&member, is_interface) {
                out[index].methods.push(method);
            }
        } else {
            java_class(&member, Some(&binary_name), out);
        }
    }
}

fn java_method(node: &JNode, in_interface: bool) -> Option<ScannedMethod> {
    let name = java_ident(node)?;
    let modifiers = java_modifier_info(node);
    let is_test = modifiers
        .annotations
        .iter()
        .any(|annotation| is_test_annotation(annotation));
    let main = if name == "main" {
        java_main_kind(node, &modifiers, in_interface)
    } else {
        None
    };
    Some(ScannedMethod {
        name,
        start: modifiers
            .start
            .unwrap_or_else(|| usize::from(node.text_range().start())),
        end: usize::from(node.text_range().end()),
        main,
        is_test,
        launch_class: None,
    })
}

fn java_main_kind(node: &JNode, modifiers: &JavaModifiers, in_interface: bool) -> Option<MainKind> {
    let returns_void = node.children().next().is_some_and(|first| {
        first.kind() == J::PRIMITIVE_TYPE
            && first
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .any(|token| token.kind() == J::VOID_KW)
    });
    if !returns_void || modifiers.is_private {
        return None;
    }
    let params: Vec<JNode> = node
        .children()
        .find(|child| child.kind() == J::PARAM_LIST)?
        .children()
        .filter(|child| child.kind() == J::PARAM)
        .collect();
    let takes_string_array = match params.as_slice() {
        [] => false,
        [param] => {
            let has_string = param
                .children()
                .filter(|child| child.kind() == J::CLASS_TYPE)
                .any(|ty| compact(&ty.text().to_string()).rsplit('.').next() == Some("String"));
            let has_array = param.children().any(|child| child.kind() == J::ARRAY_TYPE)
                || param
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .any(|token| token.kind() == J::ELLIPSIS);
            if !(has_string && has_array) {
                return None;
            }
            true
        }
        _ => return None,
    };
    let is_public = modifiers.is_public || in_interface;
    if takes_string_array && modifiers.is_static && is_public {
        Some(MainKind::Classic)
    } else {
        Some(MainKind::Extended)
    }
}

fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

// ----------------------------------------------------------------------------
// Kotlin
// ----------------------------------------------------------------------------

use hyperion_kotlin::{SyntaxKind as K, SyntaxNode as KNode};

pub(crate) fn scan_kotlin(source: &str, file_stem: &str) -> ScannedFile {
    let tree = hyperion_kotlin::parse(source);
    let root = KNode::new_root(tree.green_node);
    let package = kotlin_package(&root);

    // `@file:JvmName("X")` renames the facade class.
    let mut facade_name = default_facade_name(file_stem);
    // The parser attaches file annotations directly to the file node, before the
    // package declaration. Annotations under a MODIFIERS node are also accepted.
    let file_annotations: Vec<_> = root
        .children()
        .filter(|node| node.kind() == K::ANNOTATION)
        .chain(
            root.children()
                .filter(|node| node.kind() == K::MODIFIERS)
                .flat_map(|modifiers| modifiers.children().collect::<Vec<_>>())
                .filter(|node| node.kind() == K::ANNOTATION),
        )
        .collect();
    for annotation in file_annotations {
        let text = annotation.text().to_string();
        if text.starts_with("@file:") && simple_annotation_name(&text) == "JvmName" {
            if let Some(name) = quoted_argument(&text) {
                facade_name = name;
            }
        }
    }

    let mut classes = vec![ScannedClass {
        binary_name: facade_name,
        start: 0,
        end: source.len(),
        class_level_test: false,
        is_facade: true,
        methods: Vec::new(),
    }];
    for child in root.children() {
        match child.kind() {
            K::FUNCTION_DECL => {
                if let Some(method) = kotlin_function(&child, true, false, None) {
                    classes[0].methods.push(method);
                }
            }
            _ => kotlin_class(&child, None, &mut classes),
        }
    }
    // The facade only matters when it holds something launchable.
    if classes[0].methods.is_empty() {
        classes.remove(0);
    }
    ScannedFile {
        language: Language::Kotlin,
        package,
        classes,
    }
}

/// The Kotlin CST keeps `package a.b.c` as flat tokens, not a declaration node.
fn kotlin_package(root: &KNode) -> Option<String> {
    if let Some(decl) = root.children().find(|node| node.kind() == K::PACKAGE_DECL) {
        let name: String = decl
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| matches!(token.kind(), K::IDENT | K::DOT))
            .map(|token| token.text().to_owned())
            .collect();
        return (!name.is_empty()).then_some(name);
    }
    let mut package = String::new();
    let mut seen_keyword = false;
    for element in root.children_with_tokens() {
        match element {
            rowan::NodeOrToken::Token(token) => match token.kind() {
                K::PACKAGE_KW if !seen_keyword => seen_keyword = true,
                K::IDENT | K::DOT if seen_keyword => package.push_str(token.text()),
                K::WHITESPACE | K::LINE_COMMENT | K::BLOCK_COMMENT => {}
                _ if seen_keyword => break,
                _ => {}
            },
            rowan::NodeOrToken::Node(node) => {
                // Empty MODIFIERS placeholders sit between the tokens.
                if seen_keyword && node.kind() != K::MODIFIERS {
                    break;
                }
            }
        }
    }
    (!package.is_empty()).then_some(package)
}

fn default_facade_name(file_stem: &str) -> String {
    let mut name = String::new();
    for (index, ch) in file_stem.chars().enumerate() {
        let valid = ch == '_' || ch.is_alphanumeric();
        let ch = if valid { ch } else { '_' };
        if index == 0 {
            name.extend(ch.to_uppercase());
        } else {
            name.push(ch);
        }
    }
    name.push_str("Kt");
    name
}

fn quoted_argument(text: &str) -> Option<String> {
    let open = text.find('"')?;
    let close = text[open + 1..].find('"')?;
    Some(text[open + 1..open + 1 + close].to_owned())
}

struct KotlinModifiers {
    annotations: Vec<String>,
    is_companion: bool,
    start: Option<usize>,
}

fn kotlin_modifier_info(node: &KNode) -> KotlinModifiers {
    let mut info = KotlinModifiers {
        annotations: Vec::new(),
        is_companion: false,
        start: None,
    };
    let Some(list) = node
        .prev_sibling()
        .filter(|sibling| sibling.kind() == K::MODIFIERS)
    else {
        return info;
    };
    let list_start = usize::from(list.text_range().start());
    if usize::from(list.text_range().end()) > list_start {
        info.start = Some(list_start);
    }
    for element in list.descendants_with_tokens() {
        match element {
            rowan::NodeOrToken::Token(token) if token.kind() == K::COMPANION_KW => {
                info.is_companion = true;
            }
            rowan::NodeOrToken::Node(annotation) if annotation.kind() == K::ANNOTATION => {
                info.annotations
                    .push(simple_annotation_name(&annotation.text().to_string()));
            }
            _ => {}
        }
    }
    info
}

fn kotlin_ident(node: &KNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind() == K::IDENT)
        .map(|token| token.text().to_owned())
}

/// `outer` is `(binary name, launch class)` of the enclosing class-like
/// declaration; the launch class differs for companion objects.
fn kotlin_class(node: &KNode, outer: Option<&str>, out: &mut Vec<ScannedClass>) {
    if !matches!(
        node.kind(),
        K::CLASS_DECL | K::OBJECT_DECL | K::INTERFACE_DECL
    ) {
        return;
    }
    let modifiers = kotlin_modifier_info(node);
    let is_object = node.kind() == K::OBJECT_DECL;
    let name =
        kotlin_ident(node).or_else(|| modifiers.is_companion.then(|| "Companion".to_owned()));
    let Some(name) = name else {
        return;
    };
    let binary_name = outer.map_or_else(|| name.clone(), |outer| format!("{outer}${name}"));
    // `@JvmStatic` members of a companion are static on the containing class.
    let launch_class = if modifiers.is_companion {
        outer.map(str::to_owned)
    } else {
        Some(binary_name.clone())
    };
    let index = out.len();
    out.push(ScannedClass {
        binary_name: binary_name.clone(),
        start: modifiers
            .start
            .unwrap_or_else(|| usize::from(node.text_range().start())),
        end: usize::from(node.text_range().end()),
        class_level_test: modifiers.annotations.iter().any(|a| a == "Test"),
        is_facade: false,
        methods: Vec::new(),
    });
    let Some(body) = node.children().find(|child| child.kind() == K::CLASS_BODY) else {
        return;
    };
    for member in body.children() {
        if member.kind() == K::FUNCTION_DECL {
            if let Some(method) =
                kotlin_function(&member, false, is_object, launch_class.as_deref())
            {
                out[index].methods.push(method);
            }
        } else {
            kotlin_class(&member, Some(&binary_name), out);
        }
    }
}

fn kotlin_function(
    node: &KNode,
    top_level: bool,
    in_object: bool,
    launch_class: Option<&str>,
) -> Option<ScannedMethod> {
    // Extension functions carry a receiver before the name; they are never
    // entry points.
    let has_receiver = node
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .any(|token| token.kind() == K::DOT);
    let name = kotlin_ident(node)?;
    let modifiers = kotlin_modifier_info(node);
    let is_test = modifiers
        .annotations
        .iter()
        .any(|annotation| is_test_annotation(annotation));
    let is_jvm_static = modifiers.annotations.iter().any(|a| a == "JvmStatic");
    let mut main = None;
    let mut launch = None;
    if name == "main" && !has_receiver && kotlin_main_params(node) {
        if top_level {
            main = Some(MainKind::Classic);
        } else if in_object && is_jvm_static {
            // Only `(args: Array<String>)` exists as a JVM static entry.
            if kotlin_param_count(node) == 1 {
                main = Some(MainKind::Classic);
                launch = launch_class.map(str::to_owned);
            }
        }
    }
    Some(ScannedMethod {
        name,
        start: modifiers
            .start
            .unwrap_or_else(|| usize::from(node.text_range().start())),
        end: usize::from(node.text_range().end()),
        main,
        is_test,
        launch_class: launch,
    })
}

fn kotlin_params(node: &KNode) -> Vec<KNode> {
    node.children()
        .find(|child| child.kind() == K::PARAMETER_LIST)
        .map(|list| {
            list.children()
                .filter(|child| child.kind() == K::PARAMETER)
                .collect()
        })
        .unwrap_or_default()
}

fn kotlin_param_count(node: &KNode) -> usize {
    kotlin_params(node).len()
}

fn kotlin_main_params(node: &KNode) -> bool {
    match kotlin_params(node).as_slice() {
        [] => true,
        [param] => {
            let text = compact(&param.text().to_string());
            text.contains(":Array<") && text.contains("String")
                || (text.starts_with("vararg") && text.contains(":String"))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn java(source: &str) -> ScannedFile {
        scan_java(source)
    }

    fn mains(file: &ScannedFile) -> Vec<(String, MainKind)> {
        file.classes
            .iter()
            .flat_map(|class| {
                class.methods.iter().filter_map(|method| {
                    method.main.map(|kind| {
                        (
                            method
                                .launch_class
                                .clone()
                                .unwrap_or_else(|| class.binary_name.clone()),
                            kind,
                        )
                    })
                })
            })
            .collect()
    }

    #[test]
    fn java_entry_points_and_binary_names() {
        let file = java(
            "package com.example;\n\
             public class A {\n\
               public static void main(String[] args) {}\n\
               static class Nested { public static void main(String... a) {} }\n\
               interface I { static void main(String[] args) {} }\n\
               record R(int x) { public static void main(String[] args) {} }\n\
               void main() {}\n\
             }\n",
        );
        assert_eq!(file.package.as_deref(), Some("com.example"));
        assert_eq!(
            mains(&file),
            vec![
                ("A".to_owned(), MainKind::Classic),
                ("A".to_owned(), MainKind::Extended),
                ("A$Nested".to_owned(), MainKind::Classic),
                ("A$I".to_owned(), MainKind::Classic),
                ("A$R".to_owned(), MainKind::Classic),
            ]
        );
        assert_eq!(file.qualify("A$Nested"), "com.example.A$Nested");
    }

    #[test]
    fn java_rejects_non_entry_mains() {
        let file = java(
            "class A { public static int main(String[] a) { return 0; }\n\
             public static void main(int x) {}\n\
             private static void main(String[] a) {}\n\
             public static void main(String[] a, int b) {} }",
        );
        assert!(mains(&file).is_empty());
    }

    #[test]
    fn java_test_annotations() {
        let file = java(
            "class T {\n\
               @Test void one() {}\n\
               @org.junit.jupiter.params.ParameterizedTest @ValueSource(ints = {1}) void two(int x) {}\n\
               void helper() {}\n\
               @Nested class Inner { @Test void three() {} }\n\
             }\n\
             @Test class Suite {}\n",
        );
        let t = &file.classes[0];
        let tests: Vec<_> = t
            .methods
            .iter()
            .filter(|m| m.is_test)
            .map(|m| &m.name)
            .collect();
        assert_eq!(tests, ["one", "two"]);
        assert_eq!(file.classes[1].binary_name, "T$Inner");
        assert!(file.classes[1].methods[0].is_test);
        assert!(file.classes[2].class_level_test);
        assert!(
            file.class_has_tests(t),
            "nested tests count for the outer class"
        );
    }

    #[test]
    fn kotlin_facade_jvm_name_companion_and_objects() {
        let file = scan_kotlin(
            "@file:JvmName(\"Custom\")\npackage com.example\n\
             fun main() {}\n\
             fun main(args: Array<String>) {}\n\
             fun String.main() {}\n\
             class Foo {\n\
               companion object { @JvmStatic fun main(args: Array<String>) {} }\n\
               @Test fun t() {}\n\
               class Inner { @Test fun u() {} }\n\
             }\n\
             object Obj { @JvmStatic fun main(args: Array<String>) {}\n fun main2() {} }\n\
             object Plain { fun main(args: Array<String>) {} }\n",
            "A",
        );
        assert_eq!(file.package.as_deref(), Some("com.example"));
        assert_eq!(
            mains(&file),
            vec![
                ("Custom".to_owned(), MainKind::Classic),
                ("Custom".to_owned(), MainKind::Classic),
                ("Foo".to_owned(), MainKind::Classic),
                ("Obj".to_owned(), MainKind::Classic),
            ]
        );
        let names: Vec<_> = file
            .classes
            .iter()
            .map(|c| c.binary_name.as_str())
            .collect();
        assert!(names.contains(&"Foo$Companion"));
        assert!(names.contains(&"Foo$Inner"));
    }

    #[test]
    fn kotlin_default_facade_name_follows_the_file_name() {
        let file = scan_kotlin("fun main() {}\n", "my-app");
        assert_eq!(file.classes[0].binary_name, "My_appKt");
        assert_eq!(file.package, None);
    }
}
