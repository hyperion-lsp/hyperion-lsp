// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for JVM-spec-compliant finally block execution in the Groovy bytecode compiler.
//!
//! The finally block must execute on ALL exit paths:
//! 1. Normal completion of try block
//! 2. Normal completion of catch blocks
//! 3. Uncaught exception (from try or catch)
//! 4. Early return (in try or catch)
//! 5. Break/continue (when try/catch is inside a loop)

use hyperion_bytecode::constant_pool::ConstantPoolBuilder;
use hyperion_bytecode::groovy_ast_compiler::GroovyAstCompiler;
use hyperion_bytecode::java_compiler::JavaType;
use hyperion_bytecode::producer::BytecodeError;
use hyperion_groovy::{CatchBlock, Expression, Statement};

/// Test that finally block is compiled after normal try block completion
#[test]
fn test_finally_normal_completion() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // try { 42 } finally { 99 }
    let stmt = Statement::Try {
        body: vec![Statement::Expression(Expression::IntLiteral(42))],
        catch_blocks: vec![],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    assert!(!bytecode.is_empty(), "Should emit bytecode");

    // Should contain BIPUSH or ICONST for both 42 and 99
    // 42 is in range for BIPUSH (0x10 42)
    // 99 is in range for BIPUSH (0x10 99)
    // The finally block should be inlined after the try body
    let bytecode_slice: &[u8] = bytecode;
    assert!(
        bytecode_slice.windows(2).any(|w| w == [0x10, 42]),
        "Should contain BIPUSH 42 from try block"
    );
    assert!(
        bytecode_slice.windows(2).any(|w| w == [0x10, 99]),
        "Should contain BIPUSH 99 from finally block"
    );
}

/// Test that finally block executes before return in try block
#[test]
fn test_finally_return_from_try() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Set return type to int so return value slot is allocated
    compiler.set_method_return_type(JavaType::Int);

    // try { return 42 } finally { 99 }
    let stmt = Statement::Try {
        body: vec![Statement::Return(Some(Expression::IntLiteral(42)))],
        catch_blocks: vec![],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    assert!(!bytecode.is_empty(), "Should emit bytecode");

    // The bytecode should:
    // 1. Push 42
    // 2. Store it (ISTORE)
    // 3. Execute finally (push 99, POP)
    // 4. Load stored value
    // 5. Return

    // Check for presence of key opcodes
    let bytecode_slice: &[u8] = bytecode;
    // Should have BIPUSH 42
    assert!(
        bytecode_slice.windows(2).any(|w| w == [0x10, 42]),
        "Should contain BIPUSH 42 for return value"
    );
    // Should have BIPUSH 99 (finally)
    assert!(
        bytecode_slice.windows(2).any(|w| w == [0x10, 99]),
        "Should contain BIPUSH 99 from finally block"
    );
    // Should have IRETURN (0xAC)
    assert!(
        bytecode_slice.contains(&0xAC),
        "Should contain IRETURN for int return"
    );
}

#[test]
fn test_finally_adapts_return_to_method_descriptor() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    compiler.set_method_return_type(JavaType::Long);

    compiler.compile_statement(&Statement::Try {
        body: vec![Statement::Return(Some(Expression::IntLiteral(42)))],
        catch_blocks: vec![],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
    });

    assert!(
        compiler.bytecode().contains(&0x85),
        "return must widen with I2L"
    );
    assert!(
        compiler.bytecode().contains(&0xAD),
        "long method must use LRETURN"
    );
    compiler
        .validate()
        .expect("adapted finally return should validate");
}

/// Test that finally block executes before return in catch block
#[test]
fn test_finally_return_from_catch() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Set return type to int
    compiler.set_method_return_type(JavaType::Int);

    // try { throw new Exception() } catch (Exception e) { return 42 } finally { 99 }
    let stmt = Statement::Try {
        body: vec![Statement::Expression(Expression::IntLiteral(1))], // Placeholder for throw
        catch_blocks: vec![CatchBlock {
            exception_type: "java/lang/Exception".into(),
            variable_name: "e".into(),
            body: vec![Statement::Return(Some(Expression::IntLiteral(42)))],
        }],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    assert!(!bytecode.is_empty(), "Should emit bytecode");

    // Should generate catch handler with finally inlined before return
    let bytecode_slice: &[u8] = bytecode;
    assert!(
        bytecode_slice.windows(2).any(|w| w == [0x10, 99]),
        "Should contain BIPUSH 99 from finally block in catch path"
    );
}

/// Test that uncaught exceptions trigger finally and re-throw
#[test]
fn test_finally_uncaught_exception() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // try { 42 } finally { 99 }
    // An uncaught exception should run finally then re-throw
    let stmt = Statement::Try {
        body: vec![Statement::Expression(Expression::IntLiteral(42))],
        catch_blocks: vec![],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    assert!(!bytecode.is_empty(), "Should emit bytecode");

    // Should have ATHROW (0xBF) for re-throwing after finally
    let bytecode_slice: &[u8] = bytecode;
    assert!(
        bytecode_slice.contains(&0xBF),
        "Should contain ATHROW for re-throwing uncaught exception"
    );

    // Should have exception handlers
    let handlers = compiler.exception_handlers();
    assert!(
        handlers.iter().any(|h| h.catch_type.is_none()),
        "Should have catch-all handler for finally"
    );
}

/// Test that finally executes before break inside a loop
#[test]
fn test_finally_break_in_loop() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // while (true) { try { break } finally { 99 } }
    let stmt = Statement::While {
        condition: Expression::BooleanLiteral(true),
        body: vec![Statement::Try {
            body: vec![Statement::Break],
            catch_blocks: vec![],
            finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
        }],
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    assert!(!bytecode.is_empty(), "Should emit bytecode");

    // The finally block (99) should be compiled before the break jump
    let bytecode_slice: &[u8] = bytecode;
    assert!(
        bytecode_slice.windows(2).any(|w| w == [0x10, 99]),
        "Should contain BIPUSH 99 from finally block before break"
    );
    // Should have GOTO (0xA7) for the break (after finally)
    assert!(
        bytecode_slice.contains(&0xA7),
        "Should contain GOTO for break after finally"
    );
}

/// Test that finally executes before continue inside a loop
#[test]
fn test_finally_continue_in_loop() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // while (true) { try { continue } finally { 99 } }
    let stmt = Statement::While {
        condition: Expression::BooleanLiteral(true),
        body: vec![Statement::Try {
            body: vec![Statement::Continue],
            catch_blocks: vec![],
            finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
        }],
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    assert!(!bytecode.is_empty(), "Should emit bytecode");

    // The finally block (99) should be compiled before the continue jump
    let bytecode_slice: &[u8] = bytecode;
    assert!(
        bytecode_slice.windows(2).any(|w| w == [0x10, 99]),
        "Should contain BIPUSH 99 from finally block before continue"
    );
    // Should have multiple GOTO instructions (for continue and loop)
    let goto_count = bytecode_slice.iter().filter(|&&b| b == 0xA7).count();
    assert!(
        goto_count >= 2,
        "Should have GOTO for continue and loop iteration"
    );
}

/// Test nested try-finally blocks
#[test]
fn test_nested_finally() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // try { try { 1 } finally { 2 } } finally { 3 }
    let inner_try = Statement::Try {
        body: vec![Statement::Expression(Expression::IntLiteral(1))],
        catch_blocks: vec![],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(2))]),
    };

    let outer_try = Statement::Try {
        body: vec![inner_try],
        catch_blocks: vec![],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(3))]),
    };

    compiler.compile_statement(&outer_try);

    let bytecode = compiler.bytecode();
    assert!(!bytecode.is_empty(), "Should emit bytecode");

    // Should contain all three values
    let bytecode_slice: &[u8] = bytecode;
    // 1 is ICONST_1 (0x04)
    assert!(
        bytecode_slice.contains(&0x04),
        "Should contain ICONST_1 from inner try"
    );
    // 2 is ICONST_2 (0x05)
    assert!(
        bytecode_slice.contains(&0x05),
        "Should contain ICONST_2 from inner finally"
    );
    // 3 is ICONST_3 (0x06)
    assert!(
        bytecode_slice.contains(&0x06),
        "Should contain ICONST_3 from outer finally"
    );
}

/// Test void return with finally
#[test]
fn test_finally_void_return() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Set return type to void
    compiler.set_method_return_type(JavaType::Void);

    // try { return } finally { 99 }
    let stmt = Statement::Try {
        body: vec![Statement::Return(None)],
        catch_blocks: vec![],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    assert!(!bytecode.is_empty(), "Should emit bytecode");

    let bytecode_slice: &[u8] = bytecode;
    // Should have BIPUSH 99 (finally)
    assert!(
        bytecode_slice.windows(2).any(|w| w == [0x10, 99]),
        "Should contain BIPUSH 99 from finally block"
    );
    // Should have RETURN (0xB1) for void return
    assert!(
        bytecode_slice.contains(&0xB1),
        "Should contain RETURN for void return"
    );
}

#[test]
fn test_finally_rejects_wide_value_return_from_void_method() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    compiler.set_method_return_type(JavaType::Void);

    compiler.compile_statement(&Statement::Try {
        body: vec![Statement::Return(Some(Expression::LongLiteral(
            2_147_483_648,
        )))],
        catch_blocks: vec![],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
    });

    assert!(
        compiler.bytecode().contains(&0x58),
        "wide rejected value must be discarded with POP2"
    );
    assert!(matches!(
        compiler.validate(),
        Err(BytecodeError::UnsupportedFeature { feature })
            if feature.contains("void Groovy method cannot return a value")
    ));
}

/// Test try-catch-finally all paths
#[test]
fn test_try_catch_finally_all_paths() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // try { 1 } catch (Exception e) { 2 } finally { 3 }
    let stmt = Statement::Try {
        body: vec![Statement::Expression(Expression::IntLiteral(1))],
        catch_blocks: vec![CatchBlock {
            exception_type: "java/lang/Exception".into(),
            variable_name: "e".into(),
            body: vec![Statement::Expression(Expression::IntLiteral(2))],
        }],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(3))]),
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    let bytecode_slice: &[u8] = bytecode;

    // Try body: ICONST_1
    assert!(
        bytecode_slice.contains(&0x04),
        "Should contain ICONST_1 from try"
    );
    // Catch body: ICONST_2
    assert!(
        bytecode_slice.contains(&0x05),
        "Should contain ICONST_2 from catch"
    );
    // Finally: ICONST_3
    assert!(
        bytecode_slice.contains(&0x06),
        "Should contain ICONST_3 from finally"
    );

    // Should have exception handlers
    let handlers = compiler.exception_handlers();
    // One for the typed catch, and two catch-all for finally (try and catch regions)
    assert!(
        handlers.len() >= 2,
        "Should have at least 2 exception handlers"
    );
    // Should have at least one typed handler
    assert!(
        handlers.iter().any(|h| h.catch_type.is_some()),
        "Should have typed catch handler"
    );
    // Should have at least one catch-all handler
    assert!(
        handlers.iter().any(|h| h.catch_type.is_none()),
        "Should have catch-all handler for finally"
    );
}

/// Test that return value from catch is preserved through finally
/// Regression test: catch block return should store value, run finally once, then return
#[test]
fn test_finally_return_from_catch_preserves_value() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Set return type to int
    compiler.set_method_return_type(JavaType::Int);

    // try { 1 } catch (Exception e) { return 42 } finally { 99 }
    // The return value (42) must be preserved after finally executes
    let stmt = Statement::Try {
        body: vec![Statement::Expression(Expression::IntLiteral(1))],
        catch_blocks: vec![CatchBlock {
            exception_type: "java/lang/Exception".into(),
            variable_name: "e".into(),
            body: vec![Statement::Return(Some(Expression::IntLiteral(42)))],
        }],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    let bytecode_slice: &[u8] = bytecode;

    // Should have BIPUSH 42 for the return value
    assert!(
        bytecode_slice.windows(2).any(|w| w == [0x10, 42]),
        "Should contain BIPUSH 42 for return value from catch"
    );

    // Should have BIPUSH 99 for finally
    assert!(
        bytecode_slice.windows(2).any(|w| w == [0x10, 99]),
        "Should contain BIPUSH 99 from finally block"
    );

    // Should have ISTORE or istore_N (0x3B-0x3E for slots 0-3, 0x36 for general)
    // to save return value before finally
    let has_istore = bytecode_slice.contains(&0x36)  // istore
        || bytecode_slice.contains(&0x3B)  // istore_0
        || bytecode_slice.contains(&0x3C)  // istore_1
        || bytecode_slice.contains(&0x3D)  // istore_2
        || bytecode_slice.contains(&0x3E); // istore_3
    assert!(
        has_istore,
        "Should contain ISTORE/istore_N to save return value before finally"
    );

    // Should have ILOAD or iload_N (0x1A-0x1D for slots 0-3, 0x15 for general)
    // to reload return value after finally
    let has_iload = bytecode_slice.contains(&0x15)  // iload
        || bytecode_slice.contains(&0x1A)  // iload_0
        || bytecode_slice.contains(&0x1B)  // iload_1
        || bytecode_slice.contains(&0x1C)  // iload_2
        || bytecode_slice.contains(&0x1D); // iload_3
    assert!(
        has_iload,
        "Should contain ILOAD/iload_N to reload return value after finally"
    );

    // Should have IRETURN (0xAC)
    assert!(bytecode_slice.contains(&0xAC), "Should contain IRETURN");
}

/// Test that finally does NOT execute twice when catch block returns
/// Regression test: the Return statement in catch compiles inline finally,
/// so the catch's auto-finally should be skipped
#[test]
fn test_finally_no_double_execution() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Set return type to int
    compiler.set_method_return_type(JavaType::Int);

    // try { 1 } catch (Exception e) { return 42 } finally { 99 }
    // The finally block (BIPUSH 99) should appear exactly twice:
    // 1. After try normal completion (inline)
    // 2. Inside catch's return handling (inline)
    // It should NOT appear a third time after catch body
    let stmt = Statement::Try {
        body: vec![Statement::Expression(Expression::IntLiteral(1))],
        catch_blocks: vec![CatchBlock {
            exception_type: "java/lang/Exception".into(),
            variable_name: "e".into(),
            body: vec![Statement::Return(Some(Expression::IntLiteral(42)))],
        }],
        finally_block: Some(vec![Statement::Expression(Expression::IntLiteral(99))]),
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    let bytecode_slice: &[u8] = bytecode;

    // Count occurrences of BIPUSH 99 (0x10, 0x63)
    // Should be exactly 3: try path, catch return path, and catch-all handler
    // NOT 4 (which would indicate double execution in catch)
    let finally_count = bytecode_slice
        .windows(2)
        .filter(|w| *w == [0x10, 99])
        .count();

    // With the fix: try path (1) + catch return path (1) + catch-all path (1) = 3
    // Without the fix (bug): try path (1) + catch return path (1) + catch auto-finally (1) + catch-all path (1) = 4
    assert!(
        finally_count <= 3,
        "Finally block should NOT be duplicated. Found {} occurrences of BIPUSH 99, expected <= 3",
        finally_count
    );
}
