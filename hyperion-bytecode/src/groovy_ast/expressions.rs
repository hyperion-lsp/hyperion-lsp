// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Expression compilation

use super::{
    is_null_expression, types::parse_groovy_type_annotation, GroovyAstCompiler, VariableInfo,
};
use crate::groovy_compiler::{validate_dynamic_call_arity, BinaryOp, UnaryOp};
use crate::java_compiler::JavaType;
use hyperion_groovy::{Expression, GStringPart};
use tracing::trace;

impl GroovyAstCompiler {
    /// Return whether compiling this expression is known to produce a String.
    ///
    /// This is deliberately side-effect free: binary `+` must choose arithmetic
    /// versus concatenation before emitting either operand. It covers the syntax
    /// and local-variable cases whose type is authoritative; dynamic calls remain
    /// unknown rather than being guessed.
    fn is_statically_string_expression(&self, expr: &Expression) -> bool {
        match expr {
            Expression::StringLiteral(_) | Expression::GString(_) => true,
            Expression::Identifier(name) | Expression::Variable(name) => {
                self.locals.get(name).is_some_and(|local| {
                    matches!(
                        &local.java_type,
                        JavaType::Object(class_name) if class_name == "java/lang/String"
                    )
                })
            }
            Expression::BinaryOp {
                operator,
                left,
                right,
            } if operator == "+" => {
                self.is_statically_string_expression(left)
                    || self.is_statically_string_expression(right)
            }
            Expression::Parenthesized(inner) => self.is_statically_string_expression(inner),
            Expression::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                self.is_statically_string_expression(then_expr)
                    && self.is_statically_string_expression(else_expr)
            }
            Expression::ElvisOp { primary, fallback } => {
                self.is_statically_string_expression(primary)
                    && self.is_statically_string_expression(fallback)
            }
            _ => false,
        }
    }

    /// Emit dynamic-call arguments in source order and adapt each value to the
    /// `Object` descriptor used by Groovy call sites.
    fn compile_dynamic_call_arguments(&mut self, arguments: &[Expression]) -> Option<()> {
        for argument in arguments {
            self.compile_object_boundary(argument, "a method argument")?;
        }
        Some(())
    }

    fn validate_dynamic_call_arity(&mut self, arg_count: usize) -> Option<()> {
        if let Err(message) = validate_dynamic_call_arity(arg_count) {
            self.reject_lowering(message);
            return None;
        }
        Some(())
    }

    fn emit_dynamic_method_call(
        &mut self,
        method_name: &str,
        arg_count: usize,
    ) -> Option<JavaType> {
        match self
            .stmt_compiler
            .expr_compiler()
            .compile_dynamic_method_call(method_name, arg_count)
        {
            Ok(java_type) => Some(java_type),
            Err(message) => {
                self.reject_lowering(message);
                None
            }
        }
    }

    fn compile_object_boundary(&mut self, expression: &Expression, context: &str) -> Option<()> {
        let value_type = self.compile_expression(expression)?;
        if value_type == JavaType::Void {
            self.reject_lowering(format!("void expression cannot be used as {context}"));
            return None;
        }
        self.compile_box_primitive(&value_type);
        Some(())
    }

    pub fn compile_expression(&mut self, expr: &Expression) -> Option<JavaType> {
        trace!("compile_expression called with: {:?}", expr);
        match expr {
            Expression::IntLiteral(value) => {
                let Ok(value) = i32::try_from(*value) else {
                    self.reject_lowering(format!(
                        "Groovy int literal {value} exceeds the JVM int range"
                    ));
                    return None;
                };
                let java_type = self
                    .stmt_compiler
                    .expr_compiler()
                    .compile_int_literal(value);
                Some(java_type)
            }

            Expression::LongLiteral(value) => {
                // Compile as long constant
                let cp = self.stmt_compiler.expr_compiler().constant_pool();
                let cp_index = cp.add_long(*value);
                self.stmt_compiler.expr_compiler().emit_byte(0x14); // LDC2_W
                self.stmt_compiler.expr_compiler().emit_short(cp_index);
                self.stmt_compiler.expr_compiler().stack_depth().push2(); // long takes 2 slots
                Some(JavaType::Long)
            }

            Expression::FloatLiteral(value) => {
                // Compile as float constant
                let narrowed = *value as f32;
                if !value.is_finite() || !narrowed.is_finite() || (*value != 0.0 && narrowed == 0.0)
                {
                    self.reject_lowering(format!(
                        "Groovy float literal {value} is outside the finite nonzero JVM float range"
                    ));
                    return None;
                }
                let cp = self.stmt_compiler.expr_compiler().constant_pool();
                let cp_index = cp.add_float(narrowed);
                if cp_index < 256 {
                    self.stmt_compiler.expr_compiler().emit_byte(0x12); // LDC
                    self.stmt_compiler.expr_compiler().emit_byte(cp_index as u8);
                } else {
                    self.stmt_compiler.expr_compiler().emit_byte(0x13); // LDC_W
                    self.stmt_compiler.expr_compiler().emit_short(cp_index);
                }
                self.stmt_compiler.expr_compiler().stack_depth().push1(); // float takes 1 slot
                Some(JavaType::Float)
            }

            Expression::DoubleLiteral(value) => {
                // Compile as double constant
                if !value.is_finite() {
                    self.reject_lowering(format!(
                        "Groovy double literal {value} is outside the finite JVM double range"
                    ));
                    return None;
                }
                let cp = self.stmt_compiler.expr_compiler().constant_pool();
                let cp_index = cp.add_double(*value);
                self.stmt_compiler.expr_compiler().emit_byte(0x14); // LDC2_W
                self.stmt_compiler.expr_compiler().emit_short(cp_index);
                self.stmt_compiler.expr_compiler().stack_depth().push2(); // double takes 2 slots
                Some(JavaType::Double)
            }

            Expression::BooleanLiteral(value) => {
                let java_type = self
                    .stmt_compiler
                    .expr_compiler()
                    .compile_boolean_literal(*value);
                Some(java_type)
            }

            Expression::StringLiteral(value) => {
                let java_type = self
                    .stmt_compiler
                    .expr_compiler()
                    .compile_string_literal(value);
                Some(java_type)
            }

            Expression::NullLiteral => {
                let java_type = self.stmt_compiler.expr_compiler().compile_null_literal();
                Some(java_type)
            }

            Expression::Identifier(name) | Expression::Variable(name) => {
                trace!(
                    "Looking up identifier: '{}', locals has {} entries",
                    name,
                    self.locals.len()
                );
                for (k, v) in &self.locals {
                    trace!("local '{}' -> slot={}, type={:?}", k, v.slot, v.java_type);
                }
                if let Some(var_info) = self.locals.get(name) {
                    trace!(
                        "FOUND: slot={}, type={:?}",
                        var_info.slot,
                        var_info.java_type
                    );
                    // Load variable from local slot
                    match &var_info.java_type {
                        JavaType::Byte
                        | JavaType::Short
                        | JavaType::Int
                        | JavaType::Boolean
                        | JavaType::Char => {
                            self.stmt_compiler
                                .expr_compiler()
                                .compile_iload(var_info.slot);
                        }
                        JavaType::Long => {
                            self.stmt_compiler
                                .expr_compiler()
                                .compile_lload(var_info.slot);
                        }
                        JavaType::Float => {
                            self.stmt_compiler
                                .expr_compiler()
                                .compile_fload(var_info.slot);
                        }
                        JavaType::Double => {
                            self.stmt_compiler
                                .expr_compiler()
                                .compile_dload(var_info.slot);
                        }
                        JavaType::Object(type_name) => {
                            self.stmt_compiler
                                .expr_compiler()
                                .compile_aload(var_info.slot, type_name);
                        }
                        JavaType::Array { .. } => {
                            // For arrays, we need to get the type name for the aload
                            // Use descriptor() which gives us the proper array signature
                            let type_descriptor = var_info.java_type.descriptor();
                            self.stmt_compiler
                                .expr_compiler()
                                .compile_aload(var_info.slot, &type_descriptor);
                        }
                        JavaType::Void => {
                            // Void locals are rejected before allocation (statements.rs), so this
                            // is unreachable. Report it rather than leave the stack unbalanced.
                            self.reject_lowering("cannot load a void-typed local");
                            return None;
                        }
                    }
                    Some(var_info.java_type.clone())
                } else {
                    trace!("NOT FOUND in locals!");
                    trace!(
                        "Available variables: {:?}",
                        self.locals.keys().collect::<Vec<_>>()
                    );
                    // Variable not found in locals.
                    //
                    // Check if this looks like a class reference (starts with uppercase).
                    // For static method calls like Math.max(), we push the class name as a
                    // String so the MOP can resolve it, rather than trying to load 'this'.
                    let first_char = name.chars().next();
                    if first_char.is_some_and(|c| c.is_ascii_uppercase()) {
                        // Likely a class reference (Math, System, String, etc.)
                        // Push the class name as a String for MOP resolution
                        let java_type = self
                            .stmt_compiler
                            .expr_compiler()
                            .compile_string_literal(name);
                        Some(java_type)
                    } else {
                        // Regular identifier - try dynamic property access on 'this'
                        // This requires 'this' to be available (instance method context)
                        self.stmt_compiler
                            .expr_compiler()
                            .compile_aload(0, "java/lang/Object");
                        Some(
                            self.stmt_compiler
                                .expr_compiler()
                                .compile_dynamic_property_get(name),
                        )
                    }
                }
            }

            Expression::BinaryOp {
                operator,
                left,
                right,
            } => {
                // The `**` power operator has no `BinaryOp` mapping (Groovy lowers
                // it to `power`); handle it before the operator table.
                if operator == "**" {
                    return self.compile_power_op(left, right);
                }
                // Map operator string to BinaryOp enum
                let op = match operator.as_str() {
                    "+" => Some(BinaryOp::Add),
                    "-" => Some(BinaryOp::Sub),
                    "*" => Some(BinaryOp::Mul),
                    "/" => Some(BinaryOp::Div),
                    "%" => Some(BinaryOp::Mod),
                    "&" => Some(BinaryOp::BitwiseAnd),
                    "|" => Some(BinaryOp::BitwiseOr),
                    "^" => Some(BinaryOp::BitwiseXor),
                    "<<" => Some(BinaryOp::LeftShift),
                    ">>" => Some(BinaryOp::RightShift),
                    ">>>" => Some(BinaryOp::UnsignedRightShift),
                    "==" => Some(BinaryOp::Equal),
                    "!=" => Some(BinaryOp::NotEqual),
                    "<" => Some(BinaryOp::LessThan),
                    "<=" => Some(BinaryOp::LessThanOrEqual),
                    ">" => Some(BinaryOp::GreaterThan),
                    ">=" => Some(BinaryOp::GreaterThanOrEqual),
                    "&&" => Some(BinaryOp::LogicalAnd),
                    "||" => Some(BinaryOp::LogicalOr),
                    _ => None,
                };

                if let Some(binary_op) = op {
                    if matches!(binary_op, BinaryOp::Div) {
                        return self.compile_groovy_division(left, right);
                    }

                    // Handle logical operators with short-circuit semantics
                    if matches!(binary_op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                        return self.compile_logical_op(binary_op, left, right);
                    }

                    // Handle comparison operators
                    if matches!(
                        binary_op,
                        BinaryOp::Equal
                            | BinaryOp::NotEqual
                            | BinaryOp::LessThan
                            | BinaryOp::LessThanOrEqual
                            | BinaryOp::GreaterThan
                            | BinaryOp::GreaterThanOrEqual
                    ) {
                        return self.compile_comparison_op(binary_op, left, right);
                    }

                    // Handle string concatenation
                    if matches!(binary_op, BinaryOp::Add) {
                        let right_is_string = self.is_statically_string_expression(right);
                        // Compile the left operand once; its value stays on the
                        // operand stack. Groovy/Java `+` is concatenation when either
                        // operand is statically String. Looking at the right operand
                        // before emission is what makes `1 + "a"` legal without
                        // compiling/evaluating either side twice.
                        let left_type = self.compile_expression(left)?;
                        if right_is_string
                            || matches!(left_type, JavaType::Object(ref s) if s == "java/lang/String")
                        {
                            return self.compile_string_concat(&left_type, right);
                        }
                        // Continue with arithmetic path below
                        let right_type = self.compile_expression(right)?;
                        if Self::is_runtime_number_type(&left_type)
                            || Self::is_runtime_number_type(&right_type)
                        {
                            return self.compile_runtime_number_op(
                                binary_op,
                                &left_type,
                                &right_type,
                            );
                        }
                        let result_type =
                            self.promote_arithmetic_operands(&left_type, &right_type)?;
                        return self.compile_arithmetic_op(binary_op, result_type);
                    }

                    // Compile left operand
                    let left_type = self.compile_expression(left)?;

                    // Compile right operand
                    let right_type = self.compile_expression(right)?;

                    // Dispatch to appropriate handler
                    if matches!(
                        binary_op,
                        BinaryOp::BitwiseAnd
                            | BinaryOp::BitwiseOr
                            | BinaryOp::BitwiseXor
                            | BinaryOp::LeftShift
                            | BinaryOp::RightShift
                            | BinaryOp::UnsignedRightShift
                    ) {
                        let operand_type = if matches!(
                            binary_op,
                            BinaryOp::LeftShift
                                | BinaryOp::RightShift
                                | BinaryOp::UnsignedRightShift
                        ) {
                            self.prepare_shift_operands(&left_type, &right_type)?
                        } else {
                            self.promote_integral_operands(&left_type, &right_type)?
                        };
                        return self.compile_bitwise_op(binary_op, operand_type);
                    } else {
                        if Self::is_runtime_number_type(&left_type)
                            || Self::is_runtime_number_type(&right_type)
                        {
                            return self.compile_runtime_number_op(
                                binary_op,
                                &left_type,
                                &right_type,
                            );
                        }
                        let result_type =
                            self.promote_arithmetic_operands(&left_type, &right_type)?;
                        return self.compile_arithmetic_op(binary_op, result_type);
                    }
                }

                // Handle assignment operator "="
                if operator == "=" {
                    return self.compile_assignment(left, right);
                }

                self.reject_lowering(format!("unsupported Groovy binary operator `{operator}`"));
                None
            }

            Expression::UnaryOp { operator, operand } => {
                let op = match operator.as_str() {
                    "-" => UnaryOp::Neg,
                    "!" => UnaryOp::Not,
                    _ => {
                        self.reject_lowering(format!(
                            "unsupported Groovy unary operator `{operator}`"
                        ));
                        return None;
                    }
                };

                let operand_type = self.compile_expression(operand)?;
                let mut result_type = operand_type.clone();
                match op {
                    UnaryOp::Neg => {
                        if Self::is_runtime_number_type(&operand_type) {
                            result_type = self.compile_runtime_number_negate(&operand_type);
                            return Some(result_type);
                        }
                        // Select negation opcode based on operand type
                        match &operand_type {
                            JavaType::Long => self.stmt_compiler.expr_compiler().emit_byte(0x75),
                            JavaType::Float => self.stmt_compiler.expr_compiler().emit_byte(0x76),
                            JavaType::Double => self.stmt_compiler.expr_compiler().emit_byte(0x77),
                            JavaType::Int | JavaType::Byte | JavaType::Short | JavaType::Char => {
                                self.stmt_compiler.expr_compiler().emit_byte(0x74)
                                // INEG
                            }
                            _ => {
                                self.reject_lowering(format!(
                                    "Groovy unary negation requires a numeric operand, got {operand_type:?}"
                                ));
                                return None;
                            }
                        }
                    }
                    UnaryOp::Not => {
                        // Negate by Groovy truth: coerce the operand to a
                        // truth value, then branch. A plain `ICONST_1; IXOR`
                        // would throw on a boxed Boolean and mis-handle a
                        // non-0/1 truthy int (`!5` must be `false`).
                        self.coerce_to_truth_int(&operand_type)?;
                        let true_label = self.stmt_compiler.new_label();
                        let end_label = self.stmt_compiler.new_label();
                        self.stmt_compiler.emit_ifeq(true_label); // falsy → result true
                        self.stmt_compiler.expr_compiler().emit_byte(0x03); // ICONST_0
                        self.stmt_compiler.expr_compiler().stack_depth().push1();
                        self.stmt_compiler.emit_goto(end_label);
                        self.stmt_compiler.bind_label(true_label);
                        self.stmt_compiler.expr_compiler().emit_byte(0x04); // ICONST_1
                        self.stmt_compiler.expr_compiler().stack_depth().push1();
                        self.stmt_compiler.bind_label(end_label);
                        // The result is a boolean; reporting `Boolean` makes
                        // it box for display as `true`/`false`.
                        result_type = JavaType::Boolean;
                    }
                }

                Some(result_type)
            }

            Expression::GString(parts) => {
                // PHASE 1.6: GString interpolation compilation
                //
                // Architecture:
                // 1. Build array of string parts by inserting empty strings between interpolations
                // 2. Compile each interpolation expression, leaving values on stack
                // 3. Call compile_gstring which expects: value0, value1, ..., valueN on stack
                //
                // Example: "x=${x}, y=${y}"
                //   Parts: [Text("x="), Interpolation(x), Text(", y="), Interpolation(y)]
                //   String array: ["x=", ", y=", ""]
                //   Stack before compile_gstring: [x_value, y_value]
                //
                // compile_gstring requires strings.len() == expression_count + 1
                // So we build strings by: Text → string, Interpolation → compile + maybe add empty string

                trace!("Compiling GString with {} parts", parts.len());

                let mut string_parts = Vec::new();
                let mut interpolations = Vec::new();
                let mut last_was_interpolation = false;

                for part in parts {
                    match part {
                        GStringPart::Text(s) => {
                            string_parts.push(s.as_str());
                            last_was_interpolation = false;
                        }
                        GStringPart::Interpolation(expr) => {
                            // If previous part was also interpolation, insert empty string
                            if last_was_interpolation {
                                string_parts.push("");
                            }
                            interpolations.push(expr.as_ref());
                            last_was_interpolation = true;
                        }
                    }
                }

                // Ensure we have N+1 strings for N interpolations
                // If GString ends with interpolation, add empty string
                if last_was_interpolation {
                    string_parts.push("");
                }

                // If GString starts with interpolation, prepend empty string
                if !parts.is_empty() && matches!(parts[0], GStringPart::Interpolation(_)) {
                    string_parts.insert(0, "");
                }

                // Evaluate interpolations left-to-right and move each through
                // the Object boundary into an explicitly owned local slot.
                trace!("Compiling {} interpolations", interpolations.len());
                let mut interpolation_slots = Vec::with_capacity(interpolations.len());
                for (i, expr) in interpolations.iter().enumerate() {
                    trace!("Interpolation {}: {:?}", i, expr);
                    let result_type = self.compile_expression(expr)?;
                    if result_type == JavaType::Void {
                        self.reject_lowering("void expression cannot be interpolated in a GString");
                        return None;
                    }
                    self.compile_box_primitive(&result_type);
                    let slot = self.reserve_local(
                        &JavaType::Object("java/lang/Object".to_string()),
                        "GString interpolation temporary",
                    )?;
                    self.stmt_compiler.expr_compiler().compile_astore(slot);
                    interpolation_slots.push(slot);
                }

                trace!(
                    "Calling compile_gstring_from_locals with {} string parts and {} expressions",
                    string_parts.len(),
                    interpolations.len()
                );
                // Render from the explicitly owned Object locals.
                let java_type = self
                    .stmt_compiler
                    .expr_compiler()
                    .compile_gstring_from_locals(&string_parts, &interpolation_slots);

                trace!("compile_gstring_from_locals returned: {:?}", java_type);
                Some(java_type)
            }

            Expression::MethodCall {
                receiver: Some(recv),
                method_name,
                arguments,
            } => {
                // Check if this is a static method call on a class (Math.max, System.out, etc.)
                // Pattern: Identifier starting with uppercase + method call
                let is_static_class_call = matches!(recv.as_ref(),
                    Expression::Identifier(name) | Expression::Variable(name)
                    if name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                        && !self.locals.contains_key(name)
                );

                let class_name_for_static = if is_static_class_call {
                    if let Expression::Identifier(name) | Expression::Variable(name) = recv.as_ref()
                    {
                        Some(name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(class_name) = class_name_for_static {
                    return self.compile_static_method_call(&class_name, method_name, arguments);
                }

                // Regular instance method call - compile and adapt receiver first.
                self.validate_dynamic_call_arity(arguments.len())?;
                self.compile_object_boundary(recv, "a method receiver")?;
                self.compile_dynamic_call_arguments(arguments)?;

                let result_type = self.emit_dynamic_method_call(method_name, arguments.len())?;

                Some(result_type)
            }

            Expression::MethodCall {
                receiver: None,
                method_name,
                arguments,
            } => {
                self.validate_dynamic_call_arity(arguments.len())?;

                // Check if method_name is a local variable (closure call pattern)
                // e.g., `def add = { a, b -> a + b }; add(3, 4)`
                if let Some(var_info) = self.locals.get(method_name).cloned() {
                    if matches!(var_info.java_type, JavaType::Object(ref t) if t.contains("Closure"))
                    {
                        // This is a closure call: load closure, then call doCall
                        trace!("CLOSURE CALL: {} is a closure variable", method_name);

                        // Load the closure reference
                        self.stmt_compiler
                            .expr_compiler()
                            .compile_aload(var_info.slot, "groovy/lang/Closure");

                        self.compile_dynamic_call_arguments(arguments)?;

                        // Call doCall on the closure
                        // doCall signature: (Ljava/lang/Object;Ljava/lang/Object;...)Ljava/lang/Object;
                        let result_type =
                            self.emit_dynamic_method_call("doCall", arguments.len())?;

                        return Some(result_type);
                    }
                }

                // Method call without explicit receiver - treat as call on implicit 'this'
                // Load 'this' (local slot 0) as the receiver
                self.stmt_compiler
                    .expr_compiler()
                    .compile_aload(0, "java/lang/Object");

                self.compile_dynamic_call_arguments(arguments)?;

                // Emit invokedynamic with receiver + args
                let result_type = self.emit_dynamic_method_call(method_name, arguments.len())?;

                Some(result_type)
            }

            Expression::PropertyAccess {
                object,
                property,
                safe_navigation,
            } => {
                fn flatten_property_chain(expr: &Expression, parts: &mut Vec<String>) -> bool {
                    match expr {
                        Expression::Identifier(name) | Expression::Variable(name) => {
                            parts.push(name.clone());
                            true
                        }
                        Expression::PropertyAccess {
                            object, property, ..
                        } => {
                            if !flatten_property_chain(object, parts) {
                                return false;
                            }
                            parts.push(property.clone());
                            true
                        }
                        _ => false,
                    }
                }

                let mut chain = Vec::new();
                if flatten_property_chain(expr, &mut chain) {
                    if let (Some(first), Some(last)) = (chain.first(), chain.last()) {
                        if !self.locals.contains_key(first)
                            && last
                                .chars()
                                .next()
                                .is_some_and(|ch| ch.is_ascii_uppercase())
                        {
                            let dotted = chain.join(".");
                            let java_type = self
                                .stmt_compiler
                                .expr_compiler()
                                .compile_string_literal(&dotted);
                            return Some(java_type);
                        }
                    }
                }

                if *safe_navigation {
                    self.compile_safe_property_access(object, property)
                } else {
                    self.compile_object_boundary(object, "a property receiver")?;
                    // Standard property access (not safe navigation)
                    let result_type = self
                        .stmt_compiler
                        .expr_compiler()
                        .compile_dynamic_property_get(property);

                    Some(result_type)
                }
            }

            Expression::IndexAccess { object, index } => {
                self.compile_object_boundary(object, "an index receiver")?;
                self.compile_object_boundary(index, "an index key")?;

                // For now, emit a dynamic method call getAt(index)
                // This delegates array/map access to Groovy MOP
                let result_type = self.emit_dynamic_method_call("getAt", 1)?;

                Some(result_type)
            }

            Expression::Parenthesized(inner) => self.compile_expression(inner),

            Expression::ListLiteral(elements) => {
                // PHASE 4.7: List literal compilation
                //
                // Compiles: [1, 2, 3]
                // Into: new ArrayList(); list.add(1); list.add(2); list.add(3)
                //
                // Bytecode pattern:
                //   NEW ArrayList
                //   DUP
                //   INVOKESPECIAL <init>()
                //   # For each element:
                //   DUP
                //   compile(element)
                //   INVOKEINTERFACE List.add(Object)Z
                //   POP  # Discard add() return value
                //   # Final stack: ArrayList

                // Create ArrayList instance
                let arraylist_class = self
                    .stmt_compiler
                    .expr_compiler()
                    .constant_pool()
                    .add_class("java/util/ArrayList");
                self.stmt_compiler.expr_compiler().emit_byte(0xBB); // NEW
                self.stmt_compiler
                    .expr_compiler()
                    .emit_short(arraylist_class);
                self.stmt_compiler.expr_compiler().stack_depth().push1();

                self.stmt_compiler.expr_compiler().emit_byte(0x59); // DUP
                self.stmt_compiler.expr_compiler().stack_depth().push1();

                // Call ArrayList.<init>()
                let init_ref = self
                    .stmt_compiler
                    .expr_compiler()
                    .constant_pool()
                    .add_method_ref("java/util/ArrayList", "<init>", "()V");
                self.stmt_compiler.expr_compiler().emit_byte(0xB7); // INVOKESPECIAL
                self.stmt_compiler.expr_compiler().emit_short(init_ref);
                self.stmt_compiler.expr_compiler().stack_depth().pop1();

                // Add each element
                for element in elements {
                    self.stmt_compiler.expr_compiler().emit_byte(0x59); // DUP list ref
                    self.stmt_compiler.expr_compiler().stack_depth().push1();

                    // Compile element expression and honor List.add's Object boundary.
                    let element_type = self.compile_expression(element)?;
                    if element_type == JavaType::Void {
                        self.reject_lowering("void expression cannot be used as a list element");
                        return None;
                    }
                    self.compile_box_primitive(&element_type);

                    // Call List.add(Object)
                    let add_ref = self
                        .stmt_compiler
                        .expr_compiler()
                        .constant_pool()
                        .add_interface_method_ref("java/util/List", "add", "(Ljava/lang/Object;)Z");
                    self.stmt_compiler.expr_compiler().emit_byte(0xB9); // INVOKEINTERFACE
                    self.stmt_compiler.expr_compiler().emit_short(add_ref);
                    self.stmt_compiler.expr_compiler().emit_byte(2); // count (1 arg + this)
                    self.stmt_compiler.expr_compiler().emit_byte(0); // padding

                    // Pop return value (boolean)
                    self.stmt_compiler.expr_compiler().emit_byte(0x57); // POP
                    self.stmt_compiler.expr_compiler().stack_depth().pop1();
                    self.stmt_compiler.expr_compiler().stack_depth().pop1(); // element popped by add
                }

                // Stack: ArrayList instance
                Some(JavaType::Object("java/util/ArrayList".to_string()))
            }

            Expression::MapLiteral(entries) => {
                // PHASE 4.7: Map literal compilation
                //
                // Compiles: [a: 1, b: 2]
                // Into: new LinkedHashMap(); map.put("a", 1); map.put("b", 2)
                //
                // Bytecode pattern:
                //   NEW LinkedHashMap
                //   DUP
                //   INVOKESPECIAL <init>()
                //   # For each entry:
                //   DUP
                //   LDC key (String)
                //   compile(value)
                //   INVOKEINTERFACE Map.put(Object, Object)Object
                //   POP  # Discard put() return value
                //   # Final stack: LinkedHashMap

                // Create LinkedHashMap instance
                let hashmap_class = self
                    .stmt_compiler
                    .expr_compiler()
                    .constant_pool()
                    .add_class("java/util/LinkedHashMap");
                self.stmt_compiler.expr_compiler().emit_byte(0xBB); // NEW
                self.stmt_compiler.expr_compiler().emit_short(hashmap_class);
                self.stmt_compiler.expr_compiler().stack_depth().push1();

                self.stmt_compiler.expr_compiler().emit_byte(0x59); // DUP
                self.stmt_compiler.expr_compiler().stack_depth().push1();

                // Call LinkedHashMap.<init>()
                let init_ref = self
                    .stmt_compiler
                    .expr_compiler()
                    .constant_pool()
                    .add_method_ref("java/util/LinkedHashMap", "<init>", "()V");
                self.stmt_compiler.expr_compiler().emit_byte(0xB7); // INVOKESPECIAL
                self.stmt_compiler.expr_compiler().emit_short(init_ref);
                self.stmt_compiler.expr_compiler().stack_depth().pop1();

                // Put each entry
                for (key, value) in entries {
                    self.stmt_compiler.expr_compiler().emit_byte(0x59); // DUP map ref
                    self.stmt_compiler.expr_compiler().stack_depth().push1();

                    // Compile key (String)
                    self.stmt_compiler
                        .expr_compiler()
                        .compile_string_literal(key);

                    // Compile value expression and honor Map.put's Object boundary.
                    let value_type = self.compile_expression(value)?;
                    if value_type == JavaType::Void {
                        self.reject_lowering("void expression cannot be used as a map value");
                        return None;
                    }
                    self.compile_box_primitive(&value_type);

                    // Call Map.put(Object, Object)
                    let put_ref = self
                        .stmt_compiler
                        .expr_compiler()
                        .constant_pool()
                        .add_interface_method_ref(
                            "java/util/Map",
                            "put",
                            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                        );
                    self.stmt_compiler.expr_compiler().emit_byte(0xB9); // INVOKEINTERFACE
                    self.stmt_compiler.expr_compiler().emit_short(put_ref);
                    self.stmt_compiler.expr_compiler().emit_byte(3); // count (2 args + this)
                    self.stmt_compiler.expr_compiler().emit_byte(0); // padding

                    // Pop return value (previous value or null)
                    self.stmt_compiler.expr_compiler().emit_byte(0x57); // POP
                    self.stmt_compiler.expr_compiler().stack_depth().pop1();
                    self.stmt_compiler.expr_compiler().stack_depth().pop1(); // value popped by put
                    self.stmt_compiler.expr_compiler().stack_depth().pop1(); // key popped by put
                }

                // Stack: LinkedHashMap instance
                Some(JavaType::Object("java/util/LinkedHashMap".to_string()))
            }

            Expression::RangeLiteral {
                start,
                end,
                exclusive,
            } => {
                // PHASE 4.3: Range literals (1..10, 1..<10)
                //
                // Groovy ranges compile to groovy.lang.IntRange (for integers)
                // or groovy.lang.ObjectRange (for other Comparable types)
                //
                // Evaluate each endpoint once into primitive temporaries, then
                // call Groovy's public IntRange(boolean inclusive, int from,
                // int to) constructor. Using its real descriptor keeps emitted
                // bytecode executable outside Hyperion's embedded VM too.

                // Compile start expression
                let start_type = self.compile_expression(start)?;
                if start_type != JavaType::Int {
                    self.reject_lowering(format!(
                        "Groovy IntRange lowering requires int endpoints, got start {start_type:?}"
                    ));
                    return None;
                }
                let temp_start = self.reserve_local(&JavaType::Int, "range endpoint temporary")?;
                self.compile_store(&JavaType::Int, temp_start);

                // Compile end expression
                let end_type = self.compile_expression(end)?;
                if end_type != JavaType::Int {
                    self.reject_lowering(format!(
                        "Groovy IntRange lowering requires int endpoints, got end {end_type:?}"
                    ));
                    return None;
                }
                let temp_end = self.reserve_local(&JavaType::Int, "range endpoint temporary")?;
                self.compile_store(&JavaType::Int, temp_end);

                // Create IntRange instance
                let intrange_class_idx = self
                    .stmt_compiler
                    .expr_compiler()
                    .constant_pool()
                    .add_class("groovy/lang/IntRange");
                self.stmt_compiler.expr_compiler().emit_byte(0xBB); // NEW
                self.stmt_compiler
                    .expr_compiler()
                    .emit_short(intrange_class_idx);
                self.stmt_compiler.expr_compiler().stack_depth().push1();

                self.stmt_compiler.expr_compiler().emit_byte(0x59); // DUP
                self.stmt_compiler.expr_compiler().stack_depth().push1();

                // Push constructor arguments in descriptor order.
                let inclusive = !exclusive;
                if inclusive {
                    self.stmt_compiler.expr_compiler().emit_byte(0x04); // ICONST_1
                } else {
                    self.stmt_compiler.expr_compiler().emit_byte(0x03); // ICONST_0
                }
                self.stmt_compiler.expr_compiler().stack_depth().push1();
                self.compile_load(&JavaType::Int, temp_start);
                self.compile_load(&JavaType::Int, temp_end);

                // Stack: range_ref, range_ref, inclusive, start, end

                // Call constructor: IntRange(boolean inclusive, int from, int to)
                let constructor_ref = self
                    .stmt_compiler
                    .expr_compiler()
                    .constant_pool()
                    .add_method_ref("groovy/lang/IntRange", "<init>", "(ZII)V");
                self.stmt_compiler.expr_compiler().emit_byte(0xB7); // INVOKESPECIAL
                self.stmt_compiler
                    .expr_compiler()
                    .emit_short(constructor_ref);

                // Pop constructor arguments
                self.stmt_compiler.expr_compiler().stack_depth().pop1(); // end
                self.stmt_compiler.expr_compiler().stack_depth().pop1(); // start
                self.stmt_compiler.expr_compiler().stack_depth().pop1(); // inclusive
                self.stmt_compiler.expr_compiler().stack_depth().pop1(); // uninitialized ref

                // Stack: IntRange instance

                Some(JavaType::Object("groovy/lang/IntRange".to_string()))
            }

            Expression::Conditional {
                condition,
                then_expr,
                else_expr,
            } => self.compile_conditional_expression(condition, then_expr, else_expr),

            Expression::ElvisOp { primary, fallback } => {
                self.compile_elvis_expression(primary, fallback)
            }

            Expression::Unsupported(feature) => {
                self.reject_lowering(feature);
                None
            }

            Expression::Closure(closure_expr) => self.compile_closure(
                &closure_expr.parameters,
                &closure_expr.body,
                closure_expr.implicit_it,
            ),

            Expression::NewObject {
                class_name,
                arguments,
            } => {
                let internal_class_name = match parse_groovy_type_annotation(class_name) {
                    Ok(JavaType::Object(class_name)) => class_name,
                    Ok(java_type) => {
                        self.reject_lowering(format!(
                            "Groovy constructor requires a class type, got {java_type:?}"
                        ));
                        return None;
                    }
                    Err(message) => {
                        self.reject_lowering(message);
                        return None;
                    }
                };
                if !arguments.is_empty() {
                    self.reject_lowering(format!(
                        "constructor resolution is unavailable for Groovy call: new {class_name}(...)"
                    ));
                    return None;
                }

                // Compile: new ClassName(args)
                // Bytecode:
                //   NEW ClassName
                //   DUP
                //   INVOKESPECIAL ClassName.<init>()V

                // Get class index
                let class_index = {
                    let cp = self.stmt_compiler.expr_compiler().constant_pool();
                    cp.add_class(&internal_class_name)
                };

                // NEW
                self.stmt_compiler.expr_compiler().emit_byte(0xBB); // NEW
                self.stmt_compiler.expr_compiler().emit_short(class_index);
                self.stmt_compiler.expr_compiler().stack_depth().push1();

                // DUP (for constructor call)
                self.stmt_compiler.expr_compiler().emit_byte(0x59); // DUP
                self.stmt_compiler.expr_compiler().stack_depth().push1();

                // Get methodref for <init>
                let methodref_index = {
                    let cp = self.stmt_compiler.expr_compiler().constant_pool();
                    let nat_idx = cp.add_name_and_type("<init>", "()V");
                    cp.add_method_ref_by_indices(class_index, nat_idx)
                };

                // INVOKESPECIAL
                self.stmt_compiler.expr_compiler().emit_byte(0xB7); // INVOKESPECIAL
                self.stmt_compiler
                    .expr_compiler()
                    .emit_short(methodref_index);
                self.stmt_compiler.expr_compiler().stack_depth().pop1(); // pops DUP'd ref

                // Stack: object reference
                Some(JavaType::Object(internal_class_name))
            }
        }
    }

    /// Compile assignment expression: left = right
    ///
    /// The left side must be an identifier (variable reference).
    /// The right side is evaluated and stored into the variable.
    ///
    /// For Gradle DSL compatibility: if the variable is not defined locally,
    /// the assignment is compiled as a setter call on the implicit 'this' (project).
    /// e.g., `group = 'com.example'` becomes `this.setGroup('com.example')`
    fn compile_assignment(&mut self, left: &Expression, right: &Expression) -> Option<JavaType> {
        // Get the variable name from the left side
        let var_name = match left {
            Expression::Identifier(name) => name.clone(),
            Expression::Variable(name) => name.clone(),
            _ => {
                self.reject_lowering(match left {
                    Expression::PropertyAccess { .. } => {
                        "Groovy property assignment is not supported"
                    }
                    Expression::IndexAccess { .. } => "Groovy index assignment is not supported",
                    _ => "Groovy assignment target is not supported",
                });
                return None;
            }
        };

        // Look up the variable in locals to get its slot
        if let Some(var_info) = self.locals.get(&var_name).cloned() {
            // Compile the right side expression (value to assign)
            let value_type = self.compile_expression(right)?;
            if value_type == JavaType::Void {
                self.reject_lowering("void expression cannot be assigned to a variable");
                return None;
            }

            if let Some(declared_type) = &var_info.declared_type {
                let declared_type = declared_type.clone();
                let value_is_null = is_null_expression(right);
                self.adapt_declared_initializer(&declared_type, &value_type, value_is_null)?;
                self.duplicate_top(&declared_type);
                self.compile_store(&declared_type, var_info.slot);
                return Some(declared_type);
            }

            // A dynamically typed local may widen from one JVM slot to two.
            // Relocate it instead of overwriting the following local's slot.
            let target_slot = if value_type.is_category2() && !var_info.java_type.is_category2() {
                self.reserve_local(&value_type, "widened variable assignment")?
            } else {
                var_info.slot
            };

            // Emit STORE based on the expression's actual type (what's on the stack),
            // not the variable's declared type. Groovy is dynamically typed — `def x = 0`
            // followed by `x = "hello"` is valid, and the STORE must match the stack.
            // Assignment is itself a Groovy expression, so preserve one copy as
            // the result while the store consumes the other.
            self.duplicate_top(&value_type);
            self.compile_store(&value_type, target_slot);

            // Update the variable's type to reflect the new value
            if value_type != var_info.java_type {
                self.locals.insert(
                    var_name,
                    VariableInfo {
                        slot: target_slot,
                        java_type: value_type.clone(),
                        declared_type: None,
                        scope_depth: var_info.scope_depth,
                    },
                );
            }

            Some(value_type)
        } else {
            // Variable not in locals - treat as property setter on implicit 'this'
            // This is how Gradle DSL works: `group = 'x'` calls `project.setGroup('x')`
            tracing::trace!(
                "Compiling assignment to '{}' as setter call on implicit this",
                var_name
            );

            // Load 'this' (slot 0) as receiver
            self.stmt_compiler
                .expr_compiler()
                .compile_aload(0, "java/lang/Object");

            self.compile_object_boundary(right, "a property value")?;

            // Build setter name: group -> setGroup, version -> setVersion
            let setter_name = property_setter_name(&var_name);

            // Emit dynamic setter call
            self.emit_dynamic_method_call(&setter_name, 1)?;

            Some(JavaType::Object("java/lang/Object".to_string()))
        }
    }
}

pub(super) fn property_setter_name(property: &str) -> String {
    let mut chars = property.chars();
    let Some(first) = chars.next() else {
        return "set".to_owned();
    };

    let mut setter = String::with_capacity("set".len() + property.len());
    setter.push_str("set");
    setter.extend(first.to_uppercase());
    setter.push_str(chars.as_str());
    setter
}

#[cfg(test)]
mod tests {
    use super::property_setter_name;

    #[test]
    fn property_setter_names_are_unicode_safe() {
        assert_eq!(property_setter_name("group"), "setGroup");
        assert_eq!(property_setter_name("über"), "setÜber");
        assert_eq!(property_setter_name("ßeta"), "setSSeta");
        assert_eq!(property_setter_name(""), "set");
    }
}
