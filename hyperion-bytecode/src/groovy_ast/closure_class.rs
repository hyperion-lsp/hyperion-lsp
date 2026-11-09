// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Construction and serialization of generated Groovy closure classes.

use super::{
    closure_expression::{box_closure_value, compile_closure_expr},
    ClosureCompilationContext,
};
use crate::groovy_producer::{
    checked_bootstrap_attribute_length, checked_method_code_length, checked_u16_count,
};
use crate::java_compiler::JavaType;
use hyperion_groovy::Statement;
use tracing::trace;

/// Build constructor bytecode for a closure class
fn build_constructor_bytecode(
    cp: &mut crate::constant_pool::ConstantPoolBuilder,
    super_init_ref: u16,
    closure_ctx: &ClosureCompilationContext,
) -> Result<(Vec<u8>, u16), String> {
    let mut bytecode = vec![
        0x2A, // aload_0 (this)
        0x2B, // aload_1 (owner)
        0x2C, // aload_2 (thisObject)
        0xB7, // invokespecial Closure.<init>(Object, Object)V
    ];
    bytecode.extend_from_slice(&super_init_ref.to_be_bytes());

    // Store captured variables in fields
    // Parameters start at slot 3 (0=this, 1=owner, 2=thisObject)
    let mut next_parameter_slot = 3_u16;
    for capture in &closure_ctx.captures {
        let width = u16::from(capture.java_type.stack_width());
        if width == 0 {
            return Err(format!(
                "generated closure cannot capture void variable {}",
                capture.name
            ));
        }
        let param_slot = u8::try_from(next_parameter_slot).map_err(|_| {
            format!(
                "generated closure constructor parameter {} exceeds the JVM parameter-slot limit",
                capture.name
            )
        })?;
        next_parameter_slot = next_parameter_slot
            .checked_add(width)
            .filter(|slots| *slots <= 255)
            .ok_or_else(|| {
                "generated closure constructor exceeds the JVM 255-slot parameter limit".to_string()
            })?;

        // aload_0 (this)
        bytecode.push(0x2A);

        // Load parameter based on type
        match &capture.java_type {
            JavaType::Int
            | JavaType::Byte
            | JavaType::Short
            | JavaType::Char
            | JavaType::Boolean => {
                if param_slot <= 3 {
                    bytecode.push(0x1A + param_slot); // iload_0 to iload_3
                } else {
                    bytecode.push(0x15); // iload
                    bytecode.push(param_slot);
                }
            }
            JavaType::Long => {
                bytecode.push(0x16); // lload
                bytecode.push(param_slot);
            }
            JavaType::Float => {
                bytecode.push(0x17); // fload
                bytecode.push(param_slot);
            }
            JavaType::Double => {
                bytecode.push(0x18); // dload
                bytecode.push(param_slot);
            }
            JavaType::Object(_) | JavaType::Array { .. } => {
                if param_slot <= 3 {
                    bytecode.push(0x2A + param_slot); // aload_0 to aload_3
                } else {
                    bytecode.push(0x19); // aload
                    bytecode.push(param_slot);
                }
            }
            JavaType::Void => {
                return Err(format!(
                    "generated closure cannot capture void variable {}",
                    capture.name
                ))
            }
        }

        // putfield this.capturedN
        let field_ref_idx = cp.add_field_ref(
            &closure_ctx.class_name,
            &capture.name,
            &capture.java_type.descriptor(),
        );
        bytecode.push(0xB5); // putfield
        bytecode.extend_from_slice(&field_ref_idx.to_be_bytes());
    }

    // return
    bytecode.push(0xB1);

    Ok((bytecode, next_parameter_slot))
}

/// Build doCall bytecode for a closure class
/// Returns (bytecode, max_locals)
///
/// doCall(Object arg0, Object arg1, ...) - receives args directly, not as array
/// Local slots: 0=this, 1=arg0, 2=arg1, ...
fn build_docall_bytecode(
    cp: &mut crate::constant_pool::ConstantPoolBuilder,
    parameters: &[String],
    body: &[Statement],
    closure_ctx: &ClosureCompilationContext,
    closure_classes: &mut std::collections::HashMap<String, Vec<u8>>,
) -> Result<(Vec<u8>, u16), String> {
    if parameters.len() > 254 {
        return Err(format!(
            "generated closure has {} parameters; JVM instance methods support at most 254 single-slot parameters",
            parameters.len()
        ));
    }
    let mut bytecode = Vec::new();

    // Parameters are received directly in local slots
    // slot 0 = this, slot 1 = first param, slot 2 = second param, etc.
    let mut param_slots: std::collections::HashMap<String, u16> = std::collections::HashMap::new();
    for (i, param_name) in parameters.iter().enumerate() {
        // Parameters start at slot 1 (slot 0 is this)
        let slot = (i + 1) as u16;
        param_slots.insert(param_name.clone(), slot);
    }

    // max_locals = this + number of parameters
    let max_locals =
        crate::groovy_producer::checked_u16_count(1 + parameters.len(), "closure local slots")
            .map_err(|error| error.to_string())?;

    // Compile body statements
    trace!("doCall body has {} statements", body.len());
    let mut has_explicit_return = false;
    for (i, stmt) in body.iter().enumerate() {
        trace!("doCall statement {}: {:?}", i, stmt);
        match stmt {
            Statement::Return(Some(expr)) => {
                has_explicit_return = true;
                let return_type = compile_closure_expr(
                    &mut bytecode,
                    cp,
                    expr,
                    &param_slots,
                    &closure_ctx.captures,
                    &closure_ctx.class_name,
                    closure_classes,
                )?;
                box_closure_value(&mut bytecode, cp, &return_type)?;
                bytecode.push(0xB0); // areturn
            }
            Statement::Return(None) => {
                has_explicit_return = true;
                bytecode.push(0x01); // aconst_null
                bytecode.push(0xB0); // areturn
            }
            Statement::Expression(expr) => {
                let expression_type = compile_closure_expr(
                    &mut bytecode,
                    cp,
                    expr,
                    &param_slots,
                    &closure_ctx.captures,
                    &closure_ctx.class_name,
                    closure_classes,
                )?;
                let is_last = i == body.len() - 1;
                if is_last && !has_explicit_return {
                    // Last expression in closure body - implicit return
                    box_closure_value(&mut bytecode, cp, &expression_type)?;
                    bytecode.push(0xB0); // areturn
                    has_explicit_return = true;
                } else {
                    bytecode.push(if expression_type.is_category2() {
                        0x58 // pop2
                    } else {
                        0x57 // pop
                    });
                }
            }
            other => {
                return Err(format!(
                    "unsupported statement in generated closure: {other:?}"
                ))
            }
        }
    }

    // If body was empty or ended without an explicit return, return null.
    if !has_explicit_return {
        bytecode.push(0x01); // aconst_null
        bytecode.push(0xB0); // areturn
    }

    trace!("doCall bytecode = {:02X?}", bytecode);
    Ok((bytecode, max_locals))
}

/// Generate bytecode for a closure class
///
/// PHASE 1.7.4: Closure class generation
///
/// Architecture:
/// - Extends groovy.lang.Closure
/// - Fields for captured variables
/// - Constructor: super(owner, thisObject) + field assignments
/// - doCall(Object... args): executes closure body
///
/// Class structure:
/// ```java
/// class ParentClass$_closure_N extends groovy.lang.Closure {
///     private final Type1 captured1;
///     private final Type2 captured2;
///
///     public ParentClass$_closure_N(Object owner, Object thisObject, Type1 captured1, Type2 captured2) {
///         super(owner, thisObject);
///         this.captured1 = captured1;
///         this.captured2 = captured2;
///     }
///
///     public Object doCall(Object... args) {
///         // closure body with access to parameters and captured vars
///     }
/// }
/// ```
pub(super) fn generate_closure_class_bytes(
    closure_ctx: &ClosureCompilationContext,
    parameters: &[String],
    body: &[Statement],
    closure_classes: &mut std::collections::HashMap<String, Vec<u8>>,
) -> Result<Vec<u8>, String> {
    use crate::constant_pool::ConstantPoolBuilder;

    let mut cp = ConstantPoolBuilder::new();
    let mut class_bytes = Vec::new();

    // Build constant pool entries
    let class_name_idx = cp.add_class(&closure_ctx.class_name);
    let super_class_idx = cp.add_class("groovy/lang/Closure");

    // Constructor
    let init_name_idx = cp.add_utf8("<init>");

    // Build constructor descriptor: (Ljava/lang/Object;Ljava/lang/Object;...captures)V
    let mut init_desc = "(Ljava/lang/Object;Ljava/lang/Object;".to_string();
    for capture in &closure_ctx.captures {
        init_desc.push_str(&capture.java_type.descriptor());
    }
    init_desc.push_str(")V");
    let init_desc_idx = cp.add_utf8(&init_desc);

    // Super constructor call
    let super_init_ref = cp.add_method_ref(
        "groovy/lang/Closure",
        "<init>",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
    );

    // doCall method - signature matches the number of closure parameters
    // For 1 parameter: (Ljava/lang/Object;)Ljava/lang/Object;
    // For 0 parameters: ()Ljava/lang/Object;
    // For N parameters: (Ljava/lang/Object;...N times...)Ljava/lang/Object;
    let docall_name_idx = cp.add_utf8("doCall");
    let mut docall_desc = String::from("(");
    for _ in 0..parameters.len() {
        docall_desc.push_str("Ljava/lang/Object;");
    }
    docall_desc.push_str(")Ljava/lang/Object;");
    let docall_desc_idx = cp.add_utf8(&docall_desc);

    // Code attribute
    let code_attr_idx = cp.add_utf8("Code");

    // Add standard Groovy Closure fields (delegate, owner, resolveStrategy)
    // These must match the layout in closure_runtime.rs:field_offset
    // Field 0: delegate (Object)
    // Field 1: owner (Object)
    // Field 2: resolveStrategy (int)
    // Field 3+: captured variables
    let delegate_name_idx = cp.add_utf8("delegate");
    let delegate_desc_idx = cp.add_utf8("Ljava/lang/Object;");
    let owner_name_idx = cp.add_utf8("owner");
    let owner_desc_idx = cp.add_utf8("Ljava/lang/Object;");
    let resolve_strategy_name_idx = cp.add_utf8("resolveStrategy");
    let resolve_strategy_desc_idx = cp.add_utf8("I");

    // Add field entries for captures (after standard fields)
    let mut field_names = Vec::new();
    let mut field_descs = Vec::new();
    for capture in &closure_ctx.captures {
        let field_name_idx = cp.add_utf8(&capture.name);
        let field_desc_idx = cp.add_utf8(&capture.java_type.descriptor());
        field_names.push(field_name_idx);
        field_descs.push(field_desc_idx);
    }

    // === Build all bytecode FIRST (this populates the constant pool) ===

    // Build constructor bytecode
    let (init_bytecode, init_max_locals) =
        build_constructor_bytecode(&mut cp, super_init_ref, closure_ctx)?;

    // Build doCall bytecode
    let (docall_bytecode, docall_max_locals) =
        build_docall_bytecode(&mut cp, parameters, body, closure_ctx, closure_classes)?;
    let init_code_len = checked_method_code_length("generated closure <init>", &init_bytecode)
        .map_err(|error| error.to_string())?;
    let docall_code_len = checked_method_code_length("generated closure doCall", &docall_bytecode)
        .map_err(|error| error.to_string())?;
    // Every currently supported raw closure opcode can push at most one
    // category-1 slot per byte. The byte length is therefore a safe upper bound,
    // unlike the previous fixed value of ten.
    let docall_max_stack = u16::try_from(docall_bytecode.len())
        .map_err(|_| "generated closure doCall stack bound exceeds the JVM limit".to_string())?
        .max(1);
    // Add BootstrapMethods attribute name if we have bootstrap methods
    let bootstrap_attr_name_idx = if cp.has_bootstrap_methods() {
        Some(cp.add_utf8("BootstrapMethods"))
    } else {
        None
    };

    cp.validate().map_err(|error| error.to_string())?;
    let cp_count = checked_u16_count(cp.len(), "generated closure constant pool")
        .map_err(|error| error.to_string())?;
    let fields_count = closure_ctx
        .captures
        .len()
        .checked_add(3)
        .ok_or_else(|| "generated closure field count overflowed".to_string())
        .and_then(|count| {
            checked_u16_count(count, "generated closure field").map_err(|error| error.to_string())
        })?;

    let bootstrap_methods: Vec<crate::constant_pool::BootstrapMethodEntry> =
        cp.bootstrap_methods().to_vec();

    // NOW serialize constant pool (after all bytecode is built)
    let cp_entries = cp.get_entries();

    // === Write .class file structure ===

    // Magic number
    class_bytes.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);

    // Version: Java 21 (major=65, minor=0)
    class_bytes.extend_from_slice(&[0x00, 0x00]); // minor
    class_bytes.extend_from_slice(&[0x00, 0x41]); // major (65)

    class_bytes.extend_from_slice(&cp_count.to_be_bytes());

    crate::groovy_producer::write_constant_pool_entries(&mut class_bytes, cp_entries)
        .map_err(|error| error.to_string())?;

    // Access flags: ACC_PUBLIC | ACC_SUPER (0x0021)
    class_bytes.extend_from_slice(&[0x00, 0x21]);

    // this_class
    class_bytes.extend_from_slice(&class_name_idx.to_be_bytes());

    // super_class
    class_bytes.extend_from_slice(&super_class_idx.to_be_bytes());

    // interfaces_count
    class_bytes.extend_from_slice(&[0x00, 0x00]);

    // fields_count: 3 standard fields (delegate, owner, resolveStrategy) + captured vars
    class_bytes.extend_from_slice(&fields_count.to_be_bytes());

    // Write standard Groovy Closure fields first (to match runtime layout)
    // Field 0: delegate
    class_bytes.extend_from_slice(&[0x00, 0x02]); // ACC_PRIVATE
    class_bytes.extend_from_slice(&delegate_name_idx.to_be_bytes());
    class_bytes.extend_from_slice(&delegate_desc_idx.to_be_bytes());
    class_bytes.extend_from_slice(&[0x00, 0x00]); // attributes_count = 0

    // Field 1: owner
    class_bytes.extend_from_slice(&[0x00, 0x02]); // ACC_PRIVATE
    class_bytes.extend_from_slice(&owner_name_idx.to_be_bytes());
    class_bytes.extend_from_slice(&owner_desc_idx.to_be_bytes());
    class_bytes.extend_from_slice(&[0x00, 0x00]); // attributes_count = 0

    // Field 2: resolveStrategy
    class_bytes.extend_from_slice(&[0x00, 0x02]); // ACC_PRIVATE
    class_bytes.extend_from_slice(&resolve_strategy_name_idx.to_be_bytes());
    class_bytes.extend_from_slice(&resolve_strategy_desc_idx.to_be_bytes());
    class_bytes.extend_from_slice(&[0x00, 0x00]); // attributes_count = 0

    // Write fields for captured variables (starting at field 3)
    for (field_name_idx, field_desc_idx) in field_names.iter().zip(field_descs.iter()) {
        // ACC_PRIVATE | ACC_FINAL (0x0012)
        class_bytes.extend_from_slice(&[0x00, 0x12]);
        class_bytes.extend_from_slice(&field_name_idx.to_be_bytes());
        class_bytes.extend_from_slice(&field_desc_idx.to_be_bytes());
        // attributes_count = 0
        class_bytes.extend_from_slice(&[0x00, 0x00]);
    }

    // methods_count (<init> + doCall)
    class_bytes.extend_from_slice(&[0x00, 0x02]);

    // === Method: <init> ===
    class_bytes.extend_from_slice(&[0x00, 0x01]); // ACC_PUBLIC
    class_bytes.extend_from_slice(&init_name_idx.to_be_bytes());
    class_bytes.extend_from_slice(&init_desc_idx.to_be_bytes());
    class_bytes.extend_from_slice(&[0x00, 0x01]); // attributes_count

    // Code attribute for <init> (bytecode was built earlier)
    class_bytes.extend_from_slice(&code_attr_idx.to_be_bytes());
    let init_attr_len = 12 + init_code_len;
    class_bytes.extend_from_slice(&init_attr_len.to_be_bytes());
    class_bytes.extend_from_slice(&[0x00, 0x05]); // max_stack
    class_bytes.extend_from_slice(&init_max_locals.to_be_bytes());
    class_bytes.extend_from_slice(&init_code_len.to_be_bytes());
    class_bytes.extend_from_slice(&init_bytecode);
    class_bytes.extend_from_slice(&[0x00, 0x00]); // exception_table_length
    class_bytes.extend_from_slice(&[0x00, 0x00]); // attributes_count

    // === Method: doCall ===
    class_bytes.extend_from_slice(&[0x00, 0x01]); // ACC_PUBLIC
    class_bytes.extend_from_slice(&docall_name_idx.to_be_bytes());
    class_bytes.extend_from_slice(&docall_desc_idx.to_be_bytes());
    class_bytes.extend_from_slice(&[0x00, 0x01]); // attributes_count

    // Code attribute for doCall (bytecode was built earlier)
    class_bytes.extend_from_slice(&code_attr_idx.to_be_bytes());
    let docall_attr_len = 12 + docall_code_len;
    class_bytes.extend_from_slice(&docall_attr_len.to_be_bytes());
    class_bytes.extend_from_slice(&docall_max_stack.to_be_bytes());
    class_bytes.extend_from_slice(&docall_max_locals.to_be_bytes());
    class_bytes.extend_from_slice(&docall_code_len.to_be_bytes());
    class_bytes.extend_from_slice(&docall_bytecode);
    class_bytes.extend_from_slice(&[0x00, 0x00]); // exception_table_length
    class_bytes.extend_from_slice(&[0x00, 0x00]); // attributes_count

    // Class attributes
    if let Some(attr_name_idx) = bootstrap_attr_name_idx {
        class_bytes.extend_from_slice(&[0x00, 0x01]);
        class_bytes.extend_from_slice(&attr_name_idx.to_be_bytes());

        let attr_length = checked_bootstrap_attribute_length(&bootstrap_methods)
            .map_err(|error| error.to_string())?;
        class_bytes.extend_from_slice(&attr_length.to_be_bytes());

        let num_methods = checked_u16_count(
            bootstrap_methods.len(),
            "generated closure bootstrap method",
        )
        .map_err(|error| error.to_string())?;
        class_bytes.extend_from_slice(&num_methods.to_be_bytes());
        for bm in &bootstrap_methods {
            class_bytes.extend_from_slice(&bm.bootstrap_method_ref.to_be_bytes());
            let num_args = checked_u16_count(
                bm.bootstrap_arguments.len(),
                "generated closure bootstrap argument",
            )
            .map_err(|error| error.to_string())?;
            class_bytes.extend_from_slice(&num_args.to_be_bytes());
            for arg in &bm.bootstrap_arguments {
                class_bytes.extend_from_slice(&arg.to_be_bytes());
            }
        }
    } else {
        class_bytes.extend_from_slice(&[0x00, 0x00]);
    }

    Ok(class_bytes)
}
