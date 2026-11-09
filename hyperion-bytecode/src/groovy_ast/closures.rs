// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Closure compilation and free variable analysis
//!
//! Handles closure creation, free-variable capture, and parent-method emission.

use super::{
    closure_class::generate_closure_class_bytes, closure_scope::free_variable_names,
    CapturedVariable, ClosureCompilationContext, GroovyAstCompiler,
};
use crate::java_compiler::JavaType;
use hyperion_groovy::Statement;
#[cfg(debug_assertions)]
use tracing::trace;

impl GroovyAstCompiler {
    pub(super) fn analyze_free_variables(
        &self,
        parameters: &[String],
        body: &[Statement],
    ) -> Vec<CapturedVariable> {
        free_variable_names(parameters, body)
            .into_iter()
            .filter_map(|name| {
                self.locals.get(&name).map(|variable| CapturedVariable {
                    name,
                    java_type: variable.java_type.clone(),
                    enclosing_slot: variable.slot,
                })
            })
            .collect()
    }

    /// Compile a closure expression to bytecode
    ///
    /// Architecture:
    /// 1. Analyze closure for free variables (variables captured from enclosing scope)
    /// 2. Create ClosureCompilationContext with parameters and captures
    /// 3. Populate with types from analyze_free_variables()
    /// 4. Emit bytecode to instantiate the closure
    ///
    pub(super) fn compile_closure(
        &mut self,
        parameters: &[String],
        body: &[Statement],
        implicit_it: bool,
    ) -> Option<JavaType> {
        // Get a unique closure ID from the expression compiler
        let closure_id = self.stmt_compiler.expr_compiler().new_closure_id();
        let mut effective_params = parameters.to_vec();
        if implicit_it && parameters.is_empty() {
            effective_params.push("it".to_string());
        }

        // Create the closure compilation context with proper naming
        let mut closure_ctx =
            ClosureCompilationContext::new(format!("Closure$_closure_{}", closure_id));

        // Add closure parameters to context
        for param in &effective_params {
            closure_ctx.add_parameter(
                param.clone(),
                JavaType::Object("java/lang/Object".to_string()),
            );
        }

        // PHASE 2.2: Analyze free variables (captured from enclosing scope)
        let free_vars = self.analyze_free_variables(&effective_params, body);

        // Add captured variables to context
        for var in &free_vars {
            closure_ctx.add_capture(var.name.clone(), var.java_type.clone(), var.enclosing_slot);
        }

        // Generate into a private map before mutating the parent method. A rejected
        // body must not leave either partial parent bytecode or orphan nested classes.
        let mut generated_classes = std::collections::HashMap::new();
        let closure_bytes = match generate_closure_class_bytes(
            &closure_ctx,
            &effective_params,
            body,
            &mut generated_classes,
        ) {
            Ok(bytes) => bytes,
            Err(message) => {
                self.reject_lowering(message);
                return None;
            }
        };

        // PHASE 2.3: Emit bytecode to instantiate closure with captured variables
        // Sequence:
        // 1. NEW Closure$_closure_N
        // 2. DUP (duplicate reference: one for INVOKESPECIAL, one to return)
        // 3. ACONST_NULL (owner parameter to Closure constructor)
        // 4. ACONST_NULL (thisObject parameter to Closure constructor)
        // 5. ALOAD/DLOAD/etc for each captured variable
        // 6. INVOKESPECIAL <init>(Object owner, Object thisObject, ...captures)

        // 1. NEW - Create closure instance
        let class_index = self
            .stmt_compiler
            .expr_compiler()
            .constant_pool()
            .add_class(&closure_ctx.class_name);
        self.stmt_compiler.expr_compiler().emit_byte(0xBB); // NEW
        self.stmt_compiler.expr_compiler().emit_short(class_index);
        self.stmt_compiler.expr_compiler().stack_depth().push1();

        // 2. DUP - Keep reference for constructor call
        self.stmt_compiler.expr_compiler().emit_byte(0x59); // DUP
        self.stmt_compiler.expr_compiler().stack_depth().push1();

        // 3 & 4. Push owner and thisObject (this reference from enclosing context)
        self.stmt_compiler.expr_compiler().emit_byte(0x2A); // aload_0 (owner = this)
        self.stmt_compiler.expr_compiler().stack_depth().push1();
        self.stmt_compiler.expr_compiler().emit_byte(0x2A); // aload_0 (thisObject = this)
        self.stmt_compiler.expr_compiler().stack_depth().push1();

        // 5. Load captured variables onto stack
        for capture in &free_vars {
            match &capture.java_type {
                JavaType::Int
                | JavaType::Byte
                | JavaType::Short
                | JavaType::Char
                | JavaType::Boolean => {
                    self.stmt_compiler
                        .expr_compiler()
                        .compile_iload(capture.enclosing_slot);
                }
                JavaType::Long => {
                    self.stmt_compiler
                        .expr_compiler()
                        .compile_lload(capture.enclosing_slot);
                }
                JavaType::Float => {
                    self.stmt_compiler
                        .expr_compiler()
                        .compile_fload(capture.enclosing_slot);
                }
                JavaType::Double => {
                    self.stmt_compiler
                        .expr_compiler()
                        .compile_dload(capture.enclosing_slot);
                }
                JavaType::Object(_) | JavaType::Array { .. } => {
                    self.stmt_compiler
                        .expr_compiler()
                        .compile_aload(capture.enclosing_slot, "java/lang/Object");
                }
                JavaType::Void => {
                    // Should not happen in valid closure capture
                }
            }
        }

        // 6. Build constructor descriptor: (Ljava/lang/Object;Ljava/lang/Object;...captures)V
        let mut init_descriptor = "(Ljava/lang/Object;Ljava/lang/Object;".to_string();
        for capture in &free_vars {
            init_descriptor.push_str(&capture.java_type.descriptor());
        }
        init_descriptor.push_str(")V");

        // Emit INVOKESPECIAL to call constructor
        let init_method_index = self
            .stmt_compiler
            .expr_compiler()
            .constant_pool()
            .add_method_ref(&closure_ctx.class_name, "<init>", &init_descriptor);

        self.stmt_compiler.expr_compiler().emit_byte(0xB7); // INVOKESPECIAL
        self.stmt_compiler
            .expr_compiler()
            .emit_short(init_method_index);

        // INVOKESPECIAL consumes: receiver + owner + thisObject + all captures
        // But we have 2 copies (from DUP), so 1 copy remains as the return value
        // Stack management: INVOKESPECIAL pops (3 + N) values, returns void (0 values)
        // Net effect: reduces stack depth by (3 + N)
        for capture in &free_vars {
            if capture.java_type.is_category2() {
                self.stmt_compiler.expr_compiler().stack_depth().pop2();
            } else {
                self.stmt_compiler.expr_compiler().stack_depth().pop1();
            }
        }
        self.stmt_compiler.expr_compiler().stack_depth().pop1(); // Pop owner
        self.stmt_compiler.expr_compiler().stack_depth().pop1(); // Pop thisObject
        self.stmt_compiler.expr_compiler().stack_depth().pop1(); // Pop receiver
                                                                 // Remaining: the DUPed reference is still on stack (the return value)

        // Log closure context for debugging (Phase 2 diagnostic output)
        if !free_vars.is_empty() {
            // In Phase 2.2, we're detecting captures but not yet generating classes
            // This is validation that the analysis works correctly
            #[cfg(debug_assertions)]
            {
                trace!(
                    "Closure {}: {} parameters, {} captures",
                    closure_ctx.class_name,
                    closure_ctx.parameters.len(),
                    free_vars.len()
                );
            }
        }

        self.closure_classes.extend(generated_classes);
        self.closure_classes
            .insert(closure_ctx.class_name.clone(), closure_bytes);

        // Return type is groovy.lang.Closure
        Some(JavaType::Object("groovy/lang/Closure".to_string()))
    }
}
