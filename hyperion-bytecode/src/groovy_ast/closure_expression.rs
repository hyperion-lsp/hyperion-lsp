// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Raw expression lowering for generated Groovy closure classes.

use super::{
    closure_class::generate_closure_class_bytes, closure_scope::free_variable_names,
    expressions::property_setter_name, CapturedVariable, ClosureCompilationContext,
};
use crate::groovy_compiler::validate_dynamic_call_arity;
use crate::java_compiler::JavaType;
use hyperion_groovy::Expression;
use tracing::trace;

/// Compile a closure expression to bytecode
///
/// Handles variable resolution:
/// - Parameters: loaded from local slots (extracted from args[])
/// - Captured variables: loaded via getfield from `this`
pub(super) fn compile_closure_expr(
    bytecode: &mut Vec<u8>,
    cp: &mut crate::constant_pool::ConstantPoolBuilder,
    expr: &Expression,
    param_slots: &std::collections::HashMap<String, u16>,
    captures: &[CapturedVariable],
    class_name: &str,
    closure_classes: &mut std::collections::HashMap<String, Vec<u8>>,
) -> Result<JavaType, String> {
    match expr {
        Expression::IntLiteral(value) => {
            let v = i32::try_from(*value)
                .map_err(|_| format!("Groovy int literal {value} exceeds the JVM int range"))?;
            if (-1..=5).contains(&v) {
                bytecode.push(0x02 + (v + 1) as u8); // iconst_m1 to iconst_5
            } else if (-128..=127).contains(&v) {
                bytecode.push(0x10); // bipush
                bytecode.push(v as u8);
            } else if (-32768..=32767).contains(&v) {
                bytecode.push(0x11); // sipush
                bytecode.extend_from_slice(&(v as i16).to_be_bytes());
            } else {
                // ldc for larger integers
                let int_idx = cp.add_integer(v);
                emit_ldc(bytecode, int_idx);
            }
            Ok(JavaType::Int)
        }

        Expression::StringLiteral(value) => {
            let str_idx = cp.add_string(value);
            emit_ldc(bytecode, str_idx);
            Ok(JavaType::Object("java/lang/String".to_string()))
        }

        Expression::BooleanLiteral(value) => {
            bytecode.push(if *value { 0x04 } else { 0x03 }); // iconst_1 or iconst_0
            Ok(JavaType::Boolean)
        }

        Expression::NullLiteral => {
            bytecode.push(0x01); // aconst_null
            Ok(JavaType::Object("java/lang/Object".to_string()))
        }

        Expression::Identifier(name) | Expression::Variable(name) => {
            // Check if it's a parameter
            if let Some(&slot) = param_slots.get(name) {
                // Closure parameters are declared as Object. Keep them boxed here;
                // arithmetic operators unbox contextually, while receivers, values,
                // and returns must remain references.
                if slot <= 3 {
                    bytecode.push(0x2A + slot as u8); // aload_0 to aload_3
                } else {
                    bytecode.push(0x19); // aload
                    bytecode.push(slot as u8);
                }
                Ok(JavaType::Object("java/lang/Object".to_string()))
            } else if let Some(capture) = captures.iter().find(|c| &c.name == name) {
                // Load captured variable via getfield
                // aload_0 (this)
                bytecode.push(0x2A);
                // getfield
                // Note: captured Int fields are stored as primitive int (descriptor "I")
                // so getfield returns int directly - no unboxing needed
                let field_ref =
                    cp.add_field_ref(class_name, &capture.name, &capture.java_type.descriptor());
                bytecode.push(0xB4); // getfield
                bytecode.extend_from_slice(&field_ref.to_be_bytes());
                Ok(capture.java_type.clone())
            } else {
                // Unknown variable - resolve via closure's owner/delegate chain
                // In Groovy, closures can access variables from enclosing scope via getProperty
                // Pattern: this.getProperty("variableName")
                trace!("Resolving unknown variable via getProperty: {}", name);

                // Load this (the closure)
                bytecode.push(0x2A); // aload_0

                // Push property name as String constant
                let property_str_idx = cp.add_string(name);
                emit_ldc(bytecode, property_str_idx);

                // The invokedynamic descriptor includes the closure receiver.
                let bootstrap_index = ensure_groovy_bootstrap(cp);
                let descriptor = "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;";
                emit_invokedynamic(bytecode, cp, "getProperty", descriptor, bootstrap_index);
                Ok(JavaType::Object("java/lang/Object".to_string()))
            }
        }

        Expression::MethodCall {
            receiver,
            method_name,
            arguments,
        } => {
            validate_dynamic_call_arity(arguments.len())?;

            if let Some(recv) = receiver {
                let receiver_type = compile_closure_expr(
                    bytecode,
                    cp,
                    recv,
                    param_slots,
                    captures,
                    class_name,
                    closure_classes,
                )?;
                box_closure_value(bytecode, cp, &receiver_type)?;
            } else {
                bytecode.push(0x2A); // aload_0 (this)
            }

            for arg in arguments {
                let argument_type = compile_closure_expr(
                    bytecode,
                    cp,
                    arg,
                    param_slots,
                    captures,
                    class_name,
                    closure_classes,
                )?;
                box_closure_value(bytecode, cp, &argument_type)?;
            }

            let bootstrap_index = ensure_groovy_bootstrap(cp);
            let mut descriptor = String::from("(Ljava/lang/Object;");
            for _ in 0..arguments.len() {
                descriptor.push_str("Ljava/lang/Object;");
            }
            descriptor.push_str(")Ljava/lang/Object;");
            emit_invokedynamic(bytecode, cp, method_name, &descriptor, bootstrap_index);
            Ok(JavaType::Object("java/lang/Object".to_string()))
        }

        Expression::Closure(closure_expr) => {
            let nested_class_name = format!("{class_name}$_nested_{}", closure_classes.len());
            let mut params = closure_expr.parameters.clone();
            if closure_expr.implicit_it && params.is_empty() {
                params.push("it".to_string());
            }

            let mut closure_ctx = ClosureCompilationContext::new(nested_class_name.clone());
            for param in &params {
                closure_ctx.add_parameter(
                    param.clone(),
                    JavaType::Object("java/lang/Object".to_string()),
                );
            }

            let nested_captures: Vec<_> = free_variable_names(&params, &closure_expr.body)
                .into_iter()
                .filter_map(|name| {
                    let java_type = if param_slots.contains_key(&name) {
                        Some(JavaType::Object("java/lang/Object".to_string()))
                    } else {
                        captures
                            .iter()
                            .find(|capture| capture.name == name)
                            .map(|capture| capture.java_type.clone())
                    }?;
                    closure_ctx.add_capture(name.clone(), java_type.clone(), 0);
                    Some((name, java_type))
                })
                .collect();

            let closure_bytes = generate_closure_class_bytes(
                &closure_ctx,
                &params,
                &closure_expr.body,
                closure_classes,
            )?;
            closure_classes.insert(nested_class_name.clone(), closure_bytes);

            let class_idx = cp.add_class(&nested_class_name);
            bytecode.push(0xBB); // NEW
            bytecode.extend_from_slice(&class_idx.to_be_bytes());
            bytecode.push(0x59); // DUP
            bytecode.push(0x2A); // aload_0 (owner)
            bytecode.push(0x2A); // aload_0 (thisObject)
            for (name, expected_type) in &nested_captures {
                let actual_type = compile_closure_expr(
                    bytecode,
                    cp,
                    &Expression::Identifier(name.clone()),
                    param_slots,
                    captures,
                    class_name,
                    closure_classes,
                )?;
                if actual_type != *expected_type {
                    return Err(format!(
                        "nested closure capture {name} changed type from {expected_type:?} to {actual_type:?}"
                    ));
                }
            }
            let mut init_desc = "(Ljava/lang/Object;Ljava/lang/Object;".to_string();
            for (_, java_type) in &nested_captures {
                init_desc.push_str(&java_type.descriptor());
            }
            init_desc.push_str(")V");
            let init_ref = cp.add_method_ref(&nested_class_name, "<init>", &init_desc);
            bytecode.push(0xB7); // INVOKESPECIAL
            bytecode.extend_from_slice(&init_ref.to_be_bytes());
            Ok(JavaType::Object("groovy/lang/Closure".to_string()))
        }

        Expression::BinaryOp {
            operator,
            left,
            right,
        } => {
            if operator == "=" {
                if let Expression::Identifier(name) = left.as_ref() {
                    // Compile assignment as a dynamic setter call on the closure delegate/owner.
                    // Receiver: this (closure), Arg: value
                    bytecode.push(0x2A); // aload_0
                    let value_type = compile_closure_expr(
                        bytecode,
                        cp,
                        right,
                        param_slots,
                        captures,
                        class_name,
                        closure_classes,
                    )?;
                    box_closure_value(bytecode, cp, &value_type)?;

                    let bootstrap_index = ensure_groovy_bootstrap(cp);
                    let setter_name = property_setter_name(name);
                    let descriptor = "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
                    emit_invokedynamic(bytecode, cp, &setter_name, descriptor, bootstrap_index);
                    return Ok(JavaType::Object("java/lang/Object".to_string()));
                }
            }

            let (opcode, is_comparison) = match operator.as_str() {
                "+" => (0x60, false),   // iadd
                "-" => (0x64, false),   // isub
                "*" => (0x68, false),   // imul
                "/" => (0x6C, false),   // idiv
                "%" => (0x70, false),   // irem
                "&" => (0x7E, false),   // iand
                "|" => (0x80, false),   // ior
                "^" => (0x82, false),   // ixor
                "<<" => (0x78, false),  // ishl
                ">>" => (0x7A, false),  // ishr
                ">>>" => (0x7C, false), // iushr
                ">" => (0xA3, true),    // if_icmpgt
                "<" => (0xA1, true),    // if_icmplt
                ">=" => (0xA2, true),   // if_icmpge
                "<=" => (0xA4, true),   // if_icmple
                "==" => (0x9F, true),   // if_icmpeq
                "!=" => (0xA0, true),   // if_icmpne
                _ => {
                    return Err(format!(
                        "unsupported operator in generated closure: {operator}"
                    ))
                }
            };

            // Compile left operand
            let left_type = compile_closure_expr(
                bytecode,
                cp,
                left,
                param_slots,
                captures,
                class_name,
                closure_classes,
            )?;
            coerce_closure_int(bytecode, cp, &left_type, operator)?;
            // Compile right operand
            let right_type = compile_closure_expr(
                bytecode,
                cp,
                right,
                param_slots,
                captures,
                class_name,
                closure_classes,
            )?;
            coerce_closure_int(bytecode, cp, &right_type, operator)?;
            if is_comparison {
                // The raw closure compiler has no label allocator, so comparisons
                // use one fixed branch shape and box the result as Boolean.
                emit_int_compare(bytecode, cp, opcode);
                Ok(JavaType::Object("java/lang/Boolean".to_string()))
            } else {
                bytecode.push(opcode);
                Ok(JavaType::Int)
            }
        }

        Expression::Parenthesized(inner) => compile_closure_expr(
            bytecode,
            cp,
            inner,
            param_slots,
            captures,
            class_name,
            closure_classes,
        ),

        Expression::PropertyAccess {
            object, property, ..
        } => {
            // Compile the object expression (receiver for property access)
            let object_type = compile_closure_expr(
                bytecode,
                cp,
                object,
                param_slots,
                captures,
                class_name,
                closure_classes,
            )?;
            box_closure_value(bytecode, cp, &object_type)?;

            // Push property name as String constant
            let property_str_idx = cp.add_string(property);
            emit_ldc(bytecode, property_str_idx);

            // invokedynamic getProperty — same pattern as unknown variable resolution (line 514)
            // The bootstrap resolves getProperty("foo") → getFoo() on the receiver,
            // which routes through the gradle native bridge for gradle/* objects.
            let bootstrap_index = ensure_groovy_bootstrap(cp);
            let descriptor = "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;";
            emit_invokedynamic(bytecode, cp, "getProperty", descriptor, bootstrap_index);
            Ok(JavaType::Object("java/lang/Object".to_string()))
        }

        Expression::IndexAccess { object, index } => {
            // Compile the object expression (e.g., map or list)
            let object_type = compile_closure_expr(
                bytecode,
                cp,
                object,
                param_slots,
                captures,
                class_name,
                closure_classes,
            )?;
            box_closure_value(bytecode, cp, &object_type)?;

            // Compile the index expression
            let index_type = compile_closure_expr(
                bytecode,
                cp,
                index,
                param_slots,
                captures,
                class_name,
                closure_classes,
            )?;
            box_closure_value(bytecode, cp, &index_type)?;

            // Emit dynamic getAt call for index access
            let bootstrap_index = ensure_groovy_bootstrap(cp);
            let descriptor = "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
            emit_invokedynamic(bytecode, cp, "getAt", descriptor, bootstrap_index);
            Ok(JavaType::Object("java/lang/Object".to_string()))
        }

        Expression::MapLiteral(entries) => {
            // Create a LinkedHashMap to preserve insertion order
            let hashmap_class = cp.add_class("java/util/LinkedHashMap");
            let hashmap_init = cp.add_method_ref("java/util/LinkedHashMap", "<init>", "()V");
            let put_method = cp.add_method_ref(
                "java/util/LinkedHashMap",
                "put",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            );

            // NEW LinkedHashMap
            bytecode.push(0xBB);
            bytecode.extend_from_slice(&hashmap_class.to_be_bytes());
            // DUP
            bytecode.push(0x59);
            // INVOKESPECIAL <init>
            bytecode.push(0xB7);
            bytecode.extend_from_slice(&hashmap_init.to_be_bytes());

            // For each entry, put key-value pair
            // Note: MapLiteral keys are Strings in this AST
            for (key, value) in entries {
                // DUP the map reference
                bytecode.push(0x59);

                // Compile key as string literal
                let str_idx = cp.add_string(key);
                emit_ldc(bytecode, str_idx);

                // Compile value
                let value_type = compile_closure_expr(
                    bytecode,
                    cp,
                    value,
                    param_slots,
                    captures,
                    class_name,
                    closure_classes,
                )?;
                box_closure_value(bytecode, cp, &value_type)?;

                // Call put(key, value)
                bytecode.push(0xB6); // invokevirtual
                bytecode.extend_from_slice(&put_method.to_be_bytes());
                // Pop the return value of put()
                bytecode.push(0x57); // pop
            }
            // Map reference remains on stack
            Ok(JavaType::Object("java/util/LinkedHashMap".to_string()))
        }

        Expression::ListLiteral(elements) => {
            // Create an ArrayList
            let arraylist_class = cp.add_class("java/util/ArrayList");
            let arraylist_init = cp.add_method_ref("java/util/ArrayList", "<init>", "()V");
            let add_method =
                cp.add_method_ref("java/util/ArrayList", "add", "(Ljava/lang/Object;)Z");

            // NEW ArrayList
            bytecode.push(0xBB);
            bytecode.extend_from_slice(&arraylist_class.to_be_bytes());
            // DUP
            bytecode.push(0x59);
            // INVOKESPECIAL <init>
            bytecode.push(0xB7);
            bytecode.extend_from_slice(&arraylist_init.to_be_bytes());

            // For each element, add to list
            for elem in elements {
                // DUP the list reference
                bytecode.push(0x59);

                // Compile element
                let element_type = compile_closure_expr(
                    bytecode,
                    cp,
                    elem,
                    param_slots,
                    captures,
                    class_name,
                    closure_classes,
                )?;
                box_closure_value(bytecode, cp, &element_type)?;

                // Call add(element)
                bytecode.push(0xB6); // invokevirtual
                bytecode.extend_from_slice(&add_method.to_be_bytes());
                // Pop the boolean return value of add()
                bytecode.push(0x57); // pop
            }
            // List reference remains on stack
            Ok(JavaType::Object("java/util/ArrayList".to_string()))
        }

        _ => Err(format!(
            "unsupported expression in generated closure: {expr:?}"
        )),
    }
}

/// Adapt a naturally compiled closure value to an `Object` boundary.
///
/// Generated `doCall` methods, dynamic-call arguments, and collection elements
/// all use reference descriptors. Keeping this adaptation in one place prevents
/// primitive values from leaking into `ARETURN` and `Object` parameter slots.
pub(super) fn box_closure_value(
    bytecode: &mut Vec<u8>,
    cp: &mut crate::constant_pool::ConstantPoolBuilder,
    java_type: &JavaType,
) -> Result<(), String> {
    let (wrapper, descriptor) = match java_type {
        JavaType::Int => ("java/lang/Integer", "(I)Ljava/lang/Integer;"),
        JavaType::Long => ("java/lang/Long", "(J)Ljava/lang/Long;"),
        JavaType::Float => ("java/lang/Float", "(F)Ljava/lang/Float;"),
        JavaType::Double => ("java/lang/Double", "(D)Ljava/lang/Double;"),
        JavaType::Boolean => ("java/lang/Boolean", "(Z)Ljava/lang/Boolean;"),
        JavaType::Byte => ("java/lang/Byte", "(B)Ljava/lang/Byte;"),
        JavaType::Short => ("java/lang/Short", "(S)Ljava/lang/Short;"),
        JavaType::Char => ("java/lang/Character", "(C)Ljava/lang/Character;"),
        JavaType::Object(_) | JavaType::Array { .. } => return Ok(()),
        JavaType::Void => return Err("generated closure expression produced void".to_string()),
    };
    let value_of = cp.add_method_ref(wrapper, "valueOf", descriptor);
    bytecode.push(0xB8); // invokestatic
    bytecode.extend_from_slice(&value_of.to_be_bytes());
    Ok(())
}

/// Adapt an expression to the bounded integer operator subset used by generated
/// closures. Parameters are declared as Object and are unboxed only here, so the
/// same parameter remains usable as a receiver or returned value elsewhere.
fn coerce_closure_int(
    bytecode: &mut Vec<u8>,
    cp: &mut crate::constant_pool::ConstantPoolBuilder,
    java_type: &JavaType,
    operator: &str,
) -> Result<(), String> {
    match java_type {
        JavaType::Int | JavaType::Byte | JavaType::Short | JavaType::Char => Ok(()),
        JavaType::Object(name) if name == "java/lang/Object" || name == "java/lang/Integer" => {
            let integer_class = cp.add_class("java/lang/Integer");
            bytecode.push(0xC0); // checkcast
            bytecode.extend_from_slice(&integer_class.to_be_bytes());
            let int_value = cp.add_method_ref("java/lang/Integer", "intValue", "()I");
            bytecode.push(0xB6); // invokevirtual
            bytecode.extend_from_slice(&int_value.to_be_bytes());
            Ok(())
        }
        _ => Err(format!(
            "generated closure operator {operator} requires integer operands, got {java_type:?}"
        )),
    }
}

fn emit_ldc(bytecode: &mut Vec<u8>, index: u16) {
    if index <= 0xFF {
        bytecode.push(0x12); // ldc
        bytecode.push(index as u8);
    } else {
        bytecode.push(0x13); // ldc_w
        bytecode.extend_from_slice(&index.to_be_bytes());
    }
}

/// Emit an integer comparison that leaves a boxed `Boolean` on the operand stack.
///
/// Both operands are `int`s. The raw expression emitter has no label allocator,
/// so this uses the fixed shape `if_icmp<cond>; false; goto; true`, then boxes
/// the result for the closure's Object-returning contract.
fn emit_int_compare(
    bytecode: &mut Vec<u8>,
    cp: &mut crate::constant_pool::ConstantPoolBuilder,
    if_op: u8,
) {
    bytecode.push(if_op);
    bytecode.extend_from_slice(&[0x00, 0x07]); // true -> iconst_1
    bytecode.push(0x03); // iconst_0
    bytecode.extend_from_slice(&[0xA7, 0x00, 0x04]); // goto past iconst_1
    bytecode.push(0x04); // iconst_1
    let value_of = cp.add_method_ref("java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;");
    bytecode.push(0xB8); // invokestatic
    bytecode.extend_from_slice(&value_of.to_be_bytes());
}

fn ensure_groovy_bootstrap(cp: &mut crate::constant_pool::ConstantPoolBuilder) -> u16 {
    let bootstrap_class = cp.add_class("org/codehaus/groovy/vmplugin/v8/IndyInterface");
    let bootstrap_method_nat = cp.add_name_and_type(
        "bootstrap",
        "(Ljava/lang/invoke/MethodHandles$Lookup;Ljava/lang/String;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/CallSite;",
    );
    let bootstrap_method_ref = cp.add_method_ref_by_indices(bootstrap_class, bootstrap_method_nat);
    let method_handle = cp.add_method_handle(6, bootstrap_method_ref);
    cp.register_bootstrap_method(method_handle)
}

fn emit_invokedynamic(
    bytecode: &mut Vec<u8>,
    cp: &mut crate::constant_pool::ConstantPoolBuilder,
    method_name: &str,
    descriptor: &str,
    bootstrap_index: u16,
) {
    let indy_idx = cp.add_invoke_dynamic(bootstrap_index, method_name, descriptor);
    bytecode.push(0xBA); // invokedynamic
    bytecode.extend_from_slice(&indy_idx.to_be_bytes());
    bytecode.extend_from_slice(&[0x00, 0x00]); // reserved bytes
}
