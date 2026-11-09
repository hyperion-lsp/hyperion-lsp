// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Descriptor-safe lowering for statically recognized Groovy calls.

use super::GroovyAstCompiler;
use crate::java_compiler::JavaType;
use hyperion_groovy::Expression;

impl GroovyAstCompiler {
    pub(super) fn compile_static_method_call(
        &mut self,
        class_name: &str,
        method_name: &str,
        arguments: &[Expression],
    ) -> Option<JavaType> {
        match class_name {
            "Math" | "java.lang.Math" | "java/lang/Math" => {
                self.compile_math_method_call(method_name, arguments)
            }
            _ => {
                self.reject_lowering(format!(
                    "unsupported static Groovy call: {class_name}.{method_name}"
                ));
                None
            }
        }
    }

    fn compile_math_method_call(
        &mut self,
        method_name: &str,
        arguments: &[Expression],
    ) -> Option<JavaType> {
        match method_name {
            "max" | "min" if arguments.len() == 2 => {
                self.compile_promoted_math_binary(method_name, arguments, true)
            }
            "floorDiv" | "floorMod" | "addExact" if arguments.len() == 2 => {
                self.compile_promoted_math_binary(method_name, arguments, false)
            }
            "abs" if arguments.len() == 1 => {
                let argument_type = self.compile_expression(&arguments[0])?;
                let value_type = self.math_primitive_type(&argument_type)?;
                let descriptor = format!("({0}){0}", value_type.descriptor());
                Some(self.stmt_compiler.expr_compiler().compile_invokestatic(
                    "java/lang/Math",
                    method_name,
                    &descriptor,
                ))
            }
            "sqrt" | "floor" | "ceil" | "sin" | "cos" | "tan" | "log" | "log10" | "exp"
            | "cbrt" | "atan" | "asin" | "acos" | "toRadians" | "toDegrees"
                if arguments.len() == 1 =>
            {
                self.compile_double_math_call(method_name, arguments, JavaType::Double)
            }
            "pow" | "hypot" if arguments.len() == 2 => {
                self.compile_double_math_call(method_name, arguments, JavaType::Double)
            }
            "round" if arguments.len() == 1 => {
                let argument_type = self.compile_expression(&arguments[0])?;
                if matches!(argument_type, JavaType::Float) {
                    Some(self.stmt_compiler.expr_compiler().compile_invokestatic(
                        "java/lang/Math",
                        method_name,
                        "(F)I",
                    ))
                } else {
                    self.widen_math_value(&argument_type, &JavaType::Double)?;
                    Some(self.stmt_compiler.expr_compiler().compile_invokestatic(
                        "java/lang/Math",
                        method_name,
                        "(D)J",
                    ))
                }
            }
            "random" if arguments.is_empty() => {
                Some(self.stmt_compiler.expr_compiler().compile_invokestatic(
                    "java/lang/Math",
                    method_name,
                    "()D",
                ))
            }
            _ => {
                self.reject_lowering(format!(
                    "unsupported Math call or arity in Groovy bytecode: Math.{method_name}/{}",
                    arguments.len()
                ));
                None
            }
        }
    }

    fn compile_double_math_call(
        &mut self,
        method_name: &str,
        arguments: &[Expression],
        return_type: JavaType,
    ) -> Option<JavaType> {
        for argument in arguments {
            let argument_type = self.compile_expression(argument)?;
            self.widen_math_value(&argument_type, &JavaType::Double)?;
        }
        let descriptor = format!(
            "({}){}",
            "D".repeat(arguments.len()),
            return_type.descriptor()
        );
        Some(self.stmt_compiler.expr_compiler().compile_invokestatic(
            "java/lang/Math",
            method_name,
            &descriptor,
        ))
    }

    fn compile_promoted_math_binary(
        &mut self,
        method_name: &str,
        arguments: &[Expression],
        allow_floating_point: bool,
    ) -> Option<JavaType> {
        let (left_type, left_slot) = self.compile_static_argument(&arguments[0])?;
        let (right_type, right_slot) = self.compile_static_argument(&arguments[1])?;
        let promoted_type = self.promoted_math_type(&left_type, &right_type)?;
        if !allow_floating_point && matches!(promoted_type, JavaType::Float | JavaType::Double) {
            self.reject_lowering(format!(
                "Math.{method_name} requires integral arguments in Groovy bytecode"
            ));
            return None;
        }

        self.compile_load(&left_type, left_slot);
        self.widen_math_value(&left_type, &promoted_type)?;
        self.compile_load(&right_type, right_slot);
        self.widen_math_value(&right_type, &promoted_type)?;

        let value_descriptor = promoted_type.descriptor();
        let descriptor = format!("({value_descriptor}{value_descriptor}){value_descriptor}");
        Some(self.stmt_compiler.expr_compiler().compile_invokestatic(
            "java/lang/Math",
            method_name,
            &descriptor,
        ))
    }

    fn compile_static_argument(&mut self, expression: &Expression) -> Option<(JavaType, u16)> {
        let java_type = self.compile_expression(expression)?;
        if java_type.stack_width() == 0 {
            self.reject_lowering("void expression cannot be used as a static method argument");
            return None;
        }
        let slot = self.reserve_local(&java_type, "static-call temporary")?;
        self.compile_store(&java_type, slot);
        Some((java_type, slot))
    }

    fn promoted_math_type(&mut self, left: &JavaType, right: &JavaType) -> Option<JavaType> {
        let left = self.math_primitive_type(left)?;
        let right = self.math_primitive_type(right)?;
        let rank = |java_type: &JavaType| match java_type {
            JavaType::Int => 0,
            JavaType::Long => 1,
            JavaType::Float => 2,
            JavaType::Double => 3,
            _ => unreachable!("normalized Math primitive"),
        };
        if rank(&left) >= rank(&right) {
            Some(left)
        } else {
            Some(right)
        }
    }

    fn math_primitive_type(&mut self, java_type: &JavaType) -> Option<JavaType> {
        match java_type {
            JavaType::Byte | JavaType::Short | JavaType::Char | JavaType::Int => {
                Some(JavaType::Int)
            }
            JavaType::Long | JavaType::Float | JavaType::Double => Some(java_type.clone()),
            _ => {
                self.reject_lowering(format!(
                    "Math call requires primitive numeric arguments, found {java_type:?}"
                ));
                None
            }
        }
    }

    fn widen_math_value(&mut self, actual: &JavaType, expected: &JavaType) -> Option<()> {
        let actual = self.math_primitive_type(actual)?;
        let expected = self.math_primitive_type(expected)?;
        let (opcode, stack_delta) = match (&actual, &expected) {
            (left, right) if left == right => return Some(()),
            (JavaType::Int, JavaType::Long) => (0x85, 1), // I2L
            (JavaType::Int, JavaType::Float) => (0x86, 0), // I2F
            (JavaType::Int, JavaType::Double) => (0x87, 1), // I2D
            (JavaType::Long, JavaType::Float) => (0x89, -1), // L2F
            (JavaType::Long, JavaType::Double) => (0x8A, 0), // L2D
            (JavaType::Float, JavaType::Double) => (0x8D, 1), // F2D
            _ => {
                self.reject_lowering(format!(
                    "cannot coerce Math argument from {actual:?} to {expected:?}"
                ));
                return None;
            }
        };
        self.stmt_compiler.expr_compiler().emit_byte(opcode);
        match stack_delta {
            1 => self.stmt_compiler.expr_compiler().stack_depth().push1(),
            -1 => self.stmt_compiler.expr_compiler().stack_depth().pop1(),
            _ => {}
        }
        Some(())
    }
}
