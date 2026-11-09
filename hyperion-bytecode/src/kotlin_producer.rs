// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin evaluator bytecode producer.
//!
//! This is deliberately a small expression evaluator, not a general Kotlin
//! compiler. It emits a loadable `<File>Kt` class with a synthetic `__eval__`
//! method for the subset understood by [`crate::kotlin_converter`].

use crate::java_compiler::{Expression, Statement};
use crate::producer::{
    BytecodeError, BytecodeProducer, BytecodeResult, CompilationContext, JvmLanguage,
    LanguageFeature,
};
use parking_lot::Mutex;
use std::path::Path;

/// Extract class name from file path
/// e.g., "com/example/HelloWorld.kt" => "HelloWorld"
fn extract_class_name(file_path: &str) -> String {
    Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Main")
        .to_string()
}

/// Kotlin bytecode producer
pub struct KotlinBytecodeProducer {
    statements: Mutex<Vec<Statement>>,
    class_name: Mutex<String>,
}

impl KotlinBytecodeProducer {
    /// Create a new Kotlin bytecode producer
    pub fn new() -> Self {
        Self {
            statements: Mutex::new(Vec::new()),
            class_name: Mutex::new(String::new()),
        }
    }
}

impl Default for KotlinBytecodeProducer {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_evaluator_source(green_node: rowan::GreenNode) -> BytecodeResult<()> {
    use hyperion_kotlin::{SyntaxKind, SyntaxNode};

    let root = SyntaxNode::new_root(green_node);
    let functions: Vec<_> = root
        .children()
        .filter(|node| node.kind() == SyntaxKind::FUNCTION_DECL)
        .collect();
    let has_unsupported_declaration = root.children().any(|node| {
        matches!(
            node.kind(),
            SyntaxKind::CLASS_DECL
                | SyntaxKind::OBJECT_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::PROPERTY_DECL
        )
    });
    let function_name = functions.first().and_then(|function| {
        function
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::IDENT)
            .map(|token| token.text().to_string())
    });

    if functions.len() != 1
        || function_name.as_deref() != Some("__eval__")
        || has_unsupported_declaration
    {
        return Err(BytecodeError::UnsupportedFeature {
            feature:
                "general Kotlin compilation; only one top-level __eval__ function is supported"
                    .to_string(),
        });
    }

    Ok(())
}

impl BytecodeProducer for KotlinBytecodeProducer {
    fn language(&self) -> JvmLanguage {
        JvmLanguage::Kotlin
    }

    fn parse(&self, source: &str, file_path: &str) -> BytecodeResult<()> {
        // Use hyperion_kotlin parser to parse Kotlin source
        let parse_tree = hyperion_kotlin::parse(source);

        if !parse_tree.errors.is_empty() {
            return Err(BytecodeError::ParseError {
                file: file_path.to_string(),
                line: 0,
                column: 0,
                message: format!("Kotlin parse error: {:?}", parse_tree.errors),
            });
        }

        validate_evaluator_source(parse_tree.green_node.clone())?;

        // Convert Kotlin CST to statements using converter
        let statements = crate::kotlin_converter::convert_kotlin_source(parse_tree.green_node)?;

        // Extract class name
        let class_name = extract_class_name(file_path);

        // Store results
        *self.statements.lock() = statements;
        *self.class_name.lock() = class_name;

        Ok(())
    }

    fn produce_bytes(&self, _context: &CompilationContext) -> BytecodeResult<Vec<u8>> {
        let statements = self.statements.lock().clone();
        let class_name = format!("{}Kt", self.class_name.lock());
        let mut cp = crate::constant_pool::ConstantPoolBuilder::new();
        let class_index = cp.add_class(&class_name);
        let super_index = cp.add_class("java/lang/Object");
        let method_name_index = cp.add_utf8("__eval__");
        let method_descriptor_index = cp.add_utf8("()Ljava/lang/Object;");
        let code_attribute_index = cp.add_utf8("Code");
        let integer_value_of =
            cp.add_method_ref("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;");
        let boolean_value_of =
            cp.add_method_ref("java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;");

        let mut code = Vec::new();
        let mut max_stack = 1;
        if !compile_statements(
            &statements,
            &mut code,
            &mut cp,
            integer_value_of,
            boolean_value_of,
            &mut max_stack,
        )? {
            return Err(unit_value_refused());
        }
        if code.len() > u16::MAX as usize {
            return Err(BytecodeError::CodeSizeExceeded {
                method: "__eval__".to_string(),
            });
        }

        cp.validate()?;
        let entries = cp.get_entries();
        let mut class_bytes = Vec::new();
        class_bytes.extend_from_slice(&0xCAFEBABE_u32.to_be_bytes());
        class_bytes.extend_from_slice(&0_u16.to_be_bytes()); // minor
        class_bytes.extend_from_slice(&65_u16.to_be_bytes()); // major
        class_bytes.extend_from_slice(
            &crate::groovy_producer::checked_u16_count(entries.len() + 1, "constant pool")?
                .to_be_bytes(),
        );
        crate::groovy_producer::write_constant_pool_entries(&mut class_bytes, entries)?;
        class_bytes.extend_from_slice(&0x0021_u16.to_be_bytes());
        class_bytes.extend_from_slice(&class_index.to_be_bytes());
        class_bytes.extend_from_slice(&super_index.to_be_bytes());
        class_bytes.extend_from_slice(&0_u16.to_be_bytes());
        class_bytes.extend_from_slice(&0_u16.to_be_bytes());
        class_bytes.extend_from_slice(&1_u16.to_be_bytes());
        class_bytes.extend_from_slice(&0x0009_u16.to_be_bytes()); // public, static
        class_bytes.extend_from_slice(&method_name_index.to_be_bytes());
        class_bytes.extend_from_slice(&method_descriptor_index.to_be_bytes());
        class_bytes.extend_from_slice(&1_u16.to_be_bytes());
        class_bytes.extend_from_slice(&code_attribute_index.to_be_bytes());
        class_bytes.extend_from_slice(&(code.len() as u32 + 12).to_be_bytes());
        let max_stack = u16::try_from(max_stack).map_err(|_| BytecodeError::InternalError {
            message: "Kotlin evaluator operand stack exceeds the JVM limit".to_string(),
        })?;
        class_bytes.extend_from_slice(&max_stack.to_be_bytes());
        class_bytes.extend_from_slice(&0_u16.to_be_bytes()); // max_locals
        class_bytes.extend_from_slice(&(code.len() as u32).to_be_bytes());
        class_bytes.extend_from_slice(&code);
        class_bytes.extend_from_slice(&0_u16.to_be_bytes()); // exception_table_length
        class_bytes.extend_from_slice(&0_u16.to_be_bytes()); // attributes_count
        class_bytes.extend_from_slice(&0_u16.to_be_bytes());
        Ok(class_bytes)
    }

    fn supported_features(&self) -> Vec<LanguageFeature> {
        // The evaluator subset does not fully implement any of the Kotlin
        // language-wide capabilities represented by LanguageFeature.
        Vec::new()
    }
}

/// A snippet that yields no value yields `kotlin.Unit` in Kotlin. This backend has no `Unit`
/// instance, so such a snippet is refused, rather than returning `null` as if it were a value.
fn unit_value_refused() -> BytecodeError {
    BytecodeError::UnsupportedFeature {
        feature: "a Kotlin snippet with no value, which is kotlin.Unit".to_string(),
    }
}

/// Compile statements into the synthetic evaluator method. Returns whether a
/// return instruction was emitted.
fn compile_statements(
    statements: &[Statement],
    bytecode: &mut Vec<u8>,
    cp: &mut crate::constant_pool::ConstantPoolBuilder,
    integer_value_of: u16,
    boolean_value_of: u16,
    max_stack: &mut usize,
) -> BytecodeResult<bool> {
    for statement in statements {
        match statement {
            Statement::Expression(expression) => {
                let compiled = compile_expression(expression, bytecode, cp)?;
                *max_stack = (*max_stack).max(compiled.max_stack);
                bytecode.push(0x57); // pop
            }
            Statement::Return(expression) => {
                match expression {
                    Some(expression) => {
                        let compiled = compile_expression(expression, bytecode, cp)?;
                        *max_stack = (*max_stack).max(compiled.max_stack);
                        match compiled.kind {
                            KotlinValueKind::Int => {
                                bytecode.push(0xB8); // invokestatic Integer.valueOf
                                bytecode.extend_from_slice(&integer_value_of.to_be_bytes());
                            }
                            KotlinValueKind::Boolean => {
                                bytecode.push(0xB8); // invokestatic Boolean.valueOf
                                bytecode.extend_from_slice(&boolean_value_of.to_be_bytes());
                            }
                            KotlinValueKind::Reference => {}
                        }
                    }
                    None => return Err(unit_value_refused()),
                }
                bytecode.push(0xB0); // areturn
                return Ok(true);
            }
            Statement::Block(nested)
                if compile_statements(
                    nested,
                    bytecode,
                    cp,
                    integer_value_of,
                    boolean_value_of,
                    max_stack,
                )? =>
            {
                return Ok(true);
            }
            _ => {}
        }
    }
    Ok(false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KotlinValueKind {
    Int,
    Boolean,
    Reference,
}

struct CompiledExpression {
    kind: KotlinValueKind,
    max_stack: usize,
}

/// Compile an expression to JVM bytecode (reuse from Java)
fn compile_expression(
    expr: &Expression,
    bytecode: &mut Vec<u8>,
    cp: &mut crate::constant_pool::ConstantPoolBuilder,
) -> BytecodeResult<CompiledExpression> {
    use crate::java_compiler::{BinaryOp, Expression::*};

    match expr {
        IntLiteral(value) => {
            match *value {
                -1 => bytecode.push(0x02), // iconst_m1
                0 => bytecode.push(0x03),  // iconst_0
                1 => bytecode.push(0x04),  // iconst_1
                2 => bytecode.push(0x05),  // iconst_2
                3 => bytecode.push(0x06),  // iconst_3
                4 => bytecode.push(0x07),  // iconst_4
                5 => bytecode.push(0x08),  // iconst_5
                -128..=127 => {
                    bytecode.push(0x10); // bipush
                    bytecode.push(*value as u8);
                }
                -32768..=32767 => {
                    bytecode.push(0x11); // sipush
                    bytecode.extend_from_slice(&(*value as i16).to_be_bytes());
                }
                _ => emit_ldc(bytecode, cp.add_integer(*value)),
            }
            Ok(CompiledExpression {
                kind: KotlinValueKind::Int,
                max_stack: 1,
            })
        }
        BooleanLiteral(value) => {
            if *value {
                bytecode.push(0x04); // iconst_1
            } else {
                bytecode.push(0x03); // iconst_0
            }
            Ok(CompiledExpression {
                kind: KotlinValueKind::Boolean,
                max_stack: 1,
            })
        }
        StringLiteral(value) => {
            let index = cp.add_string(value);
            emit_ldc(bytecode, index);
            Ok(CompiledExpression {
                kind: KotlinValueKind::Reference,
                max_stack: 1,
            })
        }
        NullLiteral => {
            bytecode.push(0x01); // aconst_null
            Ok(CompiledExpression {
                kind: KotlinValueKind::Reference,
                max_stack: 1,
            })
        }
        BinaryExpr { left, op, right } => {
            let left = compile_expression(left, bytecode, cp)?;
            let right = compile_expression(right, bytecode, cp)?;
            if left.kind != KotlinValueKind::Int || right.kind != KotlinValueKind::Int {
                return Err(BytecodeError::UnsupportedFeature {
                    feature: "non-integer Kotlin binary expression".to_string(),
                });
            }

            match op {
                BinaryOp::Add => bytecode.push(0x60),      // iadd
                BinaryOp::Subtract => bytecode.push(0x64), // isub
                BinaryOp::Multiply => bytecode.push(0x68), // imul
                BinaryOp::Divide => bytecode.push(0x6C),   // idiv
                _ => {
                    return Err(BytecodeError::UnsupportedFeature {
                        feature: format!("Binary operator: {:?}", op),
                    })
                }
            }
            Ok(CompiledExpression {
                kind: KotlinValueKind::Int,
                max_stack: left.max_stack.max(1 + right.max_stack),
            })
        }
        UnaryExpr { op, operand } => {
            let operand = compile_expression(operand, bytecode, cp)?;
            match (op, operand.kind) {
                (crate::java_compiler::UnaryOp::Negate, KotlinValueKind::Int) => {
                    bytecode.push(0x74); // ineg
                    Ok(operand)
                }
                (crate::java_compiler::UnaryOp::LogicalNot, KotlinValueKind::Boolean) => {
                    bytecode.push(0x04); // iconst_1
                    bytecode.push(0x82); // ixor
                    Ok(CompiledExpression {
                        kind: KotlinValueKind::Boolean,
                        max_stack: operand.max_stack.max(2),
                    })
                }
                _ => Err(BytecodeError::UnsupportedFeature {
                    feature: format!("Kotlin unary expression: {expr:?}"),
                }),
            }
        }
        _ => Err(BytecodeError::UnsupportedFeature {
            feature: format!("Expression type: {:?}", expr),
        }),
    }
}

fn emit_ldc(bytecode: &mut Vec<u8>, index: u16) {
    if let Ok(index) = u8::try_from(index) {
        bytecode.push(0x12); // ldc
        bytecode.push(index);
    } else {
        bytecode.push(0x13); // ldc_w
        bytecode.extend_from_slice(&index.to_be_bytes());
    }
}
