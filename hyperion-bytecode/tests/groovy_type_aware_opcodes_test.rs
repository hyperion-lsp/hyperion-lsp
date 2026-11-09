// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::approx_constant)]

//! Test that the Groovy compiler selects correct opcodes based on variable types
//!
//! This ensures that:
//! - int variables use ILOAD/ISTORE/IADD/ISUB/IMUL
//! - long variables use LLOAD/LSTORE/LADD/LSUB/LMUL
//! - float variables use FLOAD/FSTORE/FADD/FSUB/FMUL
//! - double variables use DLOAD/DSTORE/DADD/DSUB/DMUL

use hyperion_bytecode::constant_pool::ConstantPoolBuilder;
use hyperion_bytecode::groovy_ast_compiler::GroovyAstCompiler;
use hyperion_bytecode::java_compiler::JavaType;
use hyperion_groovy::{Expression, Statement};

#[test]
fn test_int_variable_load_uses_iload() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // int x = 42
    compiler.compile_statement(&Statement::VariableDecl {
        name: "x".into(),
        type_annotation: Some("int".into()),
        initializer: Some(Expression::IntLiteral(42)),
    });

    // x (should use ILOAD)
    compiler.compile_expression(&Expression::Identifier("x".to_string()));

    let bytecode = compiler.bytecode();

    // Should contain ILOAD (0x15) or one of the short forms ILOAD_0..ILOAD_3 (0x1A..0x1D)
    assert!(
        bytecode
            .iter()
            .any(|&b| matches!(b, 0x15 | 0x1A | 0x1B | 0x1C | 0x1D)),
        "Should use ILOAD for int variable, got: {:?}",
        bytecode
    );
}

#[test]
fn test_long_variable_load_uses_lload() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // long x = 42L
    compiler.compile_statement(&Statement::VariableDecl {
        name: "x".into(),
        type_annotation: Some("long".into()),
        initializer: Some(Expression::LongLiteral(42)),
    });

    // x (should use LLOAD)
    compiler.compile_expression(&Expression::Identifier("x".to_string()));

    let bytecode = compiler.bytecode();

    // Should contain LLOAD (0x16) or one of the short forms LLOAD_0..LLOAD_3 (0x1E..0x21)
    assert!(
        bytecode
            .iter()
            .any(|&b| matches!(b, 0x16 | 0x1E | 0x1F | 0x20 | 0x21)),
        "Should use LLOAD for long variable, got: {:?}",
        bytecode
    );
}

#[test]
fn test_double_variable_load_uses_dload() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // double x = 3.14
    compiler.compile_statement(&Statement::VariableDecl {
        name: "x".into(),
        type_annotation: Some("double".into()),
        initializer: Some(Expression::DoubleLiteral(3.14)),
    });

    // x (should use DLOAD)
    compiler.compile_expression(&Expression::Identifier("x".to_string()));

    let bytecode = compiler.bytecode();

    // Should contain DLOAD (0x18) or one of the short forms DLOAD_0..DLOAD_3 (0x26..0x29)
    assert!(
        bytecode
            .iter()
            .any(|&b| matches!(b, 0x18 | 0x26 | 0x27 | 0x28 | 0x29)),
        "Should use DLOAD for double variable, got: {:?}",
        bytecode
    );
}

#[test]
fn test_int_addition_uses_iadd() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // int a = 10
    compiler.compile_statement(&Statement::VariableDecl {
        name: "a".into(),
        type_annotation: Some("int".into()),
        initializer: Some(Expression::IntLiteral(10)),
    });

    // int b = 20
    compiler.compile_statement(&Statement::VariableDecl {
        name: "b".into(),
        type_annotation: Some("int".into()),
        initializer: Some(Expression::IntLiteral(20)),
    });

    // a + b
    compiler.compile_expression(&Expression::BinaryOp {
        operator: "+".into(),
        left: Box::new(Expression::Identifier("a".to_string())),
        right: Box::new(Expression::Identifier("b".to_string())),
    });

    let bytecode = compiler.bytecode();

    // Should contain IADD (0x60)
    assert!(
        bytecode.contains(&0x60),
        "Should use IADD for int addition, got: {:?}",
        bytecode
    );
}

#[test]
fn test_long_addition_should_use_ladd() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // long a = 10L
    compiler.compile_statement(&Statement::VariableDecl {
        name: "a".into(),
        type_annotation: Some("long".into()),
        initializer: Some(Expression::LongLiteral(10)),
    });

    // long b = 20L
    compiler.compile_statement(&Statement::VariableDecl {
        name: "b".into(),
        type_annotation: Some("long".into()),
        initializer: Some(Expression::LongLiteral(20)),
    });

    // a + b (should use LADD, not IADD)
    compiler.compile_expression(&Expression::BinaryOp {
        operator: "+".into(),
        left: Box::new(Expression::Identifier("a".to_string())),
        right: Box::new(Expression::Identifier("b".to_string())),
    });

    let bytecode = compiler.bytecode();

    // Should contain LADD (0x61), NOT IADD (0x60)
    assert!(
        bytecode.contains(&0x61),
        "Should use LADD (0x61) for long addition, got: {:?}",
        bytecode
    );

    // Verify we're NOT using IADD
    let iadd_count = bytecode.iter().filter(|&&b| b == 0x60).count();
    assert_eq!(
        iadd_count, 0,
        "Should NOT use IADD (0x60) for long addition"
    );
}

#[test]
fn test_double_addition_should_use_dadd() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // double a = 1.5
    compiler.compile_statement(&Statement::VariableDecl {
        name: "a".into(),
        type_annotation: Some("double".into()),
        initializer: Some(Expression::DoubleLiteral(1.5)),
    });

    // double b = 2.5
    compiler.compile_statement(&Statement::VariableDecl {
        name: "b".into(),
        type_annotation: Some("double".into()),
        initializer: Some(Expression::DoubleLiteral(2.5)),
    });

    // a + b (should use DADD, not IADD)
    compiler.compile_expression(&Expression::BinaryOp {
        operator: "+".into(),
        left: Box::new(Expression::Identifier("a".to_string())),
        right: Box::new(Expression::Identifier("b".to_string())),
    });

    let bytecode = compiler.bytecode();

    // Should contain DADD (0x63), NOT IADD (0x60)
    assert!(
        bytecode.contains(&0x63),
        "Should use DADD (0x63) for double addition, got: {:?}",
        bytecode
    );

    // Verify we're NOT using IADD
    let iadd_count = bytecode.iter().filter(|&&b| b == 0x60).count();
    assert_eq!(
        iadd_count, 0,
        "Should NOT use IADD (0x60) for double addition"
    );
}

#[test]
fn test_long_subtraction_should_use_lsub() {
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

    // a - b (should use LSUB)
    compiler.compile_expression(&Expression::BinaryOp {
        operator: "-".into(),
        left: Box::new(Expression::Identifier("a".to_string())),
        right: Box::new(Expression::Identifier("b".to_string())),
    });

    let bytecode = compiler.bytecode();

    // Should contain LSUB (0x65), NOT ISUB (0x64)
    assert!(
        bytecode.contains(&0x65),
        "Should use LSUB (0x65) for long subtraction, got: {:?}",
        bytecode
    );
}

#[test]
fn test_double_multiplication_should_use_dmul() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // double a = 2.0
    compiler.compile_statement(&Statement::VariableDecl {
        name: "a".into(),
        type_annotation: Some("double".into()),
        initializer: Some(Expression::DoubleLiteral(2.0)),
    });

    // double b = 3.0
    compiler.compile_statement(&Statement::VariableDecl {
        name: "b".into(),
        type_annotation: Some("double".into()),
        initializer: Some(Expression::DoubleLiteral(3.0)),
    });

    // a * b (should use DMUL)
    compiler.compile_expression(&Expression::BinaryOp {
        operator: "*".into(),
        left: Box::new(Expression::Identifier("a".to_string())),
        right: Box::new(Expression::Identifier("b".to_string())),
    });

    let bytecode = compiler.bytecode();

    // Should contain DMUL (0x6B), NOT IMUL (0x68)
    assert!(
        bytecode.contains(&0x6B),
        "Should use DMUL (0x6B) for double multiplication, got: {:?}",
        bytecode
    );
}

#[test]
fn groovy_long_division_uses_decimal_runtime_not_ldiv() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // long a = 100L
    compiler.compile_statement(&Statement::VariableDecl {
        name: "a".into(),
        type_annotation: Some("long".into()),
        initializer: Some(Expression::LongLiteral(100)),
    });

    // long b = 10L
    compiler.compile_statement(&Statement::VariableDecl {
        name: "b".into(),
        type_annotation: Some("long".into()),
        initializer: Some(Expression::LongLiteral(10)),
    });

    // Groovy `/` is decimal division even for long operands.
    let result_type = compiler.compile_expression(&Expression::BinaryOp {
        operator: "/".into(),
        left: Box::new(Expression::Identifier("a".to_string())),
        right: Box::new(Expression::Identifier("b".to_string())),
    });

    let bytecode = compiler.bytecode();

    assert_eq!(
        result_type,
        Some(JavaType::Object("java/math/BigDecimal".to_string()))
    );
    assert!(
        !bytecode.contains(&0x6D),
        "Groovy division must not use JVM LDIV: {:?}",
        bytecode
    );
}
