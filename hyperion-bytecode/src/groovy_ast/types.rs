// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type utilities for Groovy AST compilation
//!
//! Boxing, unboxing, type-specific loads/stores, and return handling.

use super::statements::can_widen_numeric;
use super::GroovyAstCompiler;
use crate::java_compiler::JavaType;

pub(super) fn parse_groovy_type_annotation(annotation: &str) -> Result<JavaType, String> {
    let mut source = annotation.trim();
    let mut dimensions = 0usize;
    while let Some(element) = source.strip_suffix("[]") {
        dimensions += 1;
        source = element.trim_end();
    }
    let dimensions = u8::try_from(dimensions)
        .map_err(|_| format!("Groovy type `{annotation}` exceeds the JVM array-dimension limit"))?;

    let base = if let Some(generic_start) = source.find('<') {
        let mut depth = 0usize;
        let mut closed_at = None;
        for (index, character) in source
            .char_indices()
            .skip_while(|(index, _)| *index < generic_start)
        {
            match character {
                '<' => depth += 1,
                '>' => {
                    depth = depth.checked_sub(1).ok_or_else(|| {
                        format!("malformed Groovy type annotation `{annotation}`")
                    })?;
                    if depth == 0 {
                        closed_at = Some(index + character.len_utf8());
                        break;
                    }
                }
                _ => {}
            }
        }
        if depth != 0 || closed_at != Some(source.len()) {
            return Err(format!("malformed Groovy type annotation `{annotation}`"));
        }
        source[..generic_start].trim()
    } else {
        source
    };

    if base.is_empty()
        || base
            .bytes()
            .any(|byte| matches!(byte, b'[' | b']' | b';' | b'<' | b'>'))
    {
        return Err(format!("malformed Groovy type annotation `{annotation}`"));
    }

    let element_type = match base {
        "byte" => JavaType::Byte,
        "short" => JavaType::Short,
        "int" => JavaType::Int,
        "boolean" => JavaType::Boolean,
        "char" => JavaType::Char,
        "long" => JavaType::Long,
        "float" => JavaType::Float,
        "double" => JavaType::Double,
        "void" => JavaType::Void,
        "String" => JavaType::Object("java/lang/String".to_string()),
        "Object" => JavaType::Object("java/lang/Object".to_string()),
        _ => JavaType::Object(base.replace('.', "/")),
    };

    if dimensions == 0 {
        Ok(element_type)
    } else if element_type == JavaType::Void {
        Err(format!(
            "Groovy array type `{annotation}` cannot have void elements"
        ))
    } else {
        Ok(JavaType::Array {
            element_type: Box::new(element_type),
            dimensions,
        })
    }
}

impl GroovyAstCompiler {
    /// Reserve local-variable slots using the JVM width of the stored value.
    pub(super) fn reserve_local(&mut self, java_type: &JavaType, purpose: &str) -> Option<u16> {
        let width = u16::from(java_type.stack_width().max(1));
        let slot = self.next_local;
        let Some(next_local) = self.next_local.checked_add(width) else {
            self.reject_lowering(format!("{purpose} exceeds the JVM local-variable limit"));
            return None;
        };
        self.next_local = next_local;
        Some(slot)
    }

    /// Duplicate the top stack value while preserving its JVM category.
    pub(super) fn duplicate_top(&mut self, java_type: &JavaType) {
        if java_type.is_category2() {
            self.stmt_compiler.expr_compiler().emit_byte(0x5C); // DUP2
            self.stmt_compiler.expr_compiler().stack_depth().push2();
        } else if *java_type != JavaType::Void {
            self.stmt_compiler.expr_compiler().emit_byte(0x59); // DUP
            self.stmt_compiler.expr_compiler().stack_depth().push1();
        }
    }

    /// Store a value to a local slot based on its type
    pub(super) fn compile_store(&mut self, java_type: &JavaType, slot: u16) {
        match java_type {
            JavaType::Int
            | JavaType::Byte
            | JavaType::Short
            | JavaType::Char
            | JavaType::Boolean => {
                self.stmt_compiler.expr_compiler().compile_istore(slot);
            }
            JavaType::Long => {
                self.stmt_compiler.expr_compiler().compile_lstore(slot);
            }
            JavaType::Float => {
                self.stmt_compiler.expr_compiler().compile_fstore(slot);
            }
            JavaType::Double => {
                self.stmt_compiler.expr_compiler().compile_dstore(slot);
            }
            JavaType::Object(_) | JavaType::Array { .. } => {
                self.stmt_compiler.expr_compiler().compile_astore(slot);
            }
            JavaType::Void => {
                // Nothing to store for void
            }
        }
    }

    /// Load a value from a local slot based on its type.
    pub(super) fn compile_load(&mut self, java_type: &JavaType, slot: u16) {
        match java_type {
            JavaType::Int
            | JavaType::Byte
            | JavaType::Short
            | JavaType::Char
            | JavaType::Boolean => {
                self.stmt_compiler.expr_compiler().compile_iload(slot);
            }
            JavaType::Long => {
                self.stmt_compiler.expr_compiler().compile_lload(slot);
            }
            JavaType::Float => {
                self.stmt_compiler.expr_compiler().compile_fload(slot);
            }
            JavaType::Double => {
                self.stmt_compiler.expr_compiler().compile_dload(slot);
            }
            JavaType::Object(name) => {
                self.stmt_compiler.expr_compiler().compile_aload(slot, name);
            }
            JavaType::Array { .. } => {
                self.stmt_compiler
                    .expr_compiler()
                    .compile_aload(slot, &java_type.descriptor());
            }
            JavaType::Void => {}
        }
    }

    /// Load an already-adapted method result and emit its descriptor-compatible return.
    pub(super) fn compile_load_and_return(&mut self, java_type: &JavaType, slot: u16) {
        self.compile_load(java_type, slot);
        self.compile_return_top(java_type);
    }

    pub(super) fn compile_return_top(&mut self, java_type: &JavaType) {
        match java_type {
            JavaType::Int
            | JavaType::Byte
            | JavaType::Short
            | JavaType::Char
            | JavaType::Boolean => {
                self.stmt_compiler.expr_compiler().emit_byte(0xAC); // IRETURN
                self.stmt_compiler.expr_compiler().stack_depth().pop1();
            }
            JavaType::Long => {
                self.stmt_compiler.expr_compiler().emit_byte(0xAD); // LRETURN
                self.stmt_compiler.expr_compiler().stack_depth().pop2();
            }
            JavaType::Float => {
                self.stmt_compiler.expr_compiler().emit_byte(0xAE); // FRETURN
                self.stmt_compiler.expr_compiler().stack_depth().pop1();
            }
            JavaType::Double => {
                self.stmt_compiler.expr_compiler().emit_byte(0xAF); // DRETURN
                self.stmt_compiler.expr_compiler().stack_depth().pop2();
            }
            JavaType::Object(_) | JavaType::Array { .. } => {
                self.stmt_compiler.expr_compiler().emit_byte(0xB0); // ARETURN
                self.stmt_compiler.expr_compiler().stack_depth().pop1();
            }
            JavaType::Void => self.stmt_compiler.compile_return_void(),
        }
    }

    pub(super) fn adapt_method_return(
        &mut self,
        actual_type: &JavaType,
        value_is_null: bool,
    ) -> Option<JavaType> {
        let expected_type = self.current_method_return_type.clone();
        if expected_type == *actual_type {
            return Some(expected_type);
        }
        if expected_type.is_numeric() && actual_type.is_numeric() {
            if can_widen_numeric(actual_type, &expected_type) {
                self.widen_numeric_top(actual_type, &expected_type)?;
                return Some(expected_type);
            }
        } else if expected_type == JavaType::Boolean
            && matches!(actual_type, JavaType::Object(name) if name == "java/lang/Boolean")
        {
            self.unbox_boolean_top(actual_type)?;
            return Some(expected_type);
        } else if value_is_null
            && matches!(expected_type, JavaType::Object(_) | JavaType::Array { .. })
        {
            return Some(expected_type);
        } else if matches!(&expected_type, JavaType::Object(name) if matches!(name.as_str(), "Object" | "java/lang/Object" | "java.lang.Object"))
        {
            self.compile_box_primitive(actual_type);
            return Some(expected_type);
        }

        self.reject_lowering(format!(
            "Groovy method cannot adapt return value {actual_type:?} to {expected_type:?}"
        ));
        None
    }

    /// Box a primitive value on the stack to its wrapper type
    /// Returns the boxed Object type
    pub(super) fn compile_box_primitive(&mut self, java_type: &JavaType) -> JavaType {
        let (wrapper_class, descriptor) = match java_type {
            JavaType::Int => ("java/lang/Integer", "(I)Ljava/lang/Integer;"),
            JavaType::Long => ("java/lang/Long", "(J)Ljava/lang/Long;"),
            JavaType::Float => ("java/lang/Float", "(F)Ljava/lang/Float;"),
            JavaType::Double => ("java/lang/Double", "(D)Ljava/lang/Double;"),
            JavaType::Boolean => ("java/lang/Boolean", "(Z)Ljava/lang/Boolean;"),
            JavaType::Byte => ("java/lang/Byte", "(B)Ljava/lang/Byte;"),
            JavaType::Short => ("java/lang/Short", "(S)Ljava/lang/Short;"),
            JavaType::Char => ("java/lang/Character", "(C)Ljava/lang/Character;"),
            // Non-primitives don't need boxing
            _ => return java_type.clone(),
        };

        let valueof_ref = self
            .stmt_compiler
            .expr_compiler()
            .constant_pool()
            .add_method_ref(wrapper_class, "valueOf", descriptor);
        self.stmt_compiler.expr_compiler().emit_byte(0xB8); // INVOKESTATIC
        self.stmt_compiler.expr_compiler().emit_short(valueof_ref);
        if java_type.is_category2() {
            // long/double (two slots) -> reference (one slot)
            self.stmt_compiler.expr_compiler().stack_depth().pop1();
        }

        JavaType::Object(wrapper_class.to_string())
    }
}
