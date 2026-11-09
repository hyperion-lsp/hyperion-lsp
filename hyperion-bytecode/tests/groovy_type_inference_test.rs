// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::approx_constant)]

//! Test type inference for variable declarations without explicit type annotations
//!
//! Groovy supports var-like type inference where the type is inferred from the initializer:
//! - def x = 42      → int
//! - def x = 42L     → long
//! - def x = 3.14    → double
//! - def x = "hello" → String

use hyperion_bytecode::constant_pool::ConstantPoolBuilder;
use hyperion_bytecode::groovy_ast_compiler::GroovyAstCompiler;
use hyperion_groovy::{Expression, Statement};

#[test]
fn test_infer_int_from_int_literal() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // def x = 42 (no type annotation, infer from initializer)
    compiler.compile_statement(&Statement::VariableDecl {
        name: "x".into(),
        type_annotation: None,
        initializer: Some(Expression::IntLiteral(42)),
    });

    // x + 10 (should use IADD)
    compiler.compile_expression(&Expression::BinaryOp {
        operator: "+".into(),
        left: Box::new(Expression::Identifier("x".to_string())),
        right: Box::new(Expression::IntLiteral(10)),
    });

    let bytecode = compiler.bytecode();

    // Should use ILOAD for x and IADD for addition
    assert!(
        bytecode.contains(&0x60),
        "Should use IADD for int addition, got: {:?}",
        bytecode
    );
}

#[test]
fn test_infer_long_from_long_literal() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // def x = 42L (no type annotation, infer long from literal)
    compiler.compile_statement(&Statement::VariableDecl {
        name: "x".into(),
        type_annotation: None,
        initializer: Some(Expression::LongLiteral(42)),
    });

    // x + 10L (should use LADD)
    compiler.compile_expression(&Expression::BinaryOp {
        operator: "+".into(),
        left: Box::new(Expression::Identifier("x".to_string())),
        right: Box::new(Expression::LongLiteral(10)),
    });

    let bytecode = compiler.bytecode();

    // Should use LLOAD for x and LADD for addition
    assert!(
        bytecode.contains(&0x61),
        "Should use LADD for long addition, got: {:?}",
        bytecode
    );
}

#[test]
fn test_infer_double_from_double_literal() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // def x = 3.14 (no type annotation, infer double from literal)
    compiler.compile_statement(&Statement::VariableDecl {
        name: "x".into(),
        type_annotation: None,
        initializer: Some(Expression::DoubleLiteral(3.14)),
    });

    // x * 2.0 (should use DMUL)
    compiler.compile_expression(&Expression::BinaryOp {
        operator: "*".into(),
        left: Box::new(Expression::Identifier("x".to_string())),
        right: Box::new(Expression::DoubleLiteral(2.0)),
    });

    let bytecode = compiler.bytecode();

    // Should use DLOAD for x and DMUL for multiplication
    assert!(
        bytecode.contains(&0x6B),
        "Should use DMUL for double multiplication, got: {:?}",
        bytecode
    );
}

#[test]
fn test_infer_string_from_string_literal() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // def x = "hello" (no type annotation, infer String from literal)
    let result = compiler.compile_statement(&Statement::VariableDecl {
        name: "x".into(),
        type_annotation: None,
        initializer: Some(Expression::StringLiteral("hello".to_string())),
    });

    // x (should use ALOAD for String)
    compiler.compile_expression(&Expression::Identifier("x".to_string()));

    let bytecode = compiler.bytecode();

    // Should use ALOAD (0x19) or one of the short forms ALOAD_0..ALOAD_3 (0x2A..0x2D)
    assert!(
        bytecode
            .iter()
            .any(|&b| matches!(b, 0x19 | 0x2A | 0x2B | 0x2C | 0x2D)),
        "Should use ALOAD for String variable, got: {:?}",
        bytecode
    );

    // Verify the inferred type is String
    use hyperion_bytecode::java_compiler::JavaType;
    assert!(
        matches!(result, Some(JavaType::Object(ref s)) if s == "java/lang/String"),
        "Should infer String type from string literal"
    );
}

#[test]
fn test_complex_expression_with_mixed_types() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // long a = 100L
    compiler.compile_statement(&Statement::VariableDecl {
        name: "a".into(),
        type_annotation: Some("long".into()),
        initializer: Some(Expression::LongLiteral(100)),
    });

    // long b = 50L
    compiler.compile_statement(&Statement::VariableDecl {
        name: "b".into(),
        type_annotation: Some("long".into()),
        initializer: Some(Expression::LongLiteral(50)),
    });

    // (a - b) * 2L
    compiler.compile_expression(&Expression::BinaryOp {
        operator: "*".into(),
        left: Box::new(Expression::BinaryOp {
            operator: "-".into(),
            left: Box::new(Expression::Identifier("a".to_string())),
            right: Box::new(Expression::Identifier("b".to_string())),
        }),
        right: Box::new(Expression::LongLiteral(2)),
    });

    let bytecode = compiler.bytecode();

    // Should contain LSUB (0x65) for subtraction
    assert!(
        bytecode.contains(&0x65),
        "Should use LSUB for long subtraction"
    );

    // Should contain LMUL (0x69) for multiplication
    assert!(
        bytecode.contains(&0x69),
        "Should use LMUL for long multiplication"
    );
}

#[test]
fn test_unary_negation_with_different_types() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // long x = 100L
    compiler.compile_statement(&Statement::VariableDecl {
        name: "x".into(),
        type_annotation: Some("long".into()),
        initializer: Some(Expression::LongLiteral(100)),
    });

    // -x (should use LNEG)
    compiler.compile_expression(&Expression::UnaryOp {
        operator: "-".into(),
        operand: Box::new(Expression::Identifier("x".to_string())),
    });

    let bytecode = compiler.bytecode();

    // Should contain LNEG (0x75)
    assert!(
        bytecode.contains(&0x75),
        "Should use LNEG for long negation, got: {:?}",
        bytecode
    );
}

#[test]
fn test_float_operations() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // float x = 2.5f
    compiler.compile_statement(&Statement::VariableDecl {
        name: "x".into(),
        type_annotation: Some("float".into()),
        initializer: Some(Expression::FloatLiteral(2.5)),
    });

    // float y = 1.5f
    compiler.compile_statement(&Statement::VariableDecl {
        name: "y".into(),
        type_annotation: Some("float".into()),
        initializer: Some(Expression::FloatLiteral(1.5)),
    });

    // x + y (should use FADD)
    compiler.compile_expression(&Expression::BinaryOp {
        operator: "+".into(),
        left: Box::new(Expression::Identifier("x".to_string())),
        right: Box::new(Expression::Identifier("y".to_string())),
    });

    let bytecode = compiler.bytecode();

    // Should contain FADD (0x62)
    assert!(
        bytecode.contains(&0x62),
        "Should use FADD for float addition, got: {:?}",
        bytecode
    );
}

#[test]
fn test_double_negation() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // double x = 3.14
    compiler.compile_statement(&Statement::VariableDecl {
        name: "x".into(),
        type_annotation: Some("double".into()),
        initializer: Some(Expression::DoubleLiteral(3.14)),
    });

    // -x (should use DNEG)
    compiler.compile_expression(&Expression::UnaryOp {
        operator: "-".into(),
        operand: Box::new(Expression::Identifier("x".to_string())),
    });

    let bytecode = compiler.bytecode();

    // Should contain DNEG (0x77)
    assert!(
        bytecode.contains(&0x77),
        "Should use DNEG for double negation, got: {:?}",
        bytecode
    );
}
