// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy conditional-expression lowering with truth and type-safe merges.

use super::GroovyAstCompiler;
use crate::java_compiler::JavaType;
use hyperion_groovy::Expression;

impl GroovyAstCompiler {
    pub(super) fn compile_conditional_expression(
        &mut self,
        condition: &Expression,
        then_expr: &Expression,
        else_expr: &Expression,
    ) -> Option<JavaType> {
        let result_slot = self.reserve_control_result_slot()?;
        let condition_type = self.compile_expression(condition)?;
        self.coerce_to_truth_int(&condition_type)?;

        let else_label = self.stmt_compiler.new_label();
        let end_label = self.stmt_compiler.new_label();
        self.stmt_compiler.emit_ifeq(else_label);

        self.compile_control_result(then_expr, result_slot)?;
        self.stmt_compiler.emit_goto(end_label);

        self.stmt_compiler.bind_label(else_label);
        self.compile_control_result(else_expr, result_slot)?;
        self.stmt_compiler.bind_label(end_label);
        self.stmt_compiler
            .expr_compiler()
            .compile_aload(result_slot, "java/lang/Object");
        Some(JavaType::Object("java/lang/Object".to_string()))
    }

    pub(super) fn compile_elvis_expression(
        &mut self,
        primary: &Expression,
        fallback: &Expression,
    ) -> Option<JavaType> {
        let result_slot = self.reserve_control_result_slot()?;
        self.compile_control_result(primary, result_slot)?;

        self.stmt_compiler
            .expr_compiler()
            .compile_aload(result_slot, "java/lang/Object");
        self.coerce_to_truth_int(&JavaType::Object("java/lang/Object".to_string()))?;

        let fallback_label = self.stmt_compiler.new_label();
        let end_label = self.stmt_compiler.new_label();
        self.stmt_compiler.emit_ifeq(fallback_label);
        self.stmt_compiler.emit_goto(end_label);

        self.stmt_compiler.bind_label(fallback_label);
        self.compile_control_result(fallback, result_slot)?;
        self.stmt_compiler.bind_label(end_label);
        self.stmt_compiler
            .expr_compiler()
            .compile_aload(result_slot, "java/lang/Object");
        Some(JavaType::Object("java/lang/Object".to_string()))
    }

    fn reserve_control_result_slot(&mut self) -> Option<u16> {
        self.reserve_local(
            &JavaType::Object("java/lang/Object".to_string()),
            "control-expression temporary",
        )
    }

    fn compile_control_result(&mut self, expression: &Expression, slot: u16) -> Option<()> {
        let result_type = self.compile_expression(expression)?;
        if result_type == JavaType::Void {
            self.reject_lowering("void expression cannot be a conditional result");
            return None;
        }
        self.compile_box_primitive(&result_type);
        self.stmt_compiler.expr_compiler().compile_astore(slot);
        Some(())
    }
}
