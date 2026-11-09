// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Safe-navigation lowering with single evaluation and stack-safe branch merges.

use super::GroovyAstCompiler;
use crate::java_compiler::JavaType;
use hyperion_groovy::Expression;

impl GroovyAstCompiler {
    pub(super) fn compile_safe_property_access(
        &mut self,
        object: &Expression,
        property: &str,
    ) -> Option<JavaType> {
        let object_type = self.compile_expression(object)?;
        if object_type == JavaType::Void {
            self.reject_lowering("void expression cannot be a safe-navigation receiver");
            return None;
        }
        self.compile_box_primitive(&object_type);

        let receiver_slot = self.reserve_local(
            &JavaType::Object("java/lang/Object".to_string()),
            "safe-navigation temporary",
        )?;
        self.stmt_compiler
            .expr_compiler()
            .compile_astore(receiver_slot);

        let null_label = self.stmt_compiler.new_label();
        let end_label = self.stmt_compiler.new_label();
        self.stmt_compiler
            .expr_compiler()
            .compile_aload(receiver_slot, "java/lang/Object");
        self.stmt_compiler.emit_ifnull(null_label);

        self.stmt_compiler
            .expr_compiler()
            .compile_aload(receiver_slot, "java/lang/Object");
        self.stmt_compiler
            .expr_compiler()
            .compile_dynamic_property_get(property);
        self.stmt_compiler
            .expr_compiler()
            .compile_astore(receiver_slot);
        self.stmt_compiler.emit_goto(end_label);

        self.stmt_compiler.bind_label(null_label);
        self.stmt_compiler.expr_compiler().emit_byte(0x01); // aconst_null
        self.stmt_compiler.expr_compiler().stack_depth().push1();
        self.stmt_compiler
            .expr_compiler()
            .compile_astore(receiver_slot);

        self.stmt_compiler.bind_label(end_label);
        self.stmt_compiler
            .expr_compiler()
            .compile_aload(receiver_slot, "java/lang/Object");
        Some(JavaType::Object("java/lang/Object".to_string()))
    }
}
