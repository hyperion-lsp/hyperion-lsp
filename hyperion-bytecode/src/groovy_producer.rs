// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy bytecode producer - compile Groovy source to JVM bytecode
//!
//! This module implements the BytecodeProducer trait for Groovy, enabling
//! compilation of Groovy source code to JVM bytecode with support for:
//! - Closures with delegate setup
//! - Dynamic method dispatch
//! - GString interpolation
//! - Groovy Meta-Object Protocol (MOP)

use crate::producer::{
    BytecodeError, BytecodeProducer, BytecodeResult, CompilationContext, ConstantPoolItem,
    JvmLanguage, LanguageFeature,
};

/// Write a constant pool entry to bytecode
pub fn write_constant_pool_entry(
    bytes: &mut Vec<u8>,
    entry: &ConstantPoolItem,
) -> BytecodeResult<()> {
    match entry {
        ConstantPoolItem::Utf8(s) => {
            bytes.push(0x01); // CONSTANT_Utf8
            let encoded = crate::modified_utf8::encode(s);
            let len = u16::try_from(encoded.len()).map_err(|_| BytecodeError::InternalError {
                message: "Modified UTF-8 constant exceeds the JVM u16 length limit".to_string(),
            })?;
            bytes.extend_from_slice(&len.to_be_bytes());
            bytes.extend_from_slice(&encoded);
        }
        ConstantPoolItem::Integer(v) => {
            bytes.push(0x03); // CONSTANT_Integer
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        ConstantPoolItem::Float(v) => {
            bytes.push(0x04); // CONSTANT_Float
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        ConstantPoolItem::Long(v) => {
            bytes.push(0x05); // CONSTANT_Long
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        ConstantPoolItem::Double(v) => {
            bytes.push(0x06); // CONSTANT_Double
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        ConstantPoolItem::Class { name_index } => {
            bytes.push(0x07); // CONSTANT_Class
            bytes.extend_from_slice(&name_index.to_be_bytes());
        }
        ConstantPoolItem::String { string_index } => {
            bytes.push(0x08); // CONSTANT_String
            bytes.extend_from_slice(&string_index.to_be_bytes());
        }
        ConstantPoolItem::FieldRef {
            class_index,
            name_and_type_index,
        } => {
            bytes.push(0x09); // CONSTANT_Fieldref
            bytes.extend_from_slice(&class_index.to_be_bytes());
            bytes.extend_from_slice(&name_and_type_index.to_be_bytes());
        }
        ConstantPoolItem::MethodRef {
            class_index,
            name_and_type_index,
        } => {
            bytes.push(0x0A); // CONSTANT_Methodref
            bytes.extend_from_slice(&class_index.to_be_bytes());
            bytes.extend_from_slice(&name_and_type_index.to_be_bytes());
        }
        ConstantPoolItem::InterfaceMethodRef {
            class_index,
            name_and_type_index,
        } => {
            bytes.push(0x0B); // CONSTANT_InterfaceMethodref
            bytes.extend_from_slice(&class_index.to_be_bytes());
            bytes.extend_from_slice(&name_and_type_index.to_be_bytes());
        }
        ConstantPoolItem::NameAndType {
            name_index,
            descriptor_index,
        } => {
            bytes.push(0x0C); // CONSTANT_NameAndType
            bytes.extend_from_slice(&name_index.to_be_bytes());
            bytes.extend_from_slice(&descriptor_index.to_be_bytes());
        }
        ConstantPoolItem::MethodHandle {
            reference_kind,
            reference_index,
        } => {
            bytes.push(0x0F); // CONSTANT_MethodHandle
            bytes.push(*reference_kind);
            bytes.extend_from_slice(&reference_index.to_be_bytes());
        }
        ConstantPoolItem::MethodType { descriptor_index } => {
            bytes.push(0x10); // CONSTANT_MethodType
            bytes.extend_from_slice(&descriptor_index.to_be_bytes());
        }
        ConstantPoolItem::Dynamic {
            bootstrap_method_attr_index,
            name_and_type_index,
        } => {
            bytes.push(0x11); // CONSTANT_Dynamic
            bytes.extend_from_slice(&bootstrap_method_attr_index.to_be_bytes());
            bytes.extend_from_slice(&name_and_type_index.to_be_bytes());
        }
        ConstantPoolItem::InvokeDynamic {
            bootstrap_method_attr_index,
            name_and_type_index,
        } => {
            bytes.push(0x12); // CONSTANT_InvokeDynamic
            bytes.extend_from_slice(&bootstrap_method_attr_index.to_be_bytes());
            bytes.extend_from_slice(&name_and_type_index.to_be_bytes());
        }
        ConstantPoolItem::Module { name_index } => {
            bytes.push(0x13); // CONSTANT_Module
            bytes.extend_from_slice(&name_index.to_be_bytes());
        }
        ConstantPoolItem::Package { name_index } => {
            bytes.push(0x14); // CONSTANT_Package
            bytes.extend_from_slice(&name_index.to_be_bytes());
        }
    }
    Ok(())
}

/// Serialize a slice of constant-pool entries.
///
/// A `Long`/`Double` constant occupies two constant-pool indices but is encoded
/// as a single entry in the byte stream; the slot that follows it is a reserved
/// phantom (the builder pushed a placeholder there). It is *counted* in
/// `constant_pool_count` but must NOT be written — emitting it would shift every
/// later index and corrupt the class file (JVM spec §4.4.5).
pub fn write_constant_pool_entries(
    bytes: &mut Vec<u8>,
    entries: &[ConstantPoolItem],
) -> BytecodeResult<()> {
    let mut i = 0;
    while i < entries.len() {
        let entry = &entries[i];
        write_constant_pool_entry(bytes, entry)?;
        if matches!(
            entry,
            ConstantPoolItem::Long(_) | ConstantPoolItem::Double(_)
        ) {
            i += 2; // skip the reserved phantom slot
        } else {
            i += 1;
        }
    }
    Ok(())
}

/// Groovy bytecode producer for compiling Groovy source to JVM bytecode
pub struct GroovyBytecodeProducer;

fn script_class_name(context: &CompilationContext) -> &str {
    context
        .source_path
        .as_ref()
        .and_then(|path| {
            std::path::Path::new(path)
                .file_stem()
                .and_then(|stem| stem.to_str())
        })
        .unwrap_or("GroovyScript")
}

pub(crate) fn checked_method_code_length(method: &str, code: &[u8]) -> BytecodeResult<u32> {
    if code.len() > u16::MAX as usize {
        return Err(BytecodeError::CodeSizeExceeded {
            method: method.to_string(),
        });
    }
    Ok(code.len() as u32)
}

pub(crate) fn checked_u16_count(value: usize, description: &str) -> BytecodeResult<u16> {
    u16::try_from(value).map_err(|_| BytecodeError::InternalError {
        message: format!("{description} count {value} exceeds the JVM u16 limit"),
    })
}

pub(crate) fn checked_bootstrap_attribute_length(
    methods: &[crate::constant_pool::BootstrapMethodEntry],
) -> BytecodeResult<u32> {
    methods.iter().try_fold(2_u32, |length, method| {
        let argument_count = u32::try_from(method.bootstrap_arguments.len()).map_err(|_| {
            BytecodeError::InternalError {
                message: "bootstrap method arguments exceed the JVM attribute limit".to_string(),
            }
        })?;
        let argument_bytes =
            argument_count
                .checked_mul(2)
                .ok_or_else(|| BytecodeError::InternalError {
                    message: "BootstrapMethods attribute length overflow".to_string(),
                })?;
        length
            .checked_add(4)
            .and_then(|length| length.checked_add(argument_bytes))
            .ok_or_else(|| BytecodeError::InternalError {
                message: "BootstrapMethods attribute length overflow".to_string(),
            })
    })
}

impl GroovyBytecodeProducer {
    /// Create a new Groovy bytecode producer
    pub fn new() -> Self {
        Self
    }

    /// Compile Groovy source to all class bytecodes (main class + closure classes)
    ///
    /// Returns a HashMap where keys are class names and values are bytecode.
    /// The main class is named based on the source file (e.g., "Test" for "Test.groovy").
    /// Closure classes are named like "Test$_closure1".
    pub fn produce_all_classes(
        &self,
        context: &CompilationContext,
    ) -> BytecodeResult<std::collections::HashMap<String, Vec<u8>>> {
        use crate::constant_pool::{BootstrapMethodEntry, ConstantPoolBuilder};
        use crate::groovy_ast_compiler::GroovyAstCompiler;
        use hyperion_groovy::{tree_sitter_wrapper::GroovyParser, AstBuilder};

        let mut all_classes = std::collections::HashMap::new();

        // Derive class name from source path or use default
        let class_name = script_class_name(context);

        // === Parse Groovy source first ===
        let source = context.source.as_deref().unwrap_or("");

        // Parse with tree-sitter
        let mut parser = GroovyParser::new().map_err(|e| BytecodeError::InternalError {
            message: format!("Parser initialization failed: {}", e),
        })?;

        let parsed = parser
            .parse(source)
            .map_err(|e| BytecodeError::InternalError {
                message: format!("Parse error: {}", e),
            })?;

        // Convert CST to AST
        let ast_builder = AstBuilder::new(source);
        let statements = ast_builder.build_statements(parsed.root());

        // === Create constant pool and compile AST ===
        let mut cp = ConstantPoolBuilder::new();

        // Build constant pool entries for class structure
        let class_name_idx = cp.add_class(class_name);
        let super_class_idx = cp.add_class("groovy/lang/Script");

        // Add <init> method refs for constructor
        let init_name_idx = cp.add_utf8("<init>");
        let init_desc_idx = cp.add_utf8("()V");
        let _init_nat_idx = cp.add_name_and_type("<init>", "()V");
        let init_ref_idx = cp.add_method_ref("groovy/lang/Script", "<init>", "()V");

        // Add run() method descriptor
        let run_name_idx = cp.add_utf8("run");
        let run_desc_idx = cp.add_utf8("()Ljava/lang/Object;");

        // Add Code attribute name
        let code_attr_idx = cp.add_utf8("Code");

        // Compile AST to bytecode
        let mut ast_compiler = GroovyAstCompiler::new(cp);
        let _result_type = ast_compiler.compile_statements(&statements);
        // Check if the last statement was a return - if not, add one
        let ends_with_return = statements
            .last()
            .is_some_and(|stmt| matches!(stmt, hyperion_groovy::Statement::Return(_)));

        if !ends_with_return {
            ast_compiler.compile_null_return();
        }

        ast_compiler.validate()?;
        let run_bytecode = ast_compiler.bytecode().to_vec();
        let code_len = checked_method_code_length(&format!("{class_name}.run"), &run_bytecode)?;
        let max_stack = ast_compiler.max_stack()?;
        let max_locals = ast_compiler.max_locals();

        // Extract closure classes BEFORE consuming the compiler
        let closure_classes = ast_compiler.closure_classes().clone();
        let exception_handlers = ast_compiler.exception_handlers().to_vec();

        // Extract the constant pool back from the compiler
        let mut cp = ast_compiler.into_constant_pool();

        let mut serialized_exception_handlers = Vec::with_capacity(exception_handlers.len());
        for handler in exception_handlers {
            let checked_pc = |value: usize, field: &str| {
                u16::try_from(value).map_err(|_| BytecodeError::InternalError {
                    message: format!("{class_name}.run {field} exceeds the JVM code limit"),
                })
            };
            let catch_type = handler
                .catch_type
                .as_deref()
                .map(|class_name| cp.add_class(class_name))
                .unwrap_or(0);
            serialized_exception_handlers.push((
                checked_pc(handler.start_pc, "exception start_pc")?,
                checked_pc(handler.end_pc, "exception end_pc")?,
                checked_pc(handler.handler_pc, "exception handler_pc")?,
                catch_type,
            ));
        }
        let exception_handler_count =
            checked_u16_count(serialized_exception_handlers.len(), "run exception handler")?;

        // Add BootstrapMethods attribute name if we have bootstrap methods
        let bootstrap_attr_name_idx = if cp.has_bootstrap_methods() {
            Some(cp.add_utf8("BootstrapMethods"))
        } else {
            None
        };

        cp.validate()?;
        let cp_count = checked_u16_count(cp.len(), "constant pool")?;

        // Clone bootstrap methods before borrowing cp immutably for entries
        let bootstrap_methods: Vec<BootstrapMethodEntry> = cp.bootstrap_methods().to_vec();

        // Serialize constant pool
        let cp_entries = cp.get_entries();

        // === Write .class file structure ===
        let mut class_bytes = Vec::new();

        // Magic number
        class_bytes.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);

        // Version: Java 21 (major=65, minor=0)
        class_bytes.extend_from_slice(&[0x00, 0x00]); // minor
        class_bytes.extend_from_slice(&[0x00, 0x41]); // major (65)

        // Constant pool count includes the reserved index-zero slot.
        class_bytes.extend_from_slice(&cp_count.to_be_bytes());

        // Write constant pool entries (skips the phantom slot after Long/Double).
        write_constant_pool_entries(&mut class_bytes, cp_entries)?;

        // Access flags: ACC_PUBLIC | ACC_SUPER (0x0021)
        class_bytes.extend_from_slice(&[0x00, 0x21]);

        // this_class
        class_bytes.extend_from_slice(&class_name_idx.to_be_bytes());

        // super_class
        class_bytes.extend_from_slice(&super_class_idx.to_be_bytes());

        // interfaces_count
        class_bytes.extend_from_slice(&[0x00, 0x00]);

        // fields_count
        class_bytes.extend_from_slice(&[0x00, 0x00]);

        // methods_count (<init> + run)
        class_bytes.extend_from_slice(&[0x00, 0x02]);

        // === Method: <init>() ===
        class_bytes.extend_from_slice(&[0x00, 0x01]); // ACC_PUBLIC
        class_bytes.extend_from_slice(&init_name_idx.to_be_bytes());
        class_bytes.extend_from_slice(&init_desc_idx.to_be_bytes());
        class_bytes.extend_from_slice(&[0x00, 0x01]); // attributes_count

        // Code attribute
        class_bytes.extend_from_slice(&code_attr_idx.to_be_bytes());
        class_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x11]); // attribute_length = 17
        class_bytes.extend_from_slice(&[0x00, 0x01]); // max_stack = 1
        class_bytes.extend_from_slice(&[0x00, 0x01]); // max_locals = 1
        class_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x05]); // code_length = 5

        // Bytecode: aload_0; invokespecial Object.<init>; return
        class_bytes.push(0x2A); // aload_0
        class_bytes.push(0xB7); // invokespecial
        class_bytes.extend_from_slice(&init_ref_idx.to_be_bytes());
        class_bytes.push(0xB1); // return

        // Exception table
        class_bytes.extend_from_slice(&[0x00, 0x00]);

        // Code attributes
        class_bytes.extend_from_slice(&[0x00, 0x00]);

        // === Method: run() ===
        // ACC_PUBLIC | ACC_STATIC = 0x0009
        // Static so invoke_static works without needing 'this' in slot 0
        class_bytes.extend_from_slice(&[0x00, 0x09]); // ACC_PUBLIC | ACC_STATIC
        class_bytes.extend_from_slice(&run_name_idx.to_be_bytes());
        class_bytes.extend_from_slice(&run_desc_idx.to_be_bytes());
        class_bytes.extend_from_slice(&[0x00, 0x01]); // attributes_count

        // Code attribute for run()
        class_bytes.extend_from_slice(&code_attr_idx.to_be_bytes());

        // attribute_length = 12 + code_length + 8 * exception_table_length
        let exception_table_bytes = u32::from(exception_handler_count)
            .checked_mul(8)
            .ok_or_else(|| BytecodeError::InternalError {
                message: "run exception table length overflowed".to_string(),
            })?;
        let attr_len = 12u32
            .checked_add(code_len)
            .and_then(|length| length.checked_add(exception_table_bytes))
            .ok_or_else(|| BytecodeError::InternalError {
                message: "run Code attribute length overflowed".to_string(),
            })?;
        class_bytes.extend_from_slice(&attr_len.to_be_bytes());

        class_bytes.extend_from_slice(&max_stack.to_be_bytes());
        class_bytes.extend_from_slice(&max_locals.to_be_bytes());
        class_bytes.extend_from_slice(&code_len.to_be_bytes());

        // Bytecode: compiled run() method
        class_bytes.extend_from_slice(&run_bytecode);

        // Exception table
        class_bytes.extend_from_slice(&exception_handler_count.to_be_bytes());
        for (start_pc, end_pc, handler_pc, catch_type) in serialized_exception_handlers {
            class_bytes.extend_from_slice(&start_pc.to_be_bytes());
            class_bytes.extend_from_slice(&end_pc.to_be_bytes());
            class_bytes.extend_from_slice(&handler_pc.to_be_bytes());
            class_bytes.extend_from_slice(&catch_type.to_be_bytes());
        }

        // Code attributes
        class_bytes.extend_from_slice(&[0x00, 0x00]);

        // Class attributes - write BootstrapMethods if needed
        if let Some(attr_name_idx) = bootstrap_attr_name_idx {
            class_bytes.extend_from_slice(&[0x00, 0x01]);
            class_bytes.extend_from_slice(&attr_name_idx.to_be_bytes());

            let attr_length = checked_bootstrap_attribute_length(&bootstrap_methods)?;
            class_bytes.extend_from_slice(&attr_length.to_be_bytes());

            let num_methods =
                checked_u16_count(bootstrap_methods.len(), "BootstrapMethods attribute method")?;
            class_bytes.extend_from_slice(&num_methods.to_be_bytes());

            for bm in &bootstrap_methods {
                class_bytes.extend_from_slice(&bm.bootstrap_method_ref.to_be_bytes());
                let num_args =
                    checked_u16_count(bm.bootstrap_arguments.len(), "bootstrap method argument")?;
                class_bytes.extend_from_slice(&num_args.to_be_bytes());
                for arg in &bm.bootstrap_arguments {
                    class_bytes.extend_from_slice(&arg.to_be_bytes());
                }
            }
        } else {
            class_bytes.extend_from_slice(&[0x00, 0x00]);
        }

        // Add main class
        all_classes.insert(class_name.to_string(), class_bytes);

        // Add closure classes
        for (closure_name, closure_bytecode) in closure_classes {
            all_classes.insert(closure_name, closure_bytecode);
        }

        Ok(all_classes)
    }
}

impl Default for GroovyBytecodeProducer {
    fn default() -> Self {
        Self::new()
    }
}

impl BytecodeProducer for GroovyBytecodeProducer {
    fn parse(&self, source: &str, file_path: &str) -> BytecodeResult<()> {
        // In a full implementation, this would use tree-sitter-groovy
        // to parse the Groovy source into an AST
        //
        // For now, we validate basic structure
        if source.is_empty() {
            return Err(BytecodeError::ParseError {
                file: file_path.to_string(),
                line: 1,
                column: 1,
                message: "Empty Groovy source".to_string(),
            });
        }

        // Basic validation: check for balanced braces
        let mut brace_count = 0;
        for (i, ch) in source.char_indices() {
            match ch {
                '{' => brace_count += 1,
                '}' => {
                    brace_count -= 1;
                    if brace_count < 0 {
                        return Err(BytecodeError::ParseError {
                            file: file_path.to_string(),
                            line: source[..i].lines().count() as u32,
                            column: i as u32 % 100,
                            message: "Unbalanced closing brace".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }

        if brace_count != 0 {
            return Err(BytecodeError::ParseError {
                file: file_path.to_string(),
                line: source.lines().count() as u32,
                column: 1,
                message: "Unbalanced braces in Groovy source".to_string(),
            });
        }

        Ok(())
    }

    fn produce_bytes(&self, context: &CompilationContext) -> BytecodeResult<Vec<u8>> {
        let class_name = script_class_name(context);
        self.produce_all_classes(context)?
            .remove(class_name)
            .ok_or_else(|| BytecodeError::InternalError {
                message: format!("Groovy producer did not emit its main class {class_name}"),
            })
    }
    fn language(&self) -> JvmLanguage {
        JvmLanguage::Groovy
    }

    fn supported_features(&self) -> Vec<LanguageFeature> {
        vec![
            LanguageFeature::Closures,
            LanguageFeature::MetaObjectProtocol,
            LanguageFeature::GStrings,
            LanguageFeature::NamedParameters,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_groovy_producer_creation() {
        let producer = GroovyBytecodeProducer::new();
        assert_eq!(producer.language(), JvmLanguage::Groovy);
    }

    #[test]
    fn test_parse_empty_source() {
        let producer = GroovyBytecodeProducer::new();
        let result = producer.parse("", "test.groovy");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_valid_source() {
        let producer = GroovyBytecodeProducer::new();
        let result = producer.parse("println 'hello'", "test.groovy");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_unbalanced_braces() {
        let producer = GroovyBytecodeProducer::new();
        let result = producer.parse("{ { }", "test.groovy");
        assert!(result.is_err());
    }

    #[test]
    fn test_produce_bytes() {
        let producer = GroovyBytecodeProducer::new();
        let _ = producer.parse("println 'hello'", "test.groovy");

        let context = CompilationContext::default();
        let result = producer.produce_bytes(&context);
        assert!(result.is_ok());

        let bytes = result.unwrap();
        // Check magic number
        assert_eq!(&bytes[0..4], [0xCA, 0xFE, 0xBA, 0xBE]);
    }

    #[test]
    fn method_code_length_honors_the_jvm_boundary() {
        let largest_valid = vec![0; u16::MAX as usize];
        assert_eq!(
            checked_method_code_length("Script.run", &largest_valid).expect("valid code length"),
            u32::from(u16::MAX)
        );

        let oversized = vec![0; u16::MAX as usize + 1];
        assert!(matches!(
            checked_method_code_length("Script.run", &oversized),
            Err(BytecodeError::CodeSizeExceeded { method }) if method == "Script.run"
        ));
    }

    #[test]
    fn single_class_api_returns_main_class_from_closure_aware_compilation() {
        let producer = GroovyBytecodeProducer::new();
        let context = CompilationContext {
            source: Some("def captured = 42\nreturn { captured }".to_string()),
            source_path: Some("CustomScript.groovy".into()),
            ..CompilationContext::default()
        };

        let all_classes = producer
            .produce_all_classes(&context)
            .expect("closure-aware compilation");
        assert!(
            all_classes
                .keys()
                .any(|name| name.starts_with("Closure$_closure_")),
            "closure class must remain available from the multi-class API"
        );
        let main_class = producer.produce_bytes(&context).expect("main class");
        assert_eq!(
            main_class.as_slice(),
            all_classes
                .get("CustomScript")
                .expect("main class in multi-class output")
                .as_slice()
        );
    }

    #[test]
    fn nested_closure_output_is_reproducible() {
        let context = CompilationContext {
            source: Some(
                "def outer = { value -> { -> value } }\nreturn outer.call(7).call()".to_string(),
            ),
            source_path: Some("NestedScript.groovy".into()),
            ..CompilationContext::default()
        };

        let first = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect("first nested closure compilation");
        let second = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect("second nested closure compilation");

        assert_eq!(first, second);
        assert!(
            first
                .keys()
                .any(|name| name.contains("$_closure_0$_nested_0")),
            "nested closure class names should be scoped to their generated parent"
        );
    }

    #[test]
    fn generated_closure_stack_metadata_scales_with_its_body() {
        use cafebabe::attributes::AttributeData;

        let source = r#"
            def f = { target('a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l') }
            return f
        "#;
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("StackScript.groovy".into()),
            ..CompilationContext::default()
        };
        let classes = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect("closure compilation");
        let closure_bytes = classes
            .iter()
            .find_map(|(name, bytes)| name.starts_with("Closure$_closure_").then_some(bytes))
            .expect("generated closure class");
        let closure = cafebabe::parse_class(closure_bytes).expect("valid closure class");
        let code = closure
            .methods
            .iter()
            .find(|method| method.name == "doCall")
            .and_then(|method| {
                method
                    .attributes
                    .iter()
                    .find_map(|attribute| match &attribute.data {
                        AttributeData::Code(code) => Some(code),
                        _ => None,
                    })
            })
            .expect("closure doCall Code attribute");

        assert!(
            code.max_stack >= 13,
            "receiver plus twelve arguments require at least thirteen slots"
        );
    }

    #[test]
    fn test_supported_features() {
        let producer = GroovyBytecodeProducer::new();
        let features = producer.supported_features();
        assert!(features.contains(&LanguageFeature::Closures));
        assert!(features.contains(&LanguageFeature::MetaObjectProtocol));
        assert!(features.contains(&LanguageFeature::GStrings));
    }

    #[test]
    fn test_compile_integration() {
        let producer = GroovyBytecodeProducer::new();
        let context = CompilationContext::default();
        let result = producer.compile("println 'test'", "test.groovy", &context);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[0..4], [0xCA, 0xFE, 0xBA, 0xBE]);
    }
}

#[cfg(test)]
mod checked_count_tests {
    use super::checked_u16_count;

    #[test]
    fn rejects_counts_past_the_class_format_limit() {
        assert_eq!(checked_u16_count(65_535, "fields").unwrap(), u16::MAX);
        assert!(checked_u16_count(65_536, "fields").is_err());
    }
}
