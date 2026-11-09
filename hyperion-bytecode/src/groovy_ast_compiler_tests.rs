// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;

#[test]
fn test_compile_int_literal() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    let expr = Expression::IntLiteral(42);
    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Int));
    assert!(!compiler.bytecode().is_empty());
}

#[test]
fn declared_reference_types_are_canonicalized_and_erased() {
    for (annotation, expected) in [
        (
            "java.lang.String",
            JavaType::Object("java/lang/String".into()),
        ),
        (
            "java.util.List<String>",
            JavaType::Object("java/util/List".into()),
        ),
        (
            "String[][]",
            JavaType::Array {
                element_type: Box::new(JavaType::Object("java/lang/String".into())),
                dimensions: 2,
            },
        ),
    ] {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyAstCompiler::new(cp);
        let result = compiler.compile_statement(&Statement::VariableDecl {
            name: "value".to_string(),
            type_annotation: Some(annotation.to_string()),
            initializer: None,
        });

        assert_eq!(result, Some(expected), "annotation: {annotation}");
        compiler.validate().expect("canonical type must validate");
    }
}

#[test]
fn malformed_declared_type_is_rejected_before_its_initializer() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    let declaration = Statement::VariableDecl {
        name: "value".to_string(),
        type_annotation: Some("java.util.List<String".to_string()),
        initializer: Some(Expression::IntLiteral(7)),
    };

    assert!(compiler.compile_statement(&declaration).is_none());
    assert!(compiler.bytecode().is_empty());
    assert!(matches!(
        compiler.validate(),
        Err(BytecodeError::UnsupportedFeature { feature })
            if feature.contains("malformed Groovy type annotation")
    ));
}

#[test]
fn oversized_dynamic_call_is_rejected_before_emitting_operands() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    let call = Expression::MethodCall {
        receiver: None,
        method_name: "oversized".to_string(),
        arguments: vec![Expression::NullLiteral; 255],
    };

    assert!(compiler.compile_expression(&call).is_none());
    assert!(compiler.bytecode().is_empty());
    assert!(matches!(
        compiler.validate(),
        Err(BytecodeError::UnsupportedFeature { feature })
            if feature.contains("at most 254 arguments plus the receiver")
    ));
}

#[test]
fn int_literal_outside_jvm_range_is_rejected_without_bytecode() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    assert!(compiler
        .compile_expression(&Expression::IntLiteral(i64::MAX))
        .is_none());
    assert!(compiler.bytecode().is_empty());
    assert!(matches!(
        compiler.validate(),
        Err(BytecodeError::UnsupportedFeature { feature })
            if feature.contains("exceeds the JVM int range")
    ));
}

#[test]
fn unrepresentable_floating_literals_are_rejected_without_bytecode() {
    for expression in [
        Expression::FloatLiteral(f64::MAX),
        Expression::FloatLiteral(f64::MIN_POSITIVE),
        Expression::DoubleLiteral(f64::INFINITY),
    ] {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyAstCompiler::new(cp);

        assert!(compiler.compile_expression(&expression).is_none());
        assert!(compiler.bytecode().is_empty());
        assert!(matches!(
            compiler.validate(),
            Err(BytecodeError::UnsupportedFeature { feature })
                if feature.contains("outside the finite")
        ));
    }
}

#[test]
fn test_compile_binary_op() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    let expr = Expression::BinaryOp {
        operator: "+".into(),
        left: Box::new(Expression::IntLiteral(2)),
        right: Box::new(Expression::IntLiteral(3)),
    };
    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    // Should have: ICONST_2, ICONST_3, IADD
    assert!(compiler.bytecode().len() >= 3);
}

#[test]
fn test_compile_string_literal() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    let expr = Expression::StringLiteral("hello".to_string());
    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));
}

#[test]
fn test_compile_return_statement() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    let stmt = Statement::Return(Some(Expression::IntLiteral(5)));
    compiler.compile_statement(&stmt);

    // Should have: ICONST_5, INVOKESTATIC Integer.valueOf, ARETURN
    // (run() returns Object, so primitives are boxed)
    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0x08)); // ICONST_5
    assert!(bytecode.contains(&0xB8)); // INVOKESTATIC (Integer.valueOf)
    assert!(bytecode.contains(&0xB0)); // ARETURN
}

#[test]
fn test_compile_method_call() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // "hello".toUpperCase()
    let expr = Expression::MethodCall {
        receiver: Some(Box::new(Expression::StringLiteral("hello".to_string()))),
        method_name: "toUpperCase".into(),
        arguments: vec![],
    };

    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Should contain INVOKEDYNAMIC (0xBA)
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBA),
        "Should emit INVOKEDYNAMIC instruction"
    );
}

#[test]
fn test_compile_property_access() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // "hello".length
    let expr = Expression::PropertyAccess {
        object: Box::new(Expression::StringLiteral("hello".to_string())),
        property: "length".into(),
        safe_navigation: false,
    };

    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Should contain INVOKEDYNAMIC (0xBA) for property access
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBA),
        "Should emit INVOKEDYNAMIC for property access"
    );
}

#[test]
fn test_compile_index_access() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // [1, 2, 3][0]
    let expr = Expression::IndexAccess {
        object: Box::new(Expression::ListLiteral(vec![
            Expression::IntLiteral(1),
            Expression::IntLiteral(2),
            Expression::IntLiteral(3),
        ])),
        index: Box::new(Expression::IntLiteral(0)),
    };

    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Should contain INVOKEDYNAMIC (0xBA) for getAt call
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBA),
        "Should emit INVOKEDYNAMIC for index access"
    );
}

#[test]
fn test_compile_simple_closure() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // { 42 } - simple closure with no parameters returning a constant
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec![],
        body: vec![Statement::Expression(Expression::IntLiteral(42))],
        implicit_it: false,
    };

    let expr = Expression::Closure(Box::new(closure_expr));
    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Closure instantiation should emit NEW + INVOKESPECIAL and load owner/thisObject.
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBB),
        "Should emit NEW (0xBB) for closure instantiation"
    );
    assert!(
        bytecode.contains(&0xB7),
        "Should emit INVOKESPECIAL (0xB7) for closure constructor call"
    );
    let aload0_count = bytecode.iter().filter(|&&b| b == 0x2A).count();
    assert!(
        aload0_count >= 2,
        "Should emit at least 2 ALOAD_0 (0x2A) for owner and thisObject"
    );
}

#[test]
fn test_compile_closure_with_parameters() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // { x -> x * 2 } - closure with parameter
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec!["x".to_string()],
        body: vec![Statement::Expression(Expression::BinaryOp {
            operator: "*".into(),
            left: Box::new(Expression::Identifier("x".to_string())),
            right: Box::new(Expression::IntLiteral(2)),
        })],
        implicit_it: false,
    };

    let expr = Expression::Closure(Box::new(closure_expr));
    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Closure instantiation should emit NEW + INVOKESPECIAL and load owner/thisObject.
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBB),
        "Should emit NEW (0xBB) for closure instantiation"
    );
    assert!(
        bytecode.contains(&0xB7),
        "Should emit INVOKESPECIAL (0xB7) for closure constructor call"
    );
    let aload0_count = bytecode.iter().filter(|&&b| b == 0x2A).count();
    assert!(
        aload0_count >= 2,
        "Should emit at least 2 ALOAD_0 (0x2A) for owner and thisObject"
    );
}

#[test]
fn test_compile_closure_with_implicit_it() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // { println(it) } - closure with implicit 'it' parameter
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec![],
        body: vec![Statement::Expression(Expression::MethodCall {
            receiver: None,
            method_name: "println".into(),
            arguments: vec![Expression::Identifier("it".to_string())],
        })],
        implicit_it: true,
    };

    let expr = Expression::Closure(Box::new(closure_expr));
    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Closure instantiation should emit NEW + INVOKESPECIAL and load owner/thisObject.
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBB),
        "Should emit NEW (0xBB) for closure instantiation"
    );
    assert!(
        bytecode.contains(&0xB7),
        "Should emit INVOKESPECIAL (0xB7) for closure constructor call"
    );
    let aload0_count = bytecode.iter().filter(|&&b| b == 0x2A).count();
    assert!(
        aload0_count >= 2,
        "Should emit at least 2 ALOAD_0 (0x2A) for owner and thisObject"
    );
}

#[test]
fn test_analyze_free_variables_no_capture() {
    let cp = ConstantPoolBuilder::new();
    let compiler = GroovyAstCompiler::new(cp);

    // Closure: { x -> x * 2 }
    // x is a parameter, not a free variable
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec!["x".to_string()],
        body: vec![Statement::Expression(Expression::BinaryOp {
            operator: "*".into(),
            left: Box::new(Expression::Identifier("x".to_string())),
            right: Box::new(Expression::IntLiteral(2)),
        })],
        implicit_it: false,
    };

    let captured = compiler.analyze_free_variables(&closure_expr.parameters, &closure_expr.body);
    assert_eq!(captured.len(), 0, "Parameter should not be captured");
}

#[test]
fn test_analyze_free_variables_with_capture() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Define variable y in outer scope
    compiler.locals.insert(
        "y".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Int,
            declared_type: None,
            scope_depth: 0,
        },
    );

    // Closure: { x -> x + y }
    // y is a free variable (not a parameter, but used in closure body)
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec!["x".to_string()],
        body: vec![Statement::Expression(Expression::BinaryOp {
            operator: "+".into(),
            left: Box::new(Expression::Identifier("x".to_string())),
            right: Box::new(Expression::Identifier("y".to_string())),
        })],
        implicit_it: false,
    };

    let captured = compiler.analyze_free_variables(&closure_expr.parameters, &closure_expr.body);
    assert_eq!(captured.len(), 1, "Should capture y");
    assert_eq!(&*captured[0].name, "y");
    assert!(matches!(captured[0].java_type, JavaType::Int));
}

#[test]
fn test_analyze_free_variables_multiple_captures() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Define variables in outer scope
    compiler.locals.insert(
        "x".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Int,
            declared_type: None,
            scope_depth: 0,
        },
    );
    compiler.locals.insert(
        "y".into(),
        VariableInfo {
            slot: 1,
            java_type: JavaType::Int,
            declared_type: None,
            scope_depth: 0,
        },
    );

    // Closure: { a -> a + x + y }
    // x and y should be captured
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec!["a".to_string()],
        body: vec![Statement::Expression(Expression::BinaryOp {
            operator: "+".into(),
            left: Box::new(Expression::BinaryOp {
                operator: "+".into(),
                left: Box::new(Expression::Identifier("a".to_string())),
                right: Box::new(Expression::Identifier("x".to_string())),
            }),
            right: Box::new(Expression::Identifier("y".to_string())),
        })],
        implicit_it: false,
    };

    let captured = compiler.analyze_free_variables(&closure_expr.parameters, &closure_expr.body);
    assert_eq!(captured.len(), 2, "Should capture x and y");

    let names: Vec<_> = captured.iter().map(|c| &c.name).collect();
    assert!(names.contains(&&"x".to_string()));
    assert!(names.contains(&&"y".to_string()));
}

#[test]
fn test_analyze_free_variables_with_local_definition() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Define variable in outer scope
    compiler.locals.insert(
        "x".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Int,
            declared_type: None,
            scope_depth: 0,
        },
    );

    // Closure: { y = 42; x + y }
    // y is defined in closure, so should not be captured
    // x is from outer scope, so should be captured
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec![],
        body: vec![
            Statement::VariableDecl {
                name: "y".into(),
                type_annotation: None,
                initializer: Some(Expression::IntLiteral(42)),
            },
            Statement::Expression(Expression::BinaryOp {
                operator: "+".into(),
                left: Box::new(Expression::Identifier("x".to_string())),
                right: Box::new(Expression::Identifier("y".to_string())),
            }),
        ],
        implicit_it: false,
    };

    let captured = compiler.analyze_free_variables(&closure_expr.parameters, &closure_expr.body);
    assert_eq!(captured.len(), 1, "Should only capture x, not y");
    assert_eq!(&*captured[0].name, "x");
}

#[test]
fn test_phase_2_2_closure_with_captures_compilation() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Define variable y in outer scope
    compiler.locals.insert(
        "y".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Int,
            declared_type: None,
            scope_depth: 0,
        },
    );

    // Closure: { x -> x + y }
    // This closure has:
    // - 1 parameter: x
    // - 1 captured variable: y (from outer scope)
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec!["x".to_string()],
        body: vec![Statement::Expression(Expression::BinaryOp {
            operator: "+".into(),
            left: Box::new(Expression::Identifier("x".to_string())),
            right: Box::new(Expression::Identifier("y".to_string())),
        })],
        implicit_it: false,
    };

    let expr = Expression::Closure(Box::new(closure_expr));
    let result = compiler.compile_expression(&expr);

    // Compilation should succeed
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Phase 2.2: compile_closure() now calls analyze_free_variables()
    // and integrates captured variable detection.
    //
    // Closure instantiation should emit NEW + INVOKESPECIAL and load owner/thisObject.
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBB),
        "Phase 2.2: Should emit NEW (0xBB) for closure instantiation"
    );
    let aload0_count = bytecode.iter().filter(|&&b| b == 0x2A).count();
    assert!(
        aload0_count >= 2,
        "Phase 2.2: Should emit at least 2 ALOAD_0 (0x2A) for owner and thisObject"
    );
}

#[test]
fn test_phase_2_2_closure_multiple_captures_compilation() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Define multiple variables in outer scope
    compiler.locals.insert(
        "a".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Int,
            declared_type: None,
            scope_depth: 0,
        },
    );
    compiler.locals.insert(
        "b".into(),
        VariableInfo {
            slot: 1,
            java_type: JavaType::Object("java/lang/String".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );

    // Closure: { x -> a + b.length() }
    // This closure has:
    // - 1 parameter: x
    // - 2 captured variables: a (int), b (String)
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec!["x".to_string()],
        body: vec![Statement::Expression(Expression::BinaryOp {
            operator: "+".into(),
            left: Box::new(Expression::Identifier("a".to_string())),
            right: Box::new(Expression::PropertyAccess {
                object: Box::new(Expression::Identifier("b".to_string())),
                property: "length".into(),
                safe_navigation: false,
            }),
        })],
        implicit_it: false,
    };

    let expr = Expression::Closure(Box::new(closure_expr));
    let result = compiler.compile_expression(&expr);

    // Compilation should succeed
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Verify closure compiled with captures detected (and instantiated).
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBB),
        "Should emit NEW (0xBB) for closure instantiation"
    );
    let aload0_count = bytecode.iter().filter(|&&b| b == 0x2A).count();
    assert!(
        aload0_count >= 2,
        "Should emit at least 2 ALOAD_0 (0x2A) for owner and thisObject"
    );
}

#[test]
fn test_phase_2_3_closure_instantiation_with_captures() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Define variable y in outer scope
    compiler.locals.insert(
        "y".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Int,
            declared_type: None,
            scope_depth: 0,
        },
    );

    // Closure: { x -> x + y }
    // Phase 2.3 should emit:
    // - NEW Closure$_closure_N (0xBB)
    // - DUP (0x59)
    // - ALOAD_0 (0x2A) for owner
    // - ALOAD_0 (0x2A) for thisObject
    // - ILOAD slot 0 (0x15) for captured y
    // - INVOKESPECIAL (0xB7)
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec!["x".to_string()],
        body: vec![Statement::Expression(Expression::BinaryOp {
            operator: "+".into(),
            left: Box::new(Expression::Identifier("x".to_string())),
            right: Box::new(Expression::Identifier("y".to_string())),
        })],
        implicit_it: false,
    };

    let expr = Expression::Closure(Box::new(closure_expr));
    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    let bytecode = compiler.bytecode();

    // Phase 2.3: Should emit NEW (0xBB)
    assert!(
        bytecode.contains(&0xBB),
        "Phase 2.3: Should emit NEW instruction (0xBB) for closure instantiation"
    );

    // Phase 2.3: Should emit DUP (0x59)
    assert!(
        bytecode.contains(&0x59),
        "Phase 2.3: Should emit DUP instruction (0x59)"
    );

    // Phase 2.3: Should emit INVOKESPECIAL (0xB7)
    assert!(
        bytecode.contains(&0xB7),
        "Phase 2.3: Should emit INVOKESPECIAL instruction (0xB7) to call constructor"
    );

    // Phase 2.3: Should emit ALOAD_0 (0x2A) for owner and thisObject.
    // Should appear multiple times: for owner, thisObject, and potentially other loads.
    let aload0_count = bytecode.iter().filter(|&&b| b == 0x2A).count();
    assert!(
        aload0_count >= 2,
        "Phase 2.3: Should emit at least 2 ALOAD_0 (0x2A) for owner and thisObject"
    );
}

#[test]
fn test_phase_2_3_closure_no_captures_no_new() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Closure: { x -> x * 2 }
    // No captured variables, but Phase 2.3 still emits NEW sequence
    let closure_expr = hyperion_groovy::ClosureExpr {
        parameters: vec!["x".to_string()],
        body: vec![Statement::Expression(Expression::BinaryOp {
            operator: "*".into(),
            left: Box::new(Expression::Identifier("x".to_string())),
            right: Box::new(Expression::IntLiteral(2)),
        })],
        implicit_it: false,
    };

    let expr = Expression::Closure(Box::new(closure_expr));
    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());

    let bytecode = compiler.bytecode();

    // Even with no captures, Phase 2.3 should emit NEW + DUP + nulls + INVOKESPECIAL
    // (just with no capture loads in between)
    assert!(
        bytecode.contains(&0xBB),
        "Phase 2.3: Even closures without captures should emit NEW (0xBB)"
    );
    assert!(
        bytecode.contains(&0xB7),
        "Phase 2.3: Should emit INVOKESPECIAL (0xB7) for constructor call"
    );
}

// ========================================================================
// Comprehensive tests for the 3 critical expression types
// ========================================================================

#[test]
fn test_property_access_with_receiver() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // user.name
    let expr = Expression::PropertyAccess {
        object: Box::new(Expression::StringLiteral("test".to_string())),
        property: "length".into(),
        safe_navigation: false,
    };

    let result = compiler.compile_expression(&expr);

    assert!(result.is_some());
    assert!(matches!(
        result.unwrap(),
        JavaType::Object(ref s) if s == "java/lang/Object"
    ));

    // Should emit: LDC (string), LDC (property name), INVOKEDYNAMIC
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBA),
        "Should emit INVOKEDYNAMIC for property access"
    );
}

#[test]
fn test_property_access_chained() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // user.address.street
    let expr = Expression::PropertyAccess {
        object: Box::new(Expression::PropertyAccess {
            object: Box::new(Expression::Identifier("user".to_string())),
            property: "address".into(),
            safe_navigation: false,
        }),
        property: "street".into(),
        safe_navigation: false,
    };

    // First declare user variable
    compiler.locals.insert(
        "user".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Object("java/lang/Object".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Should contain multiple INVOKEDYNAMIC calls
    let bytecode = compiler.bytecode();
    let invokedynamic_count = bytecode.iter().filter(|&&b| b == 0xBA).count();
    assert!(
        invokedynamic_count >= 2,
        "Should emit multiple INVOKEDYNAMIC for chained property access"
    );
}

#[test]
fn test_index_access_array() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // arr[0]
    let expr = Expression::IndexAccess {
        object: Box::new(Expression::Identifier("arr".to_string())),
        index: Box::new(Expression::IntLiteral(0)),
    };

    // Declare arr variable
    compiler.locals.insert(
        "arr".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Array {
                element_type: Box::new(JavaType::Int),
                dimensions: 1,
            },
            declared_type: None,
            scope_depth: 0,
        },
    );

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Should emit: ALOAD (array), ICONST_0 (index), INVOKEDYNAMIC (getAt)
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBA),
        "Should emit INVOKEDYNAMIC for index access"
    );
}

#[test]
fn test_index_access_map() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // map["key"]
    let expr = Expression::IndexAccess {
        object: Box::new(Expression::Identifier("map".to_string())),
        index: Box::new(Expression::StringLiteral("key".to_string())),
    };

    // Declare map variable
    compiler.locals.insert(
        "map".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Object("java/util/Map".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBA),
        "Should emit INVOKEDYNAMIC for map access"
    );
}

#[test]
fn test_index_access_nested() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // arr[0][1]
    let expr = Expression::IndexAccess {
        object: Box::new(Expression::IndexAccess {
            object: Box::new(Expression::Identifier("arr".to_string())),
            index: Box::new(Expression::IntLiteral(0)),
        }),
        index: Box::new(Expression::IntLiteral(1)),
    };

    compiler.locals.insert(
        "arr".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Array {
                element_type: Box::new(JavaType::Array {
                    element_type: Box::new(JavaType::Int),
                    dimensions: 1,
                }),
                dimensions: 1,
            },
            declared_type: None,
            scope_depth: 0,
        },
    );

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    let invokedynamic_count = bytecode.iter().filter(|&&b| b == 0xBA).count();
    assert!(
        invokedynamic_count >= 2,
        "Should emit multiple INVOKEDYNAMIC for nested index access"
    );
}

#[test]
fn test_method_call_with_receiver() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // user.getName()
    let expr = Expression::MethodCall {
        receiver: Some(Box::new(Expression::Identifier("user".to_string()))),
        method_name: "getName".into(),
        arguments: vec![],
    };

    compiler.locals.insert(
        "user".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Object("com/example/User".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Object(_)));

    // Should emit: ALOAD (user), INVOKEDYNAMIC (getName)
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBA),
        "Should emit INVOKEDYNAMIC for method call"
    );
}

#[test]
fn test_method_call_with_arguments() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // user.setName("John", "Doe")
    let expr = Expression::MethodCall {
        receiver: Some(Box::new(Expression::Identifier("user".to_string()))),
        method_name: "setName".into(),
        arguments: vec![
            Expression::StringLiteral("John".to_string()),
            Expression::StringLiteral("Doe".to_string()),
        ],
    };

    compiler.locals.insert(
        "user".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Object("com/example/User".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBA),
        "Should emit INVOKEDYNAMIC for method call with args"
    );
}

#[test]
fn test_method_call_without_receiver() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // println("Hello")
    let expr = Expression::MethodCall {
        receiver: None,
        method_name: "println".into(),
        arguments: vec![Expression::StringLiteral("Hello".to_string())],
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    // Should load 'this' (ALOAD_0), then LDC (string), then INVOKEDYNAMIC
    assert!(
        bytecode.contains(&0x2A),
        "Should emit ALOAD_0 to load 'this' as receiver"
    );
    assert!(
        bytecode.contains(&0xBA),
        "Should emit INVOKEDYNAMIC for unqualified method call"
    );
}

#[test]
fn test_method_call_chained() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // user.getName().toUpperCase()
    let expr = Expression::MethodCall {
        receiver: Some(Box::new(Expression::MethodCall {
            receiver: Some(Box::new(Expression::Identifier("user".to_string()))),
            method_name: "getName".into(),
            arguments: vec![],
        })),
        method_name: "toUpperCase".into(),
        arguments: vec![],
    };

    compiler.locals.insert(
        "user".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Object("com/example/User".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    let invokedynamic_count = bytecode.iter().filter(|&&b| b == 0xBA).count();
    assert!(
        invokedynamic_count >= 2,
        "Should emit multiple INVOKEDYNAMIC for chained method calls"
    );
}

#[test]
fn test_complex_expression_combination() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // user.addresses[0].getStreet()
    let expr = Expression::MethodCall {
        receiver: Some(Box::new(Expression::IndexAccess {
            object: Box::new(Expression::PropertyAccess {
                object: Box::new(Expression::Identifier("user".to_string())),
                property: "addresses".into(),
                safe_navigation: false,
            }),
            index: Box::new(Expression::IntLiteral(0)),
        })),
        method_name: "getStreet".into(),
        arguments: vec![],
    };

    compiler.locals.insert(
        "user".into(),
        VariableInfo {
            slot: 0,
            java_type: JavaType::Object("com/example/User".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    let invokedynamic_count = bytecode.iter().filter(|&&b| b == 0xBA).count();
    assert!(
        invokedynamic_count >= 3,
        "Should emit INVOKEDYNAMIC for each dynamic operation (property, index, method)"
    );
}

#[test]
fn test_method_call_with_binary_op_arguments() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // doSomething(a + b, c * d)
    let expr = Expression::MethodCall {
        receiver: None,
        method_name: "doSomething".into(),
        arguments: vec![
            Expression::BinaryOp {
                operator: "+".into(),
                left: Box::new(Expression::Identifier("a".to_string())),
                right: Box::new(Expression::Identifier("b".to_string())),
            },
            Expression::BinaryOp {
                operator: "*".into(),
                left: Box::new(Expression::Identifier("c".to_string())),
                right: Box::new(Expression::Identifier("d".to_string())),
            },
        ],
    };

    // Define variables
    for (name, slot) in [("a", 1), ("b", 2), ("c", 3), ("d", 4)] {
        compiler.locals.insert(
            name.to_string(),
            VariableInfo {
                slot,
                java_type: JavaType::Int,
                declared_type: None,
                scope_depth: 0,
            },
        );
    }

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0x2A),
        "Should load 'this' for unqualified method call"
    );
    assert!(bytecode.contains(&0xBA), "Should emit INVOKEDYNAMIC");
}

// ========================================================================
// Phase 1.4: Operator Tests
// ========================================================================

#[test]
fn test_bitwise_and_operator() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 5 & 3 = 1
    let expr = Expression::BinaryOp {
        operator: "&".into(),
        left: Box::new(Expression::IntLiteral(5)),
        right: Box::new(Expression::IntLiteral(3)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Int));

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0x7E), "Should emit IAND (0x7E)");
}

#[test]
fn test_bitwise_or_operator() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 5 | 3 = 7
    let expr = Expression::BinaryOp {
        operator: "|".into(),
        left: Box::new(Expression::IntLiteral(5)),
        right: Box::new(Expression::IntLiteral(3)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0x80), "Should emit IOR (0x80)");
}

#[test]
fn test_bitwise_xor_operator() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 5 ^ 3 = 6
    let expr = Expression::BinaryOp {
        operator: "^".into(),
        left: Box::new(Expression::IntLiteral(5)),
        right: Box::new(Expression::IntLiteral(3)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0x82), "Should emit IXOR (0x82)");
}

#[test]
fn test_left_shift_operator() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 5 << 2 = 20
    let expr = Expression::BinaryOp {
        operator: "<<".into(),
        left: Box::new(Expression::IntLiteral(5)),
        right: Box::new(Expression::IntLiteral(2)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0x78), "Should emit ISHL (0x78)");
}

#[test]
fn test_right_shift_operator() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 20 >> 2 = 5
    let expr = Expression::BinaryOp {
        operator: ">>".into(),
        left: Box::new(Expression::IntLiteral(20)),
        right: Box::new(Expression::IntLiteral(2)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0x7A), "Should emit ISHR (0x7A)");
}

#[test]
fn test_unsigned_right_shift_operator() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // -20 >>> 2 (unsigned right shift)
    let expr = Expression::BinaryOp {
        operator: ">>>".into(),
        left: Box::new(Expression::IntLiteral(-20)),
        right: Box::new(Expression::IntLiteral(2)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0x7C), "Should emit IUSHR (0x7C)");
}

#[test]
fn test_long_bitwise_operators() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 5L & 3L
    let expr = Expression::BinaryOp {
        operator: "&".into(),
        left: Box::new(Expression::LongLiteral(5)),
        right: Box::new(Expression::LongLiteral(3)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(matches!(result.unwrap(), JavaType::Long));

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0x7F), "Should emit LAND (0x7F)");
}

#[test]
fn test_comparison_equal() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 5 == 5
    let expr = Expression::BinaryOp {
        operator: "==".into(),
        left: Box::new(Expression::IntLiteral(5)),
        right: Box::new(Expression::IntLiteral(5)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    // A comparison yields a boolean (represented as int 0/1 on the stack). The
    // static type is `Boolean` so the result boxes via `Boolean.valueOf` and
    // displays as `true`/`false`, not `1`/`0`.
    assert!(matches!(result.unwrap(), JavaType::Boolean));

    let bytecode = compiler.bytecode();
    // Should contain IF_ICMPEQ (0x9F)
    assert!(bytecode.contains(&0x9F), "Should emit IF_ICMPEQ");
}

#[test]
fn test_comparison_not_equal() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 5 != 3
    let expr = Expression::BinaryOp {
        operator: "!=".into(),
        left: Box::new(Expression::IntLiteral(5)),
        right: Box::new(Expression::IntLiteral(3)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0xA0), "Should emit IF_ICMPNE");
}

#[test]
fn test_comparison_less_than() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 3 < 5
    let expr = Expression::BinaryOp {
        operator: "<".into(),
        left: Box::new(Expression::IntLiteral(3)),
        right: Box::new(Expression::IntLiteral(5)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0xA1), "Should emit IF_ICMPLT");
}

#[test]
fn test_comparison_greater_than() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 5 > 3
    let expr = Expression::BinaryOp {
        operator: ">".into(),
        left: Box::new(Expression::IntLiteral(5)),
        right: Box::new(Expression::IntLiteral(3)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0xA3), "Should emit IF_ICMPGT");
}

#[test]
fn test_comparison_long() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 5L < 10L
    let expr = Expression::BinaryOp {
        operator: "<".into(),
        left: Box::new(Expression::LongLiteral(5)),
        right: Box::new(Expression::LongLiteral(10)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0x94), "Should emit LCMP");
}

#[test]
fn test_comparison_double() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // 5.0 < 10.0
    let expr = Expression::BinaryOp {
        operator: "<".into(),
        left: Box::new(Expression::DoubleLiteral(5.0)),
        right: Box::new(Expression::DoubleLiteral(10.0)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0x98),
        "less-than should emit DCMPG so NaN is not ordered below a number"
    );
}

#[test]
fn test_logical_and_short_circuit() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // true && false
    let expr = Expression::BinaryOp {
        operator: "&&".into(),
        left: Box::new(Expression::BooleanLiteral(true)),
        right: Box::new(Expression::BooleanLiteral(false)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    // Should contain DUP (0x59) for short-circuit logic
    assert!(
        bytecode.contains(&0x59),
        "Should emit DUP for short-circuit"
    );
    // Should contain IFEQ (0x99) for conditional jump
    assert!(bytecode.contains(&0x99), "Should emit IFEQ");
}

#[test]
fn test_logical_or_short_circuit() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // false || true
    let expr = Expression::BinaryOp {
        operator: "||".into(),
        left: Box::new(Expression::BooleanLiteral(false)),
        right: Box::new(Expression::BooleanLiteral(true)),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    // Should contain DUP (0x59) and IFNE (0x9A)
    assert!(bytecode.contains(&0x59), "Should emit DUP");
    assert!(bytecode.contains(&0x9A), "Should emit IFNE");
}

#[test]
fn test_string_concatenation() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // "Hello" + " World"
    let expr = Expression::BinaryOp {
        operator: "+".into(),
        left: Box::new(Expression::StringLiteral("Hello".to_string())),
        right: Box::new(Expression::StringLiteral(" World".to_string())),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(matches!(
        result.unwrap(),
        JavaType::Object(ref s) if s == "java/lang/String"
    ));

    let bytecode = compiler.bytecode();
    // Should contain NEW (0xBB) for StringBuilder
    assert!(
        bytecode.contains(&0xBB),
        "Should emit NEW for StringBuilder"
    );
    // Should contain INVOKEVIRTUAL (0xB6) for append
    assert!(
        bytecode.contains(&0xB6),
        "Should emit INVOKEVIRTUAL for append/toString"
    );
}

// ========================================================================
// Phase 1.5: Statement Tests
// ========================================================================

#[test]
fn test_throw_statement() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    let stmt = Statement::Throw(Expression::NewObject {
        class_name: "RuntimeException".to_string(),
        arguments: Vec::new(),
    });

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0xBF), "Should emit ATHROW (0xBF)");
    compiler
        .validate()
        .expect("Throwable throw should validate");
}

#[test]
fn qualified_constructor_names_are_canonicalized() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    let result = compiler.compile_expression(&Expression::NewObject {
        class_name: "java.lang.RuntimeException".to_string(),
        arguments: Vec::new(),
    });

    assert_eq!(
        result,
        Some(JavaType::Object("java/lang/RuntimeException".to_string()))
    );
    compiler
        .validate()
        .expect("canonical constructor must validate");
}

#[test]
fn malformed_constructor_type_is_rejected_without_bytecode() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    assert!(compiler
        .compile_expression(&Expression::NewObject {
            class_name: "java.util.List<String".to_string(),
            arguments: Vec::new(),
        })
        .is_none());
    assert!(compiler.bytecode().is_empty());
}

#[test]
fn test_throw_rejects_non_throwable_reference() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    compiler.compile_statement(&Statement::Throw(Expression::StringLiteral(
        "error".to_string(),
    )));

    assert!(!compiler.bytecode().contains(&0xBF));
    assert!(matches!(
        compiler.validate(),
        Err(BytecodeError::UnsupportedFeature { feature })
            if feature.contains("requires a Throwable reference")
    ));
}

#[test]
fn catch_types_are_canonicalized_before_exception_table_emission() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    compiler.compile_statement(&Statement::Try {
        body: vec![Statement::Expression(Expression::IntLiteral(1))],
        catch_blocks: vec![hyperion_groovy::CatchBlock {
            exception_type: "java.lang.RuntimeException".to_string(),
            variable_name: "error".to_string(),
            body: vec![Statement::Expression(Expression::IntLiteral(2))],
        }],
        finally_block: None,
    });

    assert!(compiler
        .exception_handlers()
        .iter()
        .any(|handler| handler.catch_type.as_deref() == Some("java/lang/RuntimeException")));
    compiler
        .validate()
        .expect("canonical catch type must validate");
}

#[test]
fn non_throwable_catch_type_is_rejected_before_try_bytecode() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    compiler.compile_statement(&Statement::Try {
        body: vec![Statement::Expression(Expression::IntLiteral(1))],
        catch_blocks: vec![hyperion_groovy::CatchBlock {
            exception_type: "java.lang.String".to_string(),
            variable_name: "value".to_string(),
            body: Vec::new(),
        }],
        finally_block: None,
    });

    assert!(compiler.bytecode().is_empty());
    assert!(matches!(
        compiler.validate(),
        Err(BytecodeError::UnsupportedFeature { feature })
            if feature.contains("catch requires a Throwable class")
    ));
}

#[test]
fn typed_method_returns_use_descriptor_compatible_opcodes() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    compiler.set_method_return_type(JavaType::Long);

    compiler.compile_statement(&Statement::Return(Some(Expression::IntLiteral(7))));

    assert!(
        compiler.bytecode().contains(&0x85),
        "int return must widen with I2L"
    );
    assert!(
        compiler.bytecode().contains(&0xAD),
        "long method must use LRETURN"
    );
    assert!(
        !compiler.bytecode().contains(&0xB0),
        "typed method must not use ARETURN"
    );
    compiler.validate().expect("adapted return should validate");
}

#[test]
fn typed_method_returns_reject_narrowing() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    compiler.set_method_return_type(JavaType::Int);

    compiler.compile_statement(&Statement::Return(Some(Expression::LongLiteral(7))));

    assert!(matches!(
        compiler.validate(),
        Err(BytecodeError::UnsupportedFeature { feature })
            if feature.contains("cannot adapt return value")
    ));
}

#[test]
fn test_for_loop_basic() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Create a list literal for iteration
    let list = Expression::ListLiteral(vec![
        Expression::IntLiteral(1),
        Expression::IntLiteral(2),
        Expression::IntLiteral(3),
    ]);

    // for (item in [1, 2, 3]) { ... }
    let stmt = Statement::For {
        variable: "item".into(),
        iterable: list,
        body: vec![Statement::Expression(Expression::Identifier("item".into()))],
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    // Should contain INVOKEINTERFACE (0xB9) for iterator methods
    assert!(
        bytecode.contains(&0xB9),
        "Should emit INVOKEINTERFACE for iterator"
    );
}

#[test]
fn test_traditional_for_loop() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // for (int i = 0; i < 5; i++) { ... }
    let init = Statement::VariableDecl {
        name: "i".into(),
        type_annotation: Some("int".into()),
        initializer: Some(Expression::IntLiteral(0)),
    };

    let condition = Expression::BinaryOp {
        operator: "<".into(),
        left: Box::new(Expression::Identifier("i".to_string())),
        right: Box::new(Expression::IntLiteral(5)),
    };

    let increment = Expression::BinaryOp {
        operator: "+".into(),
        left: Box::new(Expression::Identifier("i".to_string())),
        right: Box::new(Expression::IntLiteral(1)),
    };

    let stmt = Statement::TraditionalFor {
        init: Some(Box::new(init)),
        condition: Some(condition),
        increment: Some(increment),
        body: vec![],
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    // Should contain conditional jump (IFEQ 0x99)
    assert!(bytecode.contains(&0x99), "Should emit IFEQ for condition");
    // Should contain GOTO (0xA7) for loop back
    assert!(bytecode.contains(&0xA7), "Should emit GOTO for loop");
}

#[test]
fn test_break_statement() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    let stmt = Statement::Break;
    compiler.compile_statement(&stmt);

    let error = compiler
        .validate()
        .expect_err("break outside a loop must not emit executable output");
    assert!(
        error.to_string().contains("break outside a loop"),
        "{error}"
    );
    assert!(compiler.bytecode().is_empty());
}

#[test]
fn test_continue_statement() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    let stmt = Statement::Continue;
    compiler.compile_statement(&stmt);

    let error = compiler
        .validate()
        .expect_err("continue outside a loop must not emit executable output");
    assert!(
        error.to_string().contains("continue outside a loop"),
        "{error}"
    );
    assert!(compiler.bytecode().is_empty());
}

#[test]
fn test_switch_statement_is_rejected_until_lowering_is_implemented() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    let stmt = Statement::Switch {
        expression: Expression::IntLiteral(1),
        cases: vec![],
        default_case: None,
    };

    compiler.compile_statement(&stmt);

    let error = compiler
        .validate()
        .expect_err("switch must not be compiled as a discarded expression");
    assert!(error.to_string().contains("switch statements"), "{error}");
    assert!(compiler.bytecode().is_empty());
}

#[test]
fn test_try_catch_basic() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // try { 42 } finally { }
    let stmt = Statement::Try {
        body: vec![Statement::Expression(Expression::IntLiteral(42))],
        catch_blocks: vec![],
        finally_block: Some(vec![]),
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    // Should compile the body (contains ICONST or similar)
    assert!(!bytecode.is_empty(), "Should emit bytecode for try body");
}

// ========================================================================
// PHASE 1.7.5: Closures as Method Arguments
// ========================================================================

#[test]
fn test_method_call_with_single_closure_argument() {
    use hyperion_groovy::ClosureExpr;
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Declare 'list' variable
    compiler.locals.insert(
        "list".into(),
        VariableInfo {
            slot: 1,
            java_type: JavaType::Object("java/util/List".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );

    // list.each { println it }
    let closure_expr = ClosureExpr {
        parameters: vec![],
        body: vec![Statement::Expression(Expression::MethodCall {
            receiver: None,
            method_name: "println".into(),
            arguments: vec![Expression::Identifier("it".to_string())],
        })],
        implicit_it: true,
    };

    let expr = Expression::MethodCall {
        receiver: Some(Box::new(Expression::Identifier("list".to_string()))),
        method_name: "each".into(),
        arguments: vec![Expression::Closure(Box::new(closure_expr))],
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(!compiler.bytecode().is_empty());

    // Verify closure class was generated
    assert_eq!(compiler.closure_classes.len(), 1);
}

#[test]
fn test_method_call_with_mixed_args_and_closure() {
    use hyperion_groovy::ClosureExpr;
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Declare variables
    compiler.locals.insert(
        "handler".into(),
        VariableInfo {
            slot: 1,
            java_type: JavaType::Object("java/lang/Object".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );
    compiler.locals.insert(
        "timeout".into(),
        VariableInfo {
            slot: 2,
            java_type: JavaType::Int,
            declared_type: None,
            scope_depth: 0,
        },
    );

    // handler.process(timeout, { work() })
    let closure_expr = ClosureExpr {
        parameters: vec![],
        body: vec![Statement::Expression(Expression::MethodCall {
            receiver: None,
            method_name: "work".into(),
            arguments: vec![],
        })],
        implicit_it: false,
    };

    let expr = Expression::MethodCall {
        receiver: Some(Box::new(Expression::Identifier("handler".to_string()))),
        method_name: "process".into(),
        arguments: vec![
            Expression::Identifier("timeout".to_string()),
            Expression::Closure(Box::new(closure_expr)),
        ],
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(!compiler.bytecode().is_empty());

    // Verify closure class was generated
    assert_eq!(compiler.closure_classes.len(), 1);
}

#[test]
fn test_method_call_nested_closures_in_arguments() {
    use hyperion_groovy::ClosureExpr;
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Declare variables
    compiler.locals.insert(
        "config".into(),
        VariableInfo {
            slot: 1,
            java_type: JavaType::Object("java/lang/Object".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );
    compiler.locals.insert(
        "enabled".into(),
        VariableInfo {
            slot: 2,
            java_type: JavaType::Boolean,
            declared_type: None,
            scope_depth: 0,
        },
    );

    // config.setup(enabled, { sub { helper() } })
    let inner_closure = ClosureExpr {
        parameters: vec![],
        body: vec![Statement::Expression(Expression::MethodCall {
            receiver: None,
            method_name: "helper".into(),
            arguments: vec![],
        })],
        implicit_it: false,
    };

    let outer_closure = ClosureExpr {
        parameters: vec![],
        body: vec![Statement::Expression(Expression::MethodCall {
            receiver: None,
            method_name: "sub".into(),
            arguments: vec![Expression::Closure(Box::new(inner_closure))],
        })],
        implicit_it: false,
    };

    let expr = Expression::MethodCall {
        receiver: Some(Box::new(Expression::Identifier("config".to_string()))),
        method_name: "setup".into(),
        arguments: vec![
            Expression::Identifier("enabled".to_string()),
            Expression::Closure(Box::new(outer_closure)),
        ],
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(!compiler.bytecode().is_empty());

    // Nested closures are compiled eagerly as part of closure class bytecode generation.
    // Expect:
    // - outer closure class (Closure$_closure_N)
    // - inner closure class (Closure$_nested_N)
    assert_eq!(compiler.closure_classes.len(), 2);
}

#[test]
fn test_gradle_dsl_dependencies_pattern() {
    use hyperion_groovy::ClosureExpr;
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // dependencies {
    //     implementation 'com.google.guava:guava:31.1-jre'
    // }
    let closure_expr = ClosureExpr {
        parameters: vec![],
        body: vec![Statement::Expression(Expression::MethodCall {
            receiver: None,
            method_name: "implementation".into(),
            arguments: vec![Expression::StringLiteral(
                "com.google.guava:guava:31.1-jre".into(),
            )],
        })],
        implicit_it: false,
    };

    let expr = Expression::MethodCall {
        receiver: None, // Implicit 'this' in Gradle script
        method_name: "dependencies".into(),
        arguments: vec![Expression::Closure(Box::new(closure_expr))],
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(!compiler.bytecode().is_empty());

    // Verify closure class was generated
    assert_eq!(compiler.closure_classes.len(), 1);

    // Verify bytecode contains invokedynamic for dependencies method
    let bytecode = compiler.bytecode();
    assert!(
        bytecode.contains(&0xBA),
        "Should contain INVOKEDYNAMIC opcode"
    );
}

#[test]
fn test_method_call_with_only_normal_args_no_closure() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // Declare handler variable
    compiler.locals.insert(
        "handler".into(),
        VariableInfo {
            slot: 1,
            java_type: JavaType::Object("java/lang/Object".to_string()),
            declared_type: None,
            scope_depth: 0,
        },
    );

    // handler.process(arg1, arg2) - no closure
    let expr = Expression::MethodCall {
        receiver: Some(Box::new(Expression::Identifier("handler".to_string()))),
        method_name: "process".into(),
        arguments: vec![Expression::IntLiteral(1), Expression::IntLiteral(2)],
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());
    assert!(!compiler.bytecode().is_empty());

    // No closure classes should be generated
    assert_eq!(compiler.closure_classes.len(), 0);
}

// ========================================================================
// Integration Tests
// ========================================================================

#[test]
fn test_complex_expression_with_operators() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // (5 & 3) | (2 << 1)  =>  (1) | (4) = 5
    let expr = Expression::BinaryOp {
        operator: "|".into(),
        left: Box::new(Expression::BinaryOp {
            operator: "&".into(),
            left: Box::new(Expression::IntLiteral(5)),
            right: Box::new(Expression::IntLiteral(3)),
        }),
        right: Box::new(Expression::BinaryOp {
            operator: "<<".into(),
            left: Box::new(Expression::IntLiteral(2)),
            right: Box::new(Expression::IntLiteral(1)),
        }),
    };

    let result = compiler.compile_expression(&expr);
    assert!(result.is_some());

    let bytecode = compiler.bytecode();
    assert!(bytecode.contains(&0x7E), "Should contain IAND");
    assert!(bytecode.contains(&0x78), "Should contain ISHL");
    assert!(bytecode.contains(&0x80), "Should contain IOR");
}

#[test]
fn test_comparison_in_if_statement() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // if (x > 5) { return true; }
    compiler.locals.insert(
        "x".into(),
        VariableInfo {
            slot: 1,
            java_type: JavaType::Int,
            declared_type: None,
            scope_depth: 0,
        },
    );

    let stmt = Statement::If {
        condition: Expression::BinaryOp {
            operator: ">".into(),
            left: Box::new(Expression::Identifier("x".to_string())),
            right: Box::new(Expression::IntLiteral(5)),
        },
        then_body: vec![Statement::Return(Some(Expression::BooleanLiteral(true)))],
        else_body: None,
    };

    compiler.compile_statement(&stmt);

    let bytecode = compiler.bytecode();
    // Should contain comparison and conditional jump
    assert!(!bytecode.is_empty(), "Should generate bytecode");
}

#[test]
fn test_loop_with_break_continue() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);

    // while (true) { break; continue; }
    let stmt = Statement::While {
        condition: Expression::BooleanLiteral(true),
        body: vec![Statement::Break, Statement::Continue],
    };

    compiler.compile_statement(&stmt);

    compiler
        .validate()
        .expect("loop-local break and continue should have real branch targets");
    assert!(
        compiler.bytecode().contains(&0xA7),
        "expected GOTO branches"
    );
}

#[test]
fn validation_rejects_operand_stack_accounting_underflow() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    compiler
        .stmt_compiler_mut()
        .expr_compiler()
        .stack_depth()
        .pop1();

    let error = compiler.validate().expect_err("underflow must fail closed");
    assert!(matches!(
        error,
        BytecodeError::InternalError { message }
            if message.contains("operand stack accounting underflowed")
    ));
}

#[test]
fn rejected_closure_body_leaves_no_partial_bytecode_or_classes() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    let closure = Expression::Closure(Box::new(hyperion_groovy::ClosureExpr {
        parameters: Vec::new(),
        body: vec![Statement::VariableDecl {
            name: "erased_before".to_string(),
            type_annotation: None,
            initializer: Some(Expression::IntLiteral(1)),
        }],
        implicit_it: true,
    }));

    assert!(compiler.compile_expression(&closure).is_none());
    assert!(compiler.bytecode().is_empty());
    assert!(compiler.closure_classes().is_empty());
    assert!(matches!(
        compiler.validate(),
        Err(BytecodeError::UnsupportedFeature { feature })
            if feature.contains("unsupported statement in generated closure")
    ));
}

#[test]
fn out_of_range_int_in_closure_leaves_no_partial_bytecode_or_classes() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    let closure = Expression::Closure(Box::new(hyperion_groovy::ClosureExpr {
        parameters: Vec::new(),
        body: vec![Statement::Expression(Expression::IntLiteral(i64::MIN))],
        implicit_it: true,
    }));

    assert!(compiler.compile_expression(&closure).is_none());
    assert!(compiler.bytecode().is_empty());
    assert!(compiler.closure_classes().is_empty());
    assert!(matches!(
        compiler.validate(),
        Err(BytecodeError::UnsupportedFeature { feature })
            if feature.contains("exceeds the JVM int range")
    ));
}

#[test]
fn oversized_dynamic_call_in_closure_is_rejected_transactionally() {
    let cp = ConstantPoolBuilder::new();
    let mut compiler = GroovyAstCompiler::new(cp);
    let closure = Expression::Closure(Box::new(hyperion_groovy::ClosureExpr {
        parameters: Vec::new(),
        body: vec![Statement::Expression(Expression::MethodCall {
            receiver: None,
            method_name: "oversized".to_string(),
            arguments: vec![Expression::NullLiteral; 255],
        })],
        implicit_it: true,
    }));

    assert!(compiler.compile_expression(&closure).is_none());
    assert!(compiler.bytecode().is_empty());
    assert!(compiler.closure_classes().is_empty());
    assert!(matches!(
        compiler.validate(),
        Err(BytecodeError::UnsupportedFeature { feature })
            if feature.contains("at most 254 arguments plus the receiver")
    ));
}
