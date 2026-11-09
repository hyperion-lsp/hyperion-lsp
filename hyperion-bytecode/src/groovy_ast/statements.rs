// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Statement compilation

use super::{
    is_null_expression, types::parse_groovy_type_annotation, ExceptionHandler, FinallyContext,
    GroovyAstCompiler, VariableInfo,
};
use crate::java_compiler::JavaType;
use hyperion_groovy::{Expression, Statement};
use tracing::trace;

impl GroovyAstCompiler {
    pub fn compile_statement(&mut self, statement: &Statement) -> Option<JavaType> {
        match statement {
            Statement::Expression(expr) => {
                let expr_type = self.compile_expression(expr);
                if let Some(expr_type) = &expr_type {
                    self.discard_expression_result(expr_type);
                }
                expr_type
            }

            Statement::Return(Some(expr)) => {
                let expr_type = self.compile_expression(expr)?;
                if expr_type == JavaType::Void {
                    self.reject_lowering("void expression cannot be returned");
                    return None;
                }
                if self.current_method_return_type == JavaType::Void {
                    self.reject_lowering("void Groovy method cannot return a value");
                    self.discard_expression_result(&expr_type);
                    if !self.finally_stack.is_empty() {
                        self.compile_inline_finally();
                    }
                    self.stmt_compiler.compile_return_void();
                    return Some(expr_type);
                }
                let return_type = self.adapt_method_return(&expr_type, is_null_expression(expr))?;

                // If inside a finally block, store return value, run finally, then load and return
                if !self.finally_stack.is_empty() {
                    if let Some(ctx) = self.finally_stack.last().cloned() {
                        if let Some(return_slot) = ctx.return_value_slot {
                            // Store return value
                            self.compile_store(&return_type, return_slot);
                            // Execute finally block
                            self.compile_inline_finally();
                            // Load value and return (for finally, we still use type-specific return)
                            self.compile_load_and_return(&return_type, return_slot);
                        }
                    }
                } else {
                    self.compile_return_top(&return_type);
                }
                Some(return_type)
            }

            Statement::Return(None) => {
                // If inside a finally block, run finally first
                if !self.finally_stack.is_empty() {
                    self.compile_inline_finally();
                }
                match self.current_method_return_type {
                    JavaType::Void => self.stmt_compiler.compile_return_void(),
                    JavaType::Object(_) | JavaType::Array { .. } => self.compile_null_return(),
                    _ => self
                        .reject_lowering("primitive Groovy method cannot return without a value"),
                }
                None
            }

            Statement::VariableDecl {
                name,
                type_annotation,
                initializer,
            } => {
                trace!(
                    "Compiling VariableDecl: name={}, has_init={}, init_expr={:?}",
                    name,
                    initializer.is_some(),
                    initializer
                );

                let declared_type = if let Some(type_str) = type_annotation {
                    match parse_groovy_type_annotation(type_str) {
                        Ok(java_type) if java_type != JavaType::Void => Some(java_type),
                        Ok(_) => {
                            self.reject_lowering("void cannot be used as a Groovy local type");
                            return None;
                        }
                        Err(message) => {
                            self.reject_lowering(message);
                            return None;
                        }
                    }
                } else {
                    None
                };

                // Compile an initializer exactly once, after validating its
                // declared type, then adapt it before choosing the store opcode.
                let initializer_type = if let Some(expression) = initializer {
                    Some(self.compile_expression(expression)?)
                } else {
                    None
                };
                let var_type = if let Some(declared_type) = declared_type {
                    declared_type
                } else if let Some(initializer_type) = &initializer_type {
                    initializer_type.clone()
                } else {
                    // No type info available - default to Object
                    JavaType::Object("java/lang/Object".to_string())
                };

                // Allocate the full JVM width (`long`/`double` occupy two slots).
                let slot = self.reserve_local(&var_type, "variable declaration")?;
                trace!(
                    "Adding to locals: name={}, slot={}, type={:?}",
                    name,
                    slot,
                    var_type
                );
                self.declare_local(
                    name.clone(),
                    VariableInfo {
                        slot,
                        java_type: var_type.clone(),
                        declared_type: type_annotation.as_ref().map(|_| var_type.clone()),
                        scope_depth: self.scope_depth,
                    },
                );
                trace!("Locals now has {} entries", self.locals.len());

                if let Some(initializer_type) = &initializer_type {
                    let initializer_is_null = initializer.as_ref().is_some_and(is_null_expression);
                    self.adapt_declared_initializer(
                        &var_type,
                        initializer_type,
                        initializer_is_null,
                    )?;
                    self.compile_store(&var_type, slot);
                } else {
                    self.compile_default_local_value(&var_type);
                    self.compile_store(&var_type, slot);
                }

                Some(var_type)
            }

            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                // Statement conditions follow Groovy truth rather than assuming
                // that every condition is already an integer boolean.
                let condition_type = self.compile_expression(condition)?;
                self.coerce_to_truth_int(&condition_type)?;

                if else_body.is_some() {
                    // Pattern: if-then-else
                    // IFEQ else_label
                    // then_body
                    // GOTO end_label
                    // else_label:
                    // else_body
                    // end_label:
                    let else_label = self.stmt_compiler.new_label();
                    let end_label = self.stmt_compiler.new_label();

                    // Branch to else if condition is false (0)
                    self.stmt_compiler.emit_ifeq(else_label);

                    // Then body
                    self.enter_local_scope();
                    for stmt in then_body {
                        self.compile_statement(stmt);
                    }
                    self.exit_local_scope();
                    self.stmt_compiler.emit_goto(end_label);

                    // Else body
                    self.stmt_compiler.bind_label(else_label);
                    if let Some(else_stmts) = else_body {
                        self.enter_local_scope();
                        for stmt in else_stmts {
                            self.compile_statement(stmt);
                        }
                        self.exit_local_scope();
                    }

                    self.stmt_compiler.bind_label(end_label);
                } else {
                    // Pattern: if-then only
                    // IFEQ end_label
                    // then_body
                    // end_label:
                    let end_label = self.stmt_compiler.new_label();

                    // Branch to end if condition is false (0)
                    self.stmt_compiler.emit_ifeq(end_label);

                    // Then body
                    self.enter_local_scope();
                    for stmt in then_body {
                        self.compile_statement(stmt);
                    }
                    self.exit_local_scope();

                    self.stmt_compiler.bind_label(end_label);
                }
                None
            }

            Statement::While { condition, body } => {
                // Pattern: while (condition) { body }
                // loop_start:
                // condition
                // IFEQ loop_end
                // body
                // GOTO loop_start
                // loop_end:
                let loop_start = self.stmt_compiler.new_label();
                let loop_end = self.stmt_compiler.new_label();

                // Push loop context for break/continue
                self.stmt_compiler.push_loop_context(loop_start, loop_end);

                // Loop start
                self.stmt_compiler.bind_label(loop_start);

                // Compile and normalize the condition to a branchable boolean.
                let condition_type = self.compile_expression(condition)?;
                self.coerce_to_truth_int(&condition_type)?;

                // Branch to end if condition is false (0)
                self.stmt_compiler.emit_ifeq(loop_end);

                // Body - use our compile_statement to handle finally properly
                self.enter_local_scope();
                for stmt in body {
                    self.compile_statement(stmt);
                }
                self.exit_local_scope();

                // Jump back to start
                self.stmt_compiler.emit_goto(loop_start);

                // Loop end
                self.stmt_compiler.bind_label(loop_end);

                // Pop loop context
                self.stmt_compiler.pop_loop_context();

                None
            }

            Statement::Block(stmts) => {
                // Enter new scope
                self.enter_local_scope();

                // Compile block statements
                let result = self.compile_statements(stmts);

                self.exit_local_scope();
                // Note: We don't reset next_local - slots are reusable but numbering monotonic
                result
            }

            Statement::For {
                variable,
                iterable,
                body,
            } => {
                // For-in loop: for (item in collection) { ... }
                // Pattern:
                // 1. Evaluate iterable expression
                // 2. Get iterator (invokedynamic iterator())
                // 3. Loop: hasNext() -> ifne -> next() -> store in variable -> body -> goto loop
                // 4. Exit

                // INVOKEINTERFACE requires a reference receiver. The current
                // lowering supports Iterable objects, not primitive values or
                // JVM arrays (which do not implement java/lang/Iterable).
                let iterable_type = self.compile_expression(iterable)?;
                if !matches!(iterable_type, JavaType::Object(_)) {
                    self.reject_lowering(format!(
                        "Groovy for-in requires an Iterable reference, got {iterable_type:?}"
                    ));
                    return None;
                }
                self.enter_local_scope();

                // Get iterator via invokedynamic
                let iterator_method = self
                    .stmt_compiler
                    .expr_compiler()
                    .constant_pool()
                    .add_interface_method_ref(
                        "java/lang/Iterable",
                        "iterator",
                        "()Ljava/util/Iterator;",
                    );
                self.stmt_compiler.expr_compiler().emit_byte(0xB9); // INVOKEINTERFACE
                self.stmt_compiler
                    .expr_compiler()
                    .emit_short(iterator_method);
                self.stmt_compiler.expr_compiler().emit_byte(1); // count (1 arg: this)
                self.stmt_compiler.expr_compiler().emit_byte(0); // padding

                // Store iterator in temp local
                let Some(iterator_slot) = self.reserve_local(
                    &JavaType::Object("java/util/Iterator".to_string()),
                    "for-loop iterator",
                ) else {
                    self.exit_local_scope();
                    return None;
                };
                self.stmt_compiler
                    .expr_compiler()
                    .compile_astore(iterator_slot);

                // IntRange is the one iterable whose element type this lowering
                // can determine without generic-signature analysis. Its
                // iterator returns boxed Integers, which we unbox below so
                // arithmetic in the loop body retains a precise numeric type.
                // Keep arbitrary iterable elements as Object rather than making
                // an unsafe type assumption.
                let loop_var_type = match &iterable_type {
                    JavaType::Object(name) if name == "groovy/lang/IntRange" => JavaType::Int,
                    _ => JavaType::Object("java/lang/Object".to_string()),
                };
                let Some(var_slot) = self.reserve_local(&loop_var_type, "for-loop variable") else {
                    self.exit_local_scope();
                    return None;
                };
                self.declare_local(
                    variable.clone(),
                    VariableInfo {
                        slot: var_slot,
                        java_type: loop_var_type.clone(),
                        declared_type: None,
                        scope_depth: self.scope_depth,
                    },
                );

                // Loop start
                let loop_start = self.stmt_compiler.new_label();
                let loop_end = self.stmt_compiler.new_label();

                // Push loop context for break/continue
                self.stmt_compiler.push_loop_context(loop_start, loop_end);

                self.stmt_compiler.bind_label(loop_start);

                // Load iterator and call hasNext()
                self.stmt_compiler
                    .expr_compiler()
                    .compile_aload(iterator_slot, "java/util/Iterator");
                let has_next = self
                    .stmt_compiler
                    .expr_compiler()
                    .constant_pool()
                    .add_interface_method_ref("java/util/Iterator", "hasNext", "()Z");
                self.stmt_compiler.expr_compiler().emit_byte(0xB9); // INVOKEINTERFACE
                self.stmt_compiler.expr_compiler().emit_short(has_next);
                self.stmt_compiler.expr_compiler().emit_byte(1);
                self.stmt_compiler.expr_compiler().emit_byte(0);
                self.stmt_compiler.expr_compiler().stack_depth().push1(); // hasNext returns boolean (int)

                // If false, exit loop
                self.stmt_compiler.emit_ifeq(loop_end);

                // Call next() and store in variable
                self.stmt_compiler
                    .expr_compiler()
                    .compile_aload(iterator_slot, "java/util/Iterator");
                let next_method = self
                    .stmt_compiler
                    .expr_compiler()
                    .constant_pool()
                    .add_interface_method_ref("java/util/Iterator", "next", "()Ljava/lang/Object;");
                self.stmt_compiler.expr_compiler().emit_byte(0xB9); // INVOKEINTERFACE
                self.stmt_compiler.expr_compiler().emit_short(next_method);
                self.stmt_compiler.expr_compiler().emit_byte(1);
                self.stmt_compiler.expr_compiler().emit_byte(0);
                self.stmt_compiler.expr_compiler().stack_depth().push1(); // next returns Object

                // Store in variable. IntRange's Iterator descriptor still
                // returns Object, so recover the known Integer element type at
                // this boundary before using a primitive local slot.
                if loop_var_type == JavaType::Int {
                    let integer_class = self
                        .stmt_compiler
                        .expr_compiler()
                        .constant_pool()
                        .add_class("java/lang/Integer");
                    self.stmt_compiler.expr_compiler().emit_byte(0xC0); // CHECKCAST
                    self.stmt_compiler.expr_compiler().emit_short(integer_class);
                    let int_value = self
                        .stmt_compiler
                        .expr_compiler()
                        .constant_pool()
                        .add_method_ref("java/lang/Integer", "intValue", "()I");
                    self.stmt_compiler.expr_compiler().emit_byte(0xB6); // INVOKEVIRTUAL
                    self.stmt_compiler.expr_compiler().emit_short(int_value);
                }
                self.compile_store(&loop_var_type, var_slot);

                // Compile body
                for stmt in body {
                    self.compile_statement(stmt);
                }

                // Jump back to loop start
                self.stmt_compiler.emit_goto(loop_start);

                // Loop end
                self.stmt_compiler.bind_label(loop_end);

                // Pop loop context
                self.stmt_compiler.pop_loop_context();

                self.exit_local_scope();

                None
            }

            Statement::TraditionalFor {
                init,
                condition,
                increment,
                body,
            } => {
                // Traditional for: for (init; condition; increment) { body }
                self.enter_local_scope();

                // Compile init
                if let Some(init_stmt) = init {
                    self.compile_statement(init_stmt);
                }

                // Loop labels
                let loop_start = self.stmt_compiler.new_label();
                let loop_increment = self.stmt_compiler.new_label();
                let loop_end = self.stmt_compiler.new_label();

                // For continue: jump to increment (or loop start if no increment)
                let continue_target = if increment.is_some() {
                    loop_increment
                } else {
                    loop_start
                };

                // Push loop context for break/continue
                self.stmt_compiler
                    .push_loop_context(continue_target, loop_end);

                // Bind loop start
                self.stmt_compiler.bind_label(loop_start);

                // Compile condition (if present)
                if let Some(cond_expr) = condition {
                    let Some(condition_type) = self.compile_expression(cond_expr) else {
                        self.stmt_compiler.pop_loop_context();
                        self.exit_local_scope();
                        return None;
                    };
                    if self.coerce_to_truth_int(&condition_type).is_none() {
                        self.stmt_compiler.pop_loop_context();
                        self.exit_local_scope();
                        return None;
                    }
                    // If false, exit loop
                    self.stmt_compiler.emit_ifeq(loop_end);
                }

                // Compile body
                for stmt in body {
                    self.compile_statement(stmt);
                }

                // Bind increment label
                self.stmt_compiler.bind_label(loop_increment);

                // Compile increment (if present)
                if let Some(incr_expr) = increment {
                    let Some(increment_type) = self.compile_expression(incr_expr) else {
                        self.stmt_compiler.pop_loop_context();
                        self.exit_local_scope();
                        return None;
                    };
                    self.discard_expression_result(&increment_type);
                }

                // Jump back to loop start
                self.stmt_compiler.emit_goto(loop_start);

                // Loop end
                self.stmt_compiler.bind_label(loop_end);

                // Pop loop context
                self.stmt_compiler.pop_loop_context();

                self.exit_local_scope();

                None
            }

            Statement::Throw(expr) => {
                // Compile exception expression
                let exception_type = self.compile_expression(expr)?;
                let is_null = matches!(expr, Expression::NullLiteral);
                let is_throwable = matches!(
                    &exception_type,
                    JavaType::Object(class_name) if is_throwable_class_name(class_name)
                );
                if !is_null && !is_throwable {
                    self.reject_lowering(format!(
                        "Groovy throw requires a Throwable reference, got {exception_type:?}"
                    ));
                    return None;
                }

                // Emit athrow
                self.stmt_compiler.expr_compiler().emit_byte(0xBF); // ATHROW
                self.stmt_compiler.expr_compiler().stack_depth().pop1(); // Exception consumed

                None
            }

            Statement::Break => {
                // If inside a finally block, run finally first then jump to captured break target
                if !self.finally_stack.is_empty() {
                    if let Some(ctx) = self.finally_stack.last().cloned() {
                        self.compile_inline_finally();
                        // Use the captured break target from when we entered the try block
                        if let Some(break_label) = ctx.break_target {
                            self.stmt_compiler.emit_goto(break_label);
                        } else {
                            // No loop context at try-entry, fall back to normal break
                            self.compile_break_or_reject();
                        }
                    }
                } else {
                    self.compile_break_or_reject();
                }
                None
            }

            Statement::Continue => {
                // If inside a finally block, run finally first then jump to captured continue target
                if !self.finally_stack.is_empty() {
                    if let Some(ctx) = self.finally_stack.last().cloned() {
                        self.compile_inline_finally();
                        // Use the captured continue target from when we entered the try block
                        if let Some(continue_label) = ctx.continue_target {
                            self.stmt_compiler.emit_goto(continue_label);
                        } else {
                            // No loop context at try-entry, fall back to normal continue
                            self.compile_continue_or_reject();
                        }
                    }
                } else {
                    self.compile_continue_or_reject();
                }
                None
            }

            Statement::Switch {
                expression: _,
                cases: _,
                default_case: _,
            } => {
                self.reject_lowering("Groovy switch statements");
                None
            }

            Statement::Try {
                body,
                catch_blocks,
                finally_block,
            } => {
                let mut normalized_catch_types = Vec::with_capacity(catch_blocks.len());
                for catch_block in catch_blocks {
                    let catch_type = match parse_groovy_type_annotation(&catch_block.exception_type)
                    {
                        Ok(JavaType::Object(class_name))
                            if is_throwable_class_name(&class_name) =>
                        {
                            class_name
                        }
                        Ok(java_type) => {
                            self.reject_lowering(format!(
                                "Groovy catch requires a Throwable class, got {java_type:?}"
                            ));
                            return None;
                        }
                        Err(message) => {
                            self.reject_lowering(message);
                            return None;
                        }
                    };
                    normalized_catch_types.push(catch_type);
                }

                // JVM-spec compliant try/catch/finally compilation
                // Finally block must execute on ALL exit paths:
                // 1. Normal completion of try block
                // 2. Normal completion of catch blocks
                // 3. Uncaught exception (from try or catch)
                // 4. Early return (handled in Statement::Return)
                // 5. Break/continue (handled in Statement::Break/Continue)

                let has_finally = finally_block.is_some();

                // Setup FinallyContext if we have a finally block
                if let Some(ref finally_stmts) = finally_block {
                    // Allocate slots for exception and return value storage
                    let exception_slot = self.reserve_local(
                        &JavaType::Object("java/lang/Throwable".to_string()),
                        "finally exception",
                    )?;

                    // A Groovy script has an Object return descriptor but may
                    // return a category-2 primitive before boxing. Reserve two
                    // slots so inline-finally locals can never overlap it.
                    let return_type = self.current_method_return_type.clone();
                    let return_value_slot = match &return_type {
                        JavaType::Void => None,
                        _ => Some(self.reserve_local(&JavaType::Long, "finally return value")?),
                    };

                    // Capture loop targets at try-entry time
                    let (break_target, continue_target) = self.get_loop_targets();

                    self.finally_stack.push(FinallyContext {
                        finally_statements: finally_stmts.clone(),
                        exception_slot,
                        return_value_slot,
                        break_target,
                        continue_target,
                    });
                }

                let try_start_pc = self.stmt_compiler.pc();

                // Compile try block
                self.enter_local_scope();
                for stmt in body {
                    self.compile_statement(stmt);
                }
                self.exit_local_scope();

                let try_end_pc = self.stmt_compiler.pc();

                // After normal try completion, compile inline finally then jump to end
                let end_label = self.stmt_compiler.new_label();
                if has_finally {
                    self.compile_inline_finally();
                }
                self.stmt_compiler.emit_goto(end_label);

                // Will be set after all catch blocks

                // Compile catch blocks
                let mut catch_handler_pcs = Vec::new();
                for (catch_block, normalized_catch_type) in
                    catch_blocks.iter().zip(normalized_catch_types)
                {
                    self.enter_local_scope();
                    let handler_pc = self.stmt_compiler.pc();
                    catch_handler_pcs.push((handler_pc, normalized_catch_type.clone()));

                    // Stack has exception reference from JVM
                    self.stmt_compiler.expr_compiler().stack_depth().push1();

                    // Store exception in variable
                    let exception_type = JavaType::Object(normalized_catch_type);
                    let Some(exception_var_slot) =
                        self.reserve_local(&exception_type, "catch variable")
                    else {
                        self.exit_local_scope();
                        return None;
                    };
                    self.declare_local(
                        catch_block.variable_name.clone(),
                        VariableInfo {
                            slot: exception_var_slot,
                            java_type: exception_type,
                            declared_type: None,
                            scope_depth: self.scope_depth,
                        },
                    );

                    // ASTORE exception
                    self.stmt_compiler
                        .expr_compiler()
                        .compile_astore(exception_var_slot);

                    // Compile catch body, tracking if last statement terminates control flow
                    let mut last_stmt_terminates = false;
                    for (idx, stmt) in catch_block.body.iter().enumerate() {
                        self.compile_statement(stmt);
                        if idx == catch_block.body.len() - 1
                            && matches!(
                                stmt,
                                Statement::Return(_)
                                    | Statement::Break
                                    | Statement::Continue
                                    | Statement::Throw(_)
                            )
                        {
                            last_stmt_terminates = true;
                        }
                    }

                    // Only add finally/goto if catch doesn't terminate itself
                    // (Return/Break/Continue/Throw already handle finally inline)
                    if !last_stmt_terminates {
                        if has_finally {
                            self.compile_inline_finally();
                        }
                        self.stmt_compiler.emit_goto(end_label);
                    }

                    self.exit_local_scope();
                }

                let catch_end_pc = self.stmt_compiler.pc();

                // Compile catch-all handler for uncaught exceptions (if we have finally)
                if has_finally {
                    let catchall_handler_pc = self.stmt_compiler.pc();

                    // JVM pushes exception on stack
                    self.stmt_compiler.expr_compiler().stack_depth().push1();

                    // Store exception in reserved slot
                    let exception_slot = self.finally_stack.last().unwrap().exception_slot;
                    self.stmt_compiler
                        .expr_compiler()
                        .compile_astore(exception_slot);

                    // Execute finally block
                    self.compile_inline_finally();

                    // Re-load and re-throw the exception
                    self.stmt_compiler
                        .expr_compiler()
                        .compile_aload(exception_slot, "java/lang/Throwable");
                    self.stmt_compiler.expr_compiler().emit_byte(0xBF); // ATHROW
                    self.stmt_compiler.expr_compiler().stack_depth().pop1();

                    // Register catch-all handler covering try block
                    if try_start_pc < try_end_pc {
                        self.exception_handlers.push(ExceptionHandler {
                            start_pc: try_start_pc,
                            end_pc: try_end_pc,
                            handler_pc: catchall_handler_pc,
                            catch_type: None, // catch-all (finally)
                        });
                    }

                    // Register catch-all handler covering catch blocks (if any)
                    if !catch_handler_pcs.is_empty() {
                        // Get the start of first catch handler
                        let catch_start_pc = catch_handler_pcs[0].0;
                        if catch_start_pc < catch_end_pc {
                            self.exception_handlers.push(ExceptionHandler {
                                start_pc: catch_start_pc,
                                end_pc: catch_end_pc,
                                handler_pc: catchall_handler_pc,
                                catch_type: None, // catch-all (finally)
                            });
                        }
                    }
                }

                // Register typed catch handlers
                for (handler_pc, exception_type) in catch_handler_pcs {
                    if try_start_pc < try_end_pc {
                        self.exception_handlers.push(ExceptionHandler {
                            start_pc: try_start_pc,
                            end_pc: try_end_pc,
                            handler_pc,
                            catch_type: Some(exception_type),
                        });
                    }
                }

                // Bind end label (normal completion point)
                self.stmt_compiler.bind_label(end_label);

                // Pop FinallyContext
                if has_finally {
                    self.finally_stack.pop();
                }

                None
            }

            Statement::MethodDef {
                name,
                parameters,
                body,
                ..
            } => {
                // In Groovy scripts, `def add(a, b) { body }` defines a script method
                // For our VM tests, we compile it as a closure stored in a local variable
                // This allows `add(5, 7)` to work via the closure call pattern
                trace!(
                    "Compiling MethodDef: name={}, params={:?}",
                    name,
                    parameters.iter().map(|p| &p.name).collect::<Vec<_>>()
                );

                // Extract parameter names as Vec<String> for closure compilation
                let param_names: Vec<String> = parameters.iter().map(|p| p.name.clone()).collect();

                // Compile as a closure
                let closure_type = self.compile_closure(&param_names, body, false)?;

                // Store the closure in a local variable with the method name
                let slot = self.reserve_local(&closure_type, "script method closure")?;
                self.declare_local(
                    name.clone(),
                    VariableInfo {
                        slot,
                        java_type: closure_type.clone(),
                        declared_type: None,
                        scope_depth: self.scope_depth,
                    },
                );

                // Store the closure reference
                self.stmt_compiler.expr_compiler().compile_astore(slot);

                Some(closure_type)
            }

            Statement::Unsupported(feature) => {
                self.reject_lowering(feature.clone());
                None
            }
        }
    }

    fn discard_expression_result(&mut self, expression_type: &JavaType) {
        match expression_type {
            JavaType::Long | JavaType::Double => {
                self.stmt_compiler.expr_compiler().emit_byte(0x58); // POP2
                self.stmt_compiler.expr_compiler().stack_depth().pop2();
            }
            JavaType::Void => {}
            _ => {
                self.stmt_compiler.expr_compiler().emit_byte(0x57); // POP
                self.stmt_compiler.expr_compiler().stack_depth().pop1();
            }
        }
    }

    pub(super) fn adapt_declared_initializer(
        &mut self,
        declared_type: &JavaType,
        initializer_type: &JavaType,
        initializer_is_null: bool,
    ) -> Option<()> {
        if declared_type == initializer_type {
            return Some(());
        }
        if declared_type.is_numeric() && initializer_type.is_numeric() {
            if can_widen_numeric(initializer_type, declared_type) {
                return self.widen_numeric_top(initializer_type, declared_type);
            }
            self.reject_lowering(format!(
                "Groovy typed local cannot adapt initializer {initializer_type:?} to {declared_type:?}"
            ));
            return None;
        }
        if *declared_type == JavaType::Boolean
            && matches!(initializer_type, JavaType::Object(name) if name == "java/lang/Boolean")
        {
            return self.unbox_boolean_top(initializer_type);
        }
        if initializer_is_null
            && matches!(declared_type, JavaType::Object(_) | JavaType::Array { .. })
        {
            return Some(());
        }
        if matches!(declared_type, JavaType::Object(name) if matches!(name.as_str(), "Object" | "java/lang/Object" | "java.lang.Object"))
            && *initializer_type != JavaType::Void
        {
            self.compile_box_primitive(initializer_type);
            return Some(());
        }

        self.reject_lowering(format!(
            "Groovy typed local cannot adapt initializer {initializer_type:?} to {declared_type:?}"
        ));
        None
    }

    fn compile_default_local_value(&mut self, java_type: &JavaType) {
        let compiler = self.stmt_compiler.expr_compiler();
        match java_type {
            JavaType::Long => {
                compiler.emit_byte(0x09); // LCONST_0
                compiler.stack_depth().push2();
            }
            JavaType::Float => {
                compiler.emit_byte(0x0B); // FCONST_0
                compiler.stack_depth().push1();
            }
            JavaType::Double => {
                compiler.emit_byte(0x0E); // DCONST_0
                compiler.stack_depth().push2();
            }
            JavaType::Object(_) | JavaType::Array { .. } => {
                compiler.emit_byte(0x01); // ACONST_NULL
                compiler.stack_depth().push1();
            }
            JavaType::Void => unreachable!("void local declaration rejected before allocation"),
            _ => {
                compiler.emit_byte(0x03); // ICONST_0
                compiler.stack_depth().push1();
            }
        }
    }
}

fn is_throwable_class_name(class_name: &str) -> bool {
    let simple_name = class_name
        .rsplit(['/', '.', '$'])
        .next()
        .unwrap_or(class_name);
    simple_name == "Throwable"
        || simple_name.ends_with("Exception")
        || simple_name.ends_with("Error")
}

pub(super) fn can_widen_numeric(source: &JavaType, target: &JavaType) -> bool {
    let rank = |java_type: &JavaType| match java_type {
        JavaType::Byte | JavaType::Short | JavaType::Char | JavaType::Int => Some(0),
        JavaType::Long => Some(1),
        JavaType::Float => Some(2),
        JavaType::Double => Some(3),
        _ => None,
    };
    matches!((rank(source), rank(target)), (Some(source), Some(target)) if source <= target)
}
