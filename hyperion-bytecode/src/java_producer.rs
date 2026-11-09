// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java bytecode producer - compile Java source to JVM bytecode
//!
//! This module implements the BytecodeProducer trait for Java, converting
//! Java source code to valid JVM .class files.

use crate::java_compiler::ClassDecl;
use crate::java_converter::convert_compilation_unit;
use crate::producer::{
    BytecodeError, BytecodeProducer, BytecodeResult, CompilationContext, JvmLanguage,
    LanguageFeature,
};
use hyperion_parser::syntax::SyntaxNode;
use hyperion_parser::{Lexer, Parser};
use parking_lot::Mutex;
use std::path::Path;

/// Extract class name from file path
/// e.g., "com/example/HelloWorld.java" => "HelloWorld"
fn extract_class_name(file_path: &str) -> String {
    Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Main")
        .to_string()
}

pub struct JavaBytecodeProducer {
    /// Class name extracted from source
    class_name: Mutex<String>,
    /// Full class declaration (preferred compilation path; the primary class).
    class_decl: Mutex<Option<ClassDecl>>,
    /// All top-level classes/records in the unit (for cross-class loading).
    class_decls: Mutex<Vec<ClassDecl>>,
}

impl JavaBytecodeProducer {
    /// Create a new Java bytecode producer
    pub fn new() -> Self {
        Self {
            class_name: Mutex::new(String::from("Main")),
            class_decl: Mutex::new(None),
            class_decls: Mutex::new(Vec::new()),
        }
    }
}

impl Default for JavaBytecodeProducer {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaBytecodeProducer {
    /// Return the entry point to invoke for a parsed full class, as
    /// `(class_internal_name, method_name, descriptor)`.
    ///
    /// Prefers `public static void main(String[])`; otherwise falls back to the
    /// first static method (useful for evaluating snippets). Returns `None` if
    /// no class was parsed or it has no static methods.
    pub fn entry_point(&self) -> Option<(String, String, String)> {
        // Prefer a `public static void main(String[])` anywhere in the unit — a
        // helper class is often declared *before* the entry class (and the eval
        // harness names the file `__Eval__`, so the primary can't be chosen by
        // file name). Scan every parsed class for `main` first.
        let decls = self.class_decls.lock();
        let is_main = |m: &crate::java_compiler::MethodDecl| m.name == "main" && m.is_static;
        if let Some((decl, m)) = decls
            .iter()
            .find_map(|d| d.methods.iter().find(|m| is_main(m)).map(|m| (d, m)))
        {
            return Some((decl.name.clone(), m.name.clone(), m.descriptor()));
        }
        drop(decls);

        // No `main`: fall back to the primary class's first static method (used
        // when evaluating a snippet that defines a single helper method).
        let guard = self.class_decl.lock();
        let decl = guard.as_ref()?;
        let chosen = decl.methods.iter().find(|m| m.is_static)?;
        Some((decl.name.clone(), chosen.name.clone(), chosen.descriptor()))
    }

    /// Compile every top-level class/record in the unit to `(name, bytes)`, so a
    /// caller can load a file's helper classes alongside the primary one and
    /// resolve cross-class references. A class that fails to compile is skipped.
    pub fn produce_all_classes(&self) -> BytecodeResult<Vec<(String, Vec<u8>)>> {
        let mut decls = self.class_decls.lock().clone();
        // Order by superclass depth within the unit so a class loads (and
        // links) after any in-unit superclass — the linker resolves supers from
        // already-loaded classes.
        let super_of: std::collections::HashMap<String, String> = decls
            .iter()
            .map(|d| (d.name.clone(), d.super_name.clone()))
            .collect();
        let limit = decls.len();
        let depth = |name: &str| {
            let (mut d, mut cur) = (0usize, name.to_string());
            while let Some(sup) = super_of.get(&cur) {
                d += 1;
                if d > limit {
                    break; // guard against a malformed cycle
                }
                cur = sup.clone();
            }
            d
        };
        decls.sort_by_key(|d| depth(&d.name));
        // A class that fails to compile fails the unit, rather than being left out
        crate::java_backend::compile_unit(&decls)
            .into_iter()
            .map(|(name, result)| result.map(|bytes| (name, bytes)))
            .collect()
    }
}

impl BytecodeProducer for JavaBytecodeProducer {
    fn language(&self) -> JvmLanguage {
        JvmLanguage::Java
    }

    fn parse(&self, source: &str, file_path: &str) -> BytecodeResult<()> {
        // Tokenize the source code
        let tokens = Lexer::new(source).collect();

        // Parse to CST (Concrete Syntax Tree)
        let (green_node, parse_errors) = Parser::parse(tokens);

        if !parse_errors.is_empty() {
            return Err(BytecodeError::ParseError {
                file: file_path.to_string(),
                line: 0,
                column: 0,
                message: format!("Parse errors: {:?}", parse_errors),
            });
        }

        // Convert green tree to red tree (SyntaxNode)
        let root = SyntaxNode::new_root(green_node);

        // Extract class name from file path
        let class_name = extract_class_name(file_path);
        *self.class_name.lock() = class_name;

        // Preferred path: build all top-level class declarations. The primary
        // class (for the entry point) is the one matching the file name, else
        // the first. A file-name fallback fills an empty primary name.
        let mut decls = convert_compilation_unit(root)?;
        let file_class = self.class_name.lock().clone();
        let primary_idx = decls.iter().position(|d| d.name == file_class).unwrap_or(0);
        let primary = decls.get(primary_idx).cloned().map(|mut d| {
            if d.name.is_empty() {
                d.name = file_class.clone();
            }
            d
        });
        *self.class_decl.lock() = primary;
        *self.class_decls.lock() = std::mem::take(&mut decls);

        Ok(())
    }

    fn produce_bytes(&self, _context: &CompilationContext) -> BytecodeResult<Vec<u8>> {
        // A source with no class has nothing to compile to a class file. It is an error here,
        // not a placeholder class.
        match self.class_decl.lock().clone() {
            Some(decl) => crate::java_backend::compile_class(&decl),
            None => Err(BytecodeError::UnsupportedFeature {
                feature: "a source file with no class declaration".to_string(),
            }),
        }
    }

    fn supported_features(&self) -> Vec<LanguageFeature> {
        vec![
            LanguageFeature::Records,
            LanguageFeature::SealedClasses,
            LanguageFeature::PatternMatching,
            LanguageFeature::TextBlocks,
            LanguageFeature::SwitchExpressions,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses and produces `source`, and says whether either step failed.
    fn produce_fails(source: &str) -> bool {
        let producer = JavaBytecodeProducer::new();
        if producer.parse(source, "T.java").is_err() {
            return true;
        }
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("T.java".into()),
            ..Default::default()
        };
        producer.produce_bytes(&context).is_err()
    }

    #[test]
    fn an_interface_and_an_annotation_type_compile() {
        // Interfaces and annotation types compile, at top level and nested.
        assert!(!produce_fails("public interface T { int x(); }"));
        assert!(!produce_fails(
            "public class T { public static int run() { return 2; } } interface E { int x(); }"
        ));
        assert!(!produce_fails(
            "public class T { interface E { int x(); } public static int run() { return 2; } }"
        ));
        assert!(!produce_fails("public @interface Marker { }"));
    }

    #[test]
    fn an_enum_is_compiled_and_is_not_a_placeholder() {
        // Enums compile now. The unit's class is the enum, so the producer must succeed.
        assert!(!produce_fails("public enum T { RED, GREEN }"));
        assert!(!produce_fails(
            "public class T { public static int run() { return 2; } } enum E { A }"
        ));
    }

    #[test]
    fn a_source_with_no_class_is_an_error() {
        assert!(produce_fails("package example;\n"));
    }

    #[test]
    fn test_java_producer_language() {
        let producer = JavaBytecodeProducer::new();
        assert_eq!(producer.language(), JvmLanguage::Java);
    }

    #[test]
    fn test_java_producer_features() {
        let producer = JavaBytecodeProducer::new();
        let features = producer.supported_features();
        assert!(!features.is_empty());
        assert!(features.contains(&LanguageFeature::Records));
        assert!(features.contains(&LanguageFeature::SealedClasses));
    }
}
