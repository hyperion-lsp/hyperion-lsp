// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Binary operator compilation (arithmetic, bitwise, comparison, logical, string concat)

use super::GroovyAstCompiler;
use crate::groovy_compiler::{BinaryOp, Label};
use crate::groovy_runtime::{
    BINARY_NUMBER_DESCRIPTOR, COMPARE_NUMBER_DESCRIPTOR, NUMBERS_CLASS, TRUTH_CLASS,
    TRUTH_DESCRIPTOR, TRUTH_METHOD, UNARY_NUMBER_DESCRIPTOR,
};
use crate::java_compiler::JavaType;
use hyperion_groovy::Expression;

impl GroovyAstCompiler {
    /// Compile Groovy `/` through the shared numeric runtime. Unlike JVM
    /// `idiv`/`ldiv`, Groovy division produces a BigDecimal for integral
    /// operands and a Double when either operand is floating-point.
    pub(super) fn compile_groovy_division(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Option<JavaType> {
        let left_type = self.compile_expression(left)?;
        let right_type = self.compile_expression(right)?;
        self.compile_runtime_number_op(BinaryOp::Div, &left_type, &right_type)
    }

    /// Whether a statically known reference needs Groovy's boxed numeric path
    /// for subsequent arithmetic.
    pub(super) fn is_runtime_number_type(java_type: &JavaType) -> bool {
        matches!(
            java_type,
            JavaType::Object(name)
                if matches!(
                    name.as_str(),
                    "java/math/BigDecimal" | "java/lang/Double" | "java/lang/Float"
                )
        )
    }

    /// Replace the two already-compiled operands on the stack with one boxed
    /// runtime result. The right operand is temporarily stored so the buried
    /// left operand can be boxed without evaluating either expression twice.
    pub(super) fn compile_runtime_number_op(
        &mut self,
        op: BinaryOp,
        left_type: &JavaType,
        right_type: &JavaType,
    ) -> Option<JavaType> {
        let is_number = |java_type: &JavaType| {
            java_type.is_numeric() || Self::is_runtime_number_type(java_type)
        };
        if !is_number(left_type) || !is_number(right_type) {
            self.reject_lowering(format!(
                "Groovy arithmetic requires numeric operands, got {left_type:?} and {right_type:?}"
            ));
            return None;
        }

        let right_slot = self.reserve_local(right_type, "numeric operation temporary")?;
        self.compile_store(right_type, right_slot);
        self.compile_box_primitive(left_type);
        self.compile_load(right_type, right_slot);
        self.compile_box_primitive(right_type);

        let method_name = match op {
            BinaryOp::Add => "plus",
            BinaryOp::Sub => "minus",
            BinaryOp::Mul => "multiply",
            BinaryOp::Div => "divide",
            BinaryOp::Mod => "remainder",
            _ => unreachable!("non-arithmetic Groovy runtime operator"),
        };
        let method = self
            .stmt_compiler
            .expr_compiler()
            .constant_pool()
            .add_method_ref(NUMBERS_CLASS, method_name, BINARY_NUMBER_DESCRIPTOR);
        self.stmt_compiler.expr_compiler().emit_byte(0xB8); // INVOKESTATIC
        self.stmt_compiler.expr_compiler().emit_short(method);
        // two references -> one reference
        self.stmt_compiler.expr_compiler().stack_depth().pop1();

        if matches!(left_type, JavaType::Float | JavaType::Double)
            || matches!(right_type, JavaType::Float | JavaType::Double)
            || matches!(left_type, JavaType::Object(name) if matches!(name.as_str(), "java/lang/Float" | "java/lang/Double"))
            || matches!(right_type, JavaType::Object(name) if matches!(name.as_str(), "java/lang/Float" | "java/lang/Double"))
        {
            Some(JavaType::Object("java/lang/Double".to_string()))
        } else {
            Some(JavaType::Object("java/math/BigDecimal".to_string()))
        }
    }

    /// Compare two already-compiled boxed/runtime numeric operands, leaving a
    /// JVM comparison int (`-1`, `0`, `1`) on the stack.
    fn compile_runtime_number_compare(
        &mut self,
        left_type: &JavaType,
        right_type: &JavaType,
    ) -> Option<()> {
        let right_slot = self.reserve_local(right_type, "numeric comparison temporary")?;
        self.compile_store(right_type, right_slot);
        self.compile_box_primitive(left_type);
        self.compile_load(right_type, right_slot);
        self.compile_box_primitive(right_type);

        let method = self
            .stmt_compiler
            .expr_compiler()
            .constant_pool()
            .add_method_ref(NUMBERS_CLASS, "compare", COMPARE_NUMBER_DESCRIPTOR);
        self.stmt_compiler.expr_compiler().emit_byte(0xB8); // INVOKESTATIC
        self.stmt_compiler.expr_compiler().emit_short(method);
        // two references -> one int
        self.stmt_compiler.expr_compiler().stack_depth().pop1();
        Some(())
    }

    fn compile_boolean_equality_operands(
        &mut self,
        left_type: &JavaType,
        right_type: &JavaType,
    ) -> Option<()> {
        let right_slot = self.reserve_local(right_type, "boolean comparison temporary")?;
        self.compile_store(right_type, right_slot);
        self.unbox_boolean_top(left_type)?;
        self.compile_load(right_type, right_slot);
        self.unbox_boolean_top(right_type)
    }

    pub(super) fn unbox_boolean_top(&mut self, java_type: &JavaType) -> Option<()> {
        match java_type {
            JavaType::Boolean => Some(()),
            JavaType::Object(name) if name == "java/lang/Boolean" => {
                let method = self
                    .stmt_compiler
                    .expr_compiler()
                    .constant_pool()
                    .add_method_ref("java/lang/Boolean", "booleanValue", "()Z");
                self.stmt_compiler.expr_compiler().emit_byte(0xB6); // INVOKEVIRTUAL
                self.stmt_compiler.expr_compiler().emit_short(method);
                Some(())
            }
            _ => {
                self.reject_lowering(format!(
                    "Groovy boolean equality requires boolean operands, got {java_type:?}"
                ));
                None
            }
        }
    }

    /// Negate an already-compiled boxed runtime number.
    pub(super) fn compile_runtime_number_negate(&mut self, operand_type: &JavaType) -> JavaType {
        self.compile_box_primitive(operand_type);
        let method = self
            .stmt_compiler
            .expr_compiler()
            .constant_pool()
            .add_method_ref(NUMBERS_CLASS, "negate", UNARY_NUMBER_DESCRIPTOR);
        self.stmt_compiler.expr_compiler().emit_byte(0xB8); // INVOKESTATIC
        self.stmt_compiler.expr_compiler().emit_short(method);
        operand_type.clone()
    }

    /// Apply JVM binary numeric promotion to two already-compiled operands.
    ///
    /// The right operand is stored temporarily so the buried left operand can
    /// be widened without re-evaluating either expression. Both operands are
    /// then left on the stack with the exact type required by the arithmetic
    /// opcode selected from the returned type.
    pub(super) fn promote_arithmetic_operands(
        &mut self,
        left_type: &JavaType,
        right_type: &JavaType,
    ) -> Option<JavaType> {
        if !left_type.is_numeric() || !right_type.is_numeric() {
            self.reject_lowering(format!(
                "Groovy arithmetic requires numeric operands, got {left_type:?} and {right_type:?}"
            ));
            return None;
        }

        let promoted = if matches!(left_type, JavaType::Double)
            || matches!(right_type, JavaType::Double)
        {
            JavaType::Double
        } else if matches!(left_type, JavaType::Float) || matches!(right_type, JavaType::Float) {
            JavaType::Float
        } else if matches!(left_type, JavaType::Long) || matches!(right_type, JavaType::Long) {
            JavaType::Long
        } else {
            JavaType::Int
        };

        let right_slot = self.reserve_local(right_type, "numeric promotion temporary")?;
        self.compile_store(right_type, right_slot);
        self.widen_numeric_top(left_type, &promoted)?;
        self.compile_load(right_type, right_slot);
        self.widen_numeric_top(right_type, &promoted)?;
        Some(promoted)
    }

    pub(super) fn promote_integral_operands(
        &mut self,
        left_type: &JavaType,
        right_type: &JavaType,
    ) -> Option<JavaType> {
        if !is_integral_type(left_type) || !is_integral_type(right_type) {
            self.reject_lowering(format!(
                "Groovy bitwise operators require integral operands, got {left_type:?} and {right_type:?}"
            ));
            return None;
        }

        let promoted =
            if matches!(left_type, JavaType::Long) || matches!(right_type, JavaType::Long) {
                JavaType::Long
            } else {
                JavaType::Int
            };
        let right_slot = self.reserve_local(right_type, "bitwise promotion temporary")?;
        self.compile_store(right_type, right_slot);
        self.widen_numeric_top(left_type, &promoted)?;
        self.compile_load(right_type, right_slot);
        self.widen_numeric_top(right_type, &promoted)?;
        Some(promoted)
    }

    pub(super) fn prepare_shift_operands(
        &mut self,
        left_type: &JavaType,
        right_type: &JavaType,
    ) -> Option<JavaType> {
        if !is_integral_type(left_type) || !is_integral_type(right_type) {
            self.reject_lowering(format!(
                "Groovy shifts require integral operands, got {left_type:?} and {right_type:?}"
            ));
            return None;
        }

        // Every JVM shift consumes an int distance, even when the shifted value
        // is a long. Groovy accepts an integral long distance, so narrow only
        // the already-evaluated right operand; the JVM applies its normal mask.
        if matches!(right_type, JavaType::Long) {
            self.stmt_compiler.expr_compiler().emit_byte(0x88); // L2I
            self.stmt_compiler.expr_compiler().stack_depth().pop1();
        }

        Some(if matches!(left_type, JavaType::Long) {
            JavaType::Long
        } else {
            JavaType::Int
        })
    }

    pub(super) fn widen_numeric_top(&mut self, source: &JavaType, target: &JavaType) -> Option<()> {
        let source = if matches!(
            source,
            JavaType::Byte | JavaType::Short | JavaType::Char | JavaType::Int
        ) {
            &JavaType::Int
        } else {
            source
        };

        let conversion = match (source, target) {
            (source, target) if source == target => None,
            (JavaType::Int, JavaType::Long) => Some((0x85, 1)), // I2L
            (JavaType::Int, JavaType::Float) => Some((0x86, 0)), // I2F
            (JavaType::Int, JavaType::Double) => Some((0x87, 1)), // I2D
            (JavaType::Long, JavaType::Float) => Some((0x89, -1)), // L2F
            (JavaType::Long, JavaType::Double) => Some((0x8A, 0)), // L2D
            (JavaType::Float, JavaType::Double) => Some((0x8D, 1)), // F2D
            _ => {
                self.reject_lowering(format!(
                    "unsupported Groovy numeric promotion from {source:?} to {target:?}"
                ));
                return None;
            }
        };

        if let Some((opcode, stack_delta)) = conversion {
            let compiler = self.stmt_compiler.expr_compiler();
            compiler.emit_byte(opcode);
            match stack_delta {
                1 => compiler.stack_depth().push1(),
                -1 => compiler.stack_depth().pop1(),
                _ => {}
            }
        }
        Some(())
    }

    /// Compile arithmetic operations (+, -, *, /, %)
    pub(super) fn compile_arithmetic_op(
        &mut self,
        op: BinaryOp,
        left_type: JavaType,
    ) -> Option<JavaType> {
        match &left_type {
            JavaType::Long => {
                self.stmt_compiler.expr_compiler().stack_depth().pop2();
                self.stmt_compiler.expr_compiler().stack_depth().pop2();
                self.stmt_compiler.expr_compiler().stack_depth().push2();

                match op {
                    BinaryOp::Add => self.stmt_compiler.expr_compiler().emit_byte(0x61), // LADD
                    BinaryOp::Sub => self.stmt_compiler.expr_compiler().emit_byte(0x65), // LSUB
                    BinaryOp::Mul => self.stmt_compiler.expr_compiler().emit_byte(0x69), // LMUL
                    BinaryOp::Div => self.stmt_compiler.expr_compiler().emit_byte(0x6D), // LDIV
                    BinaryOp::Mod => self.stmt_compiler.expr_compiler().emit_byte(0x71), // LREM
                    _ => {}
                }
            }
            JavaType::Float => {
                self.stmt_compiler.expr_compiler().stack_depth().pop1();

                match op {
                    BinaryOp::Add => self.stmt_compiler.expr_compiler().emit_byte(0x62), // FADD
                    BinaryOp::Sub => self.stmt_compiler.expr_compiler().emit_byte(0x66), // FSUB
                    BinaryOp::Mul => self.stmt_compiler.expr_compiler().emit_byte(0x6A), // FMUL
                    BinaryOp::Div => self.stmt_compiler.expr_compiler().emit_byte(0x6E), // FDIV
                    BinaryOp::Mod => self.stmt_compiler.expr_compiler().emit_byte(0x72), // FREM
                    _ => {}
                }
            }
            JavaType::Double => {
                self.stmt_compiler.expr_compiler().stack_depth().pop2();
                self.stmt_compiler.expr_compiler().stack_depth().pop2();
                self.stmt_compiler.expr_compiler().stack_depth().push2();

                match op {
                    BinaryOp::Add => self.stmt_compiler.expr_compiler().emit_byte(0x63), // DADD
                    BinaryOp::Sub => self.stmt_compiler.expr_compiler().emit_byte(0x67), // DSUB
                    BinaryOp::Mul => self.stmt_compiler.expr_compiler().emit_byte(0x6B), // DMUL
                    BinaryOp::Div => self.stmt_compiler.expr_compiler().emit_byte(0x6F), // DDIV
                    BinaryOp::Mod => self.stmt_compiler.expr_compiler().emit_byte(0x73), // DREM
                    _ => {}
                }
            }
            JavaType::Byte | JavaType::Short | JavaType::Int | JavaType::Char => {
                self.stmt_compiler.expr_compiler().stack_depth().pop1();

                match op {
                    BinaryOp::Add => self.stmt_compiler.expr_compiler().emit_byte(0x60), // IADD
                    BinaryOp::Sub => self.stmt_compiler.expr_compiler().emit_byte(0x64), // ISUB
                    BinaryOp::Mul => self.stmt_compiler.expr_compiler().emit_byte(0x68), // IMUL
                    BinaryOp::Div => self.stmt_compiler.expr_compiler().emit_byte(0x6C), // IDIV
                    BinaryOp::Mod => self.stmt_compiler.expr_compiler().emit_byte(0x70), // IREM
                    _ => {}
                }
            }
            _ => {
                self.reject_lowering(format!(
                    "Groovy arithmetic cannot use JVM opcode with operand type {left_type:?}"
                ));
                return None;
            }
        }

        Some(left_type)
    }

    /// Compile bitwise operations (&, |, ^, <<, >>, >>>)
    pub(super) fn compile_bitwise_op(
        &mut self,
        op: BinaryOp,
        left_type: JavaType,
    ) -> Option<JavaType> {
        let is_shift = matches!(
            op,
            BinaryOp::LeftShift | BinaryOp::RightShift | BinaryOp::UnsignedRightShift
        );
        match &left_type {
            JavaType::Long => {
                if is_shift {
                    // [long, int] -> [long]
                    self.stmt_compiler.expr_compiler().stack_depth().pop1();
                } else {
                    // [long, long] -> [long]
                    self.stmt_compiler.expr_compiler().stack_depth().pop2();
                }

                match op {
                    BinaryOp::BitwiseAnd => self.stmt_compiler.expr_compiler().emit_byte(0x7F), // LAND
                    BinaryOp::BitwiseOr => self.stmt_compiler.expr_compiler().emit_byte(0x81), // LOR
                    BinaryOp::BitwiseXor => self.stmt_compiler.expr_compiler().emit_byte(0x83), // LXOR
                    BinaryOp::LeftShift => self.stmt_compiler.expr_compiler().emit_byte(0x79), // LSHL
                    BinaryOp::RightShift => self.stmt_compiler.expr_compiler().emit_byte(0x7B), // LSHR
                    BinaryOp::UnsignedRightShift => {
                        self.stmt_compiler.expr_compiler().emit_byte(0x7D) // LUSHR
                    }
                    _ => {}
                }
            }
            JavaType::Int => {
                // [int, int] -> [int]
                self.stmt_compiler.expr_compiler().stack_depth().pop1();

                match op {
                    BinaryOp::BitwiseAnd => self.stmt_compiler.expr_compiler().emit_byte(0x7E), // IAND
                    BinaryOp::BitwiseOr => self.stmt_compiler.expr_compiler().emit_byte(0x80), // IOR
                    BinaryOp::BitwiseXor => self.stmt_compiler.expr_compiler().emit_byte(0x82), // IXOR
                    BinaryOp::LeftShift => self.stmt_compiler.expr_compiler().emit_byte(0x78), // ISHL
                    BinaryOp::RightShift => self.stmt_compiler.expr_compiler().emit_byte(0x7A), // ISHR
                    BinaryOp::UnsignedRightShift => {
                        self.stmt_compiler.expr_compiler().emit_byte(0x7C) // IUSHR
                    }
                    _ => {}
                }
            }
            _ => unreachable!("bitwise preparation returned a non-integral JVM type"),
        }

        Some(left_type)
    }

    /// Compile comparison operations (==, !=, <, <=, >, >=)
    /// Returns boolean (int 0 or 1)
    pub(super) fn compile_comparison_op(
        &mut self,
        op: BinaryOp,
        left: &Expression,
        right: &Expression,
    ) -> Option<JavaType> {
        // Compile left operand
        let left_type = self.compile_expression(left)?;

        // Compile right operand
        let right_type = self.compile_expression(right)?;

        let left_is_number = left_type.is_numeric() || Self::is_runtime_number_type(&left_type);
        let right_is_number = right_type.is_numeric() || Self::is_runtime_number_type(&right_type);
        let is_boolean = |java_type: &JavaType| {
            java_type == &JavaType::Boolean
                || matches!(java_type, JavaType::Object(name) if name == "java/lang/Boolean")
        };
        let boolean_equality = matches!(op, BinaryOp::Equal | BinaryOp::NotEqual)
            && is_boolean(&left_type)
            && is_boolean(&right_type);

        if !(boolean_equality || left_is_number && right_is_number) {
            self.reject_lowering(format!(
                "Groovy comparison requires numeric operands or two booleans for equality; reference value comparison is not yet supported (got {left_type:?} and {right_type:?})"
            ));
            return None;
        }

        // Allocate labels for conditional logic
        let true_label = self.stmt_compiler.new_label();
        let end_label = self.stmt_compiler.new_label();

        if Self::is_runtime_number_type(&left_type) || Self::is_runtime_number_type(&right_type) {
            self.compile_runtime_number_compare(&left_type, &right_type)?;
            match op {
                BinaryOp::Equal => self.stmt_compiler.emit_ifeq(true_label),
                BinaryOp::NotEqual => self.stmt_compiler.emit_ifne(true_label),
                BinaryOp::LessThan => self.stmt_compiler.emit_iflt(true_label),
                BinaryOp::GreaterThanOrEqual => self.stmt_compiler.emit_ifge(true_label),
                BinaryOp::GreaterThan => self.stmt_compiler.emit_ifgt(true_label),
                BinaryOp::LessThanOrEqual => self.stmt_compiler.emit_ifle(true_label),
                _ => unreachable!(),
            }
        } else if boolean_equality {
            self.compile_boolean_equality_operands(&left_type, &right_type)?;
            match op {
                BinaryOp::Equal => self.stmt_compiler.emit_if_icmpeq(true_label),
                BinaryOp::NotEqual => self.stmt_compiler.emit_if_icmpne(true_label),
                _ => unreachable!(),
            }
        } else {
            let promoted_type = self.promote_arithmetic_operands(&left_type, &right_type)?;

            // Emit comparison instruction based on the type shared by both
            // operands, never on only one side of a mixed-width expression.
            match promoted_type {
                JavaType::Long => {
                    // LCMP: pops 2 longs, pushes int (-1, 0, or 1)
                    self.stmt_compiler.expr_compiler().emit_byte(0x94); // LCMP
                    self.stmt_compiler.expr_compiler().stack_depth().pop2(); // right
                    self.stmt_compiler.expr_compiler().stack_depth().pop2(); // left
                    self.stmt_compiler.expr_compiler().stack_depth().push1(); // result

                    // Now compare result with 0 using if_icmp*
                    self.emit_comparison_branch(op, true_label);
                }
                JavaType::Float => {
                    // Use CMPG for `<`/`<=` and CMPL otherwise so every ordered
                    // comparison is false when either operand is NaN.
                    let opcode = if matches!(op, BinaryOp::LessThan | BinaryOp::LessThanOrEqual) {
                        0x96 // FCMPG
                    } else {
                        0x95 // FCMPL
                    };
                    self.stmt_compiler.expr_compiler().emit_byte(opcode);
                    self.stmt_compiler.expr_compiler().stack_depth().pop1(); // right
                    self.stmt_compiler.expr_compiler().stack_depth().pop1(); // left
                    self.stmt_compiler.expr_compiler().stack_depth().push1(); // result

                    self.emit_comparison_branch(op, true_label);
                }
                JavaType::Double => {
                    let opcode = if matches!(op, BinaryOp::LessThan | BinaryOp::LessThanOrEqual) {
                        0x98 // DCMPG
                    } else {
                        0x97 // DCMPL
                    };
                    self.stmt_compiler.expr_compiler().emit_byte(opcode);
                    self.stmt_compiler.expr_compiler().stack_depth().pop2(); // right
                    self.stmt_compiler.expr_compiler().stack_depth().pop2(); // left
                    self.stmt_compiler.expr_compiler().stack_depth().push1(); // result

                    self.emit_comparison_branch(op, true_label);
                }
                JavaType::Int => {
                    // Integer comparison uses direct if_icmp* instructions.
                    match op {
                        BinaryOp::Equal => self.stmt_compiler.emit_if_icmpeq(true_label),
                        BinaryOp::NotEqual => self.stmt_compiler.emit_if_icmpne(true_label),
                        BinaryOp::LessThan => self.stmt_compiler.emit_if_icmplt(true_label),
                        BinaryOp::LessThanOrEqual => self.stmt_compiler.emit_if_icmple(true_label),
                        BinaryOp::GreaterThan => self.stmt_compiler.emit_if_icmpgt(true_label),
                        BinaryOp::GreaterThanOrEqual => {
                            self.stmt_compiler.emit_if_icmpge(true_label)
                        }
                        _ => {}
                    }
                }
                _ => unreachable!("numeric promotion returned a non-numeric JVM type"),
            }
        }

        // False path: push 0
        self.stmt_compiler.expr_compiler().emit_byte(0x03); // ICONST_0
        self.stmt_compiler.expr_compiler().stack_depth().push1();
        self.stmt_compiler.emit_goto(end_label);

        // True path: push 1
        self.stmt_compiler.bind_label(true_label);
        self.stmt_compiler.expr_compiler().emit_byte(0x04); // ICONST_1
        self.stmt_compiler.expr_compiler().stack_depth().push1();

        // End
        self.stmt_compiler.bind_label(end_label);

        // The result is a boolean (represented as int 0/1 on the operand stack).
        // Reporting `Boolean` — not `Int` — lets return/boxing pick
        // `Boolean.valueOf`, so a comparison result displays as `true`/`false`
        // rather than `1`/`0`. The emitted bytecode and stack value are
        // unchanged, so conditional branches are unaffected.
        Some(JavaType::Boolean)
    }

    /// Helper: emit comparison branch based on comparison result (for long/float/double)
    pub(super) fn emit_comparison_branch(&mut self, op: BinaryOp, true_label: Label) {
        match op {
            BinaryOp::Equal => self.stmt_compiler.emit_ifeq(true_label),
            BinaryOp::NotEqual => self.stmt_compiler.emit_ifne(true_label),
            BinaryOp::LessThan => self.stmt_compiler.emit_iflt(true_label),
            BinaryOp::LessThanOrEqual => self.stmt_compiler.emit_ifle(true_label),
            BinaryOp::GreaterThan => self.stmt_compiler.emit_ifgt(true_label),
            BinaryOp::GreaterThanOrEqual => self.stmt_compiler.emit_ifge(true_label),
            _ => {}
        }
    }

    /// Coerce the value on top of the operand stack to a primitive truth `int`
    /// so it can drive `ifeq`/`ifne`.
    ///
    /// All truth decisions go through the interpreter's canonical Groovy-truth
    /// implementation, keeping control expressions and collection DGMs aligned.
    pub(super) fn coerce_to_truth_int(&mut self, ty: &JavaType) -> Option<()> {
        if *ty == JavaType::Void {
            self.reject_lowering("void expression cannot be coerced to Groovy truth");
            return None;
        }

        self.compile_box_primitive(ty);
        let method = self
            .stmt_compiler
            .expr_compiler()
            .constant_pool()
            .add_method_ref(TRUTH_CLASS, TRUTH_METHOD, TRUTH_DESCRIPTOR);
        self.stmt_compiler.expr_compiler().emit_byte(0xB8); // invokestatic
        self.stmt_compiler.expr_compiler().emit_short(method);
        // Object argument -> primitive int result: no net stack-depth change.
        Some(())
    }

    /// Widen the numeric value on top of the stack to `double` (for `**`).
    fn widen_to_double(&mut self, ty: &JavaType) -> Option<()> {
        // (opcode, extra stack slots added by the widening)
        let (opcode, extra_slot): (Option<u8>, bool) = match ty {
            JavaType::Int | JavaType::Short | JavaType::Byte | JavaType::Char => (Some(0x87), true), // i2d
            JavaType::Long => (Some(0x8A), false), // l2d (2→2 slots)
            JavaType::Float => (Some(0x8D), true), // f2d
            JavaType::Double => (None, false),
            _ => {
                self.reject_lowering(format!(
                    "Groovy power requires primitive numeric operands, got {ty:?}"
                ));
                return None;
            }
        };
        if let Some(op) = opcode {
            self.stmt_compiler.expr_compiler().emit_byte(op);
            if extra_slot {
                self.stmt_compiler.expr_compiler().stack_depth().push1();
            }
        }
        Some(())
    }

    /// Compile the Groovy `**` power operator via `Math.pow`.
    ///
    /// Groovy's runtime rule is type-dependent: `int ** nonNegInt` yields an
    /// `Integer` (a `BigInteger` on overflow — not modelled), while a negative or
    /// fractional exponent yields a `Double`/`BigDecimal`. We can only see the
    /// exponent statically, so we produce an integer result when the base is an
    /// integral type AND the exponent is a non-negative integer literal; every
    /// other case stays `Double`. Overflow past `int`/`long` is a known gap.
    pub(super) fn compile_power_op(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Option<JavaType> {
        let left_type = self.compile_expression(left)?;
        let base_long = matches!(left_type, JavaType::Long);
        let base_integral = matches!(
            left_type,
            JavaType::Int | JavaType::Short | JavaType::Byte | JavaType::Char | JavaType::Long
        );
        self.widen_to_double(&left_type)?;

        let right_type = self.compile_expression(right)?;
        self.widen_to_double(&right_type)?;

        // invokestatic Math.pow(DD)D
        let m = self
            .stmt_compiler
            .expr_compiler()
            .constant_pool()
            .add_method_ref("java/lang/Math", "pow", "(DD)D");
        self.stmt_compiler.expr_compiler().emit_byte(0xB8); // invokestatic
        self.stmt_compiler.expr_compiler().emit_short(m);
        // pops two doubles (4 slots), pushes one double (2 slots)
        self.stmt_compiler.expr_compiler().stack_depth().pop2();
        self.stmt_compiler.expr_compiler().stack_depth().pop2();
        self.stmt_compiler.expr_compiler().stack_depth().push2();

        let nonneg_int_exp = matches!(right, Expression::IntLiteral(n) if *n >= 0);
        if base_integral && nonneg_int_exp {
            if base_long {
                self.stmt_compiler.expr_compiler().emit_byte(0x8F); // d2l
                Some(JavaType::Long)
            } else {
                self.stmt_compiler.expr_compiler().emit_byte(0x8E); // d2i
                self.stmt_compiler.expr_compiler().stack_depth().pop2();
                self.stmt_compiler.expr_compiler().stack_depth().push1();
                Some(JavaType::Int)
            }
        } else {
            Some(JavaType::Double)
        }
    }

    /// Compile logical operators with short-circuit semantics.
    ///
    /// Both operands are coerced to a primitive truth `int` (see
    /// [`Self::coerce_to_truth_int`]); the result is reported as `Boolean` so it
    /// boxes via `Boolean.valueOf` and displays as `true`/`false`.
    pub(super) fn compile_logical_op(
        &mut self,
        op: BinaryOp,
        left: &Expression,
        right: &Expression,
    ) -> Option<JavaType> {
        // `&&` short-circuits when the left is false (IFEQ); `||` when it is true
        // (IFNE). Otherwise the shape is identical.
        let short_circuit_op = match op {
            BinaryOp::LogicalAnd => 0x99, // IFEQ
            BinaryOp::LogicalOr => 0x9A,  // IFNE
            _ => return None,
        };

        // Compile + coerce the left operand.
        let left_type = self.compile_expression(left)?;
        self.coerce_to_truth_int(&left_type)?;

        // DUP the truth value so the short-circuit path can reuse it as result.
        self.stmt_compiler.expr_compiler().emit_byte(0x59); // DUP
        self.stmt_compiler.expr_compiler().stack_depth().push1();

        // Short-circuit jump (consumes the duplicate).
        let end_label = self.stmt_compiler.new_label();
        if short_circuit_op == 0x99 {
            self.stmt_compiler.emit_ifeq(end_label);
        } else {
            self.stmt_compiler.emit_ifne(end_label);
        }

        // Not short-circuited: drop the duplicate and evaluate + coerce the right.
        self.stmt_compiler.expr_compiler().emit_byte(0x57); // POP
        self.stmt_compiler.expr_compiler().stack_depth().pop1();
        let right_type = self.compile_expression(right)?;
        self.coerce_to_truth_int(&right_type)?;

        self.stmt_compiler.bind_label(end_label);
        Some(JavaType::Boolean)
    }

    /// Compile string concatenation using StringBuilder pattern
    /// Compile `left + right` as a `String` concatenation, where `left` has
    /// already been compiled and its value is on top of the operand stack with
    /// static type `left_type`.
    ///
    /// Emits the javac-style idiom
    /// `new StringBuilder().append(left).append(right).toString()`, but slots
    /// the fresh `StringBuilder` *under* the already-computed `left` value with
    /// `dup_x1` so `left` is never recompiled. Recompiling `left` was the cause
    /// of a silent-wrong-answer bug: for a chain like `"a" + "b" + "c"` the
    /// inner concat was evaluated twice, orphaning values on the operand stack
    /// and yielding `"c"` instead of `"abc"`.
    pub(super) fn compile_string_concat(
        &mut self,
        left_type: &JavaType,
        right: &Expression,
    ) -> Option<JavaType> {
        if *left_type == JavaType::Void {
            self.reject_lowering("void expression cannot be concatenated as a String");
            return None;
        }

        let ec = self.stmt_compiler.expr_compiler();

        // stack: [left]
        let sb_class = ec.constant_pool().add_class("java/lang/StringBuilder");
        ec.emit_byte(0xBB); // NEW StringBuilder  -> [left, SB]
        ec.emit_short(sb_class);
        ec.stack_depth().push1();

        ec.emit_byte(0x5A); // DUP_X1  -> [SB, left, SB]
        ec.stack_depth().push1();

        let init_ref =
            ec.constant_pool()
                .add_method_ref("java/lang/StringBuilder", "<init>", "()V");
        ec.emit_byte(0xB7); // INVOKESPECIAL <init>()V  -> [SB, left]
        ec.emit_short(init_ref);
        ec.stack_depth().pop1();

        // append(left): receiver SB sits below the left value  -> [SB']
        let append_left = ec.constant_pool().add_method_ref(
            "java/lang/StringBuilder",
            "append",
            sb_append_descriptor(left_type),
        );
        ec.emit_byte(0xB6); // INVOKEVIRTUAL append
        ec.emit_short(append_left);
        if matches!(left_type, JavaType::Long | JavaType::Double) {
            ec.stack_depth().pop2();
        } else {
            ec.stack_depth().pop1();
        }

        // append(right): compile right, then dispatch on its static type.
        let right_type = self.compile_expression(right)?;
        if right_type == JavaType::Void {
            self.reject_lowering("void expression cannot be concatenated as a String");
            return None;
        }
        let ec = self.stmt_compiler.expr_compiler();
        let append_right = ec.constant_pool().add_method_ref(
            "java/lang/StringBuilder",
            "append",
            sb_append_descriptor(&right_type),
        );
        ec.emit_byte(0xB6); // INVOKEVIRTUAL append
        ec.emit_short(append_right);
        // Pop the right argument (two slots for long/double, else one). The
        // receiver StringBuilder is consumed and re-pushed as the result.
        if matches!(right_type, JavaType::Long | JavaType::Double) {
            ec.stack_depth().pop2();
        } else {
            ec.stack_depth().pop1();
        }

        let to_string = ec.constant_pool().add_method_ref(
            "java/lang/StringBuilder",
            "toString",
            "()Ljava/lang/String;",
        );
        ec.emit_byte(0xB6); // INVOKEVIRTUAL toString  -> [String] (net stack: 0)
        ec.emit_short(to_string);

        Some(JavaType::Object("java/lang/String".to_string()))
    }
}

fn is_integral_type(java_type: &JavaType) -> bool {
    matches!(
        java_type,
        JavaType::Byte | JavaType::Short | JavaType::Int | JavaType::Long | JavaType::Char
    )
}

/// The `StringBuilder.append` overload descriptor for a given operand type.
/// The interpreter's native `append` stringifies any value, but the descriptor
/// still drives operand-stack slot accounting (long/double take two slots) and
/// char/boolean rendering, so it must match the value's category.
fn sb_append_descriptor(ty: &JavaType) -> &'static str {
    match ty {
        JavaType::Object(s) if s == "java/lang/String" => {
            "(Ljava/lang/String;)Ljava/lang/StringBuilder;"
        }
        JavaType::Boolean => "(Z)Ljava/lang/StringBuilder;",
        JavaType::Char => "(C)Ljava/lang/StringBuilder;",
        JavaType::Byte | JavaType::Short | JavaType::Int => "(I)Ljava/lang/StringBuilder;",
        JavaType::Long => "(J)Ljava/lang/StringBuilder;",
        JavaType::Float => "(F)Ljava/lang/StringBuilder;",
        JavaType::Double => "(D)Ljava/lang/StringBuilder;",
        // References (and arrays) go through append(Object).
        _ => "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
    }
}
