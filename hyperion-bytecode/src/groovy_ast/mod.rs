// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy AST Compiler - Compile AST nodes to JVM bytecode
//!
//! This module walks Groovy AST (Expression/Statement) nodes and generates
//! JVM bytecode using the GroovyExpressionCompiler infrastructure.

mod closure_class;
mod closure_expression;
mod closure_scope;
mod closures;
mod control_expressions;
mod expressions;
mod operators;
mod safe_navigation;
mod statements;
mod static_calls;
mod types;

use crate::constant_pool::ConstantPoolBuilder;
use crate::groovy_compiler::{GroovyExpressionCompiler, GroovyStatementCompiler, Label};
use crate::java_compiler::JavaType;
use crate::producer::{BytecodeError, BytecodeResult};
use hyperion_groovy::{Expression, Statement};

#[derive(Clone, Debug)]
pub(super) struct VariableInfo {
    pub(super) slot: u16,
    pub(super) java_type: JavaType,
    pub(super) declared_type: Option<JavaType>,
    pub(super) scope_depth: usize,
}

/// Represents a variable captured from enclosing scope
#[derive(Clone, Debug)]
pub(super) struct CapturedVariable {
    pub(super) name: String,
    pub(super) java_type: JavaType,
    pub(super) enclosing_slot: u16,
}

/// Compilation context for a closure
///
/// ## Delegate resolution
///
/// Closures extend groovy.lang.Closure which provides:
/// - `delegate`: Object - the delegate instance
/// - `owner`: Object - the defining class/closure instance
/// - `resolveStrategy`: int - resolution order (see below)
///
/// ### Resolution Strategies (from groovy.lang.Closure):
/// - OWNER_FIRST (0): Try owner, then delegate, then closure (Groovy default)
/// - DELEGATE_FIRST (1): Try delegate, then owner, then closure (**Gradle default**)
/// - OWNER_ONLY (2): Only try owner
/// - DELEGATE_ONLY (3): Only try delegate
/// - TO_SELF (4): Only try the closure itself
///
/// ### Gradle DSL Example:
/// ```groovy
/// dependencies {
///     implementation "com.google.guava:guava:32.1.2"  // Resolves to DependencyHandler
/// }
/// ```
///
/// When `dependencies` closure is executed:
/// 1. Gradle sets `closure.delegate = DependencyHandlerStub`
/// 2. Gradle sets `closure.resolveStrategy = DELEGATE_FIRST` (1)
/// 3. Method calls inside closure (e.g., `implementation`) check delegate first
/// 4. The interpreter applies this resolution through `InvokerHelper`.
///
/// The runtime resolution contract is:
/// When InvokerHelper.invokeMethod() is called on a Closure:
/// 1. Check resolveStrategy field
/// 2. For DELEGATE_FIRST: try delegate.method(), then owner.method(), then closure.method()
/// 3. For OWNER_FIRST: try owner.method(), then delegate.method(), then closure.method()
/// 4. This enables Gradle DSL where method calls inside closures resolve to the delegate
#[derive(Clone, Debug)]
pub(super) struct ClosureCompilationContext {
    /// Free variables captured from enclosing scope
    pub(super) captures: Vec<CapturedVariable>,
    /// Closure parameters with their types
    pub(super) parameters: Vec<(String, JavaType)>,
    /// Delegate type for Gradle DSL method resolution
    pub(super) _delegate_type: Option<String>,
    /// Nesting level, where zero is a top-level closure
    pub(super) _nesting_level: u32,
    /// Parent context for a nested closure
    pub(super) _parent: Option<Box<ClosureCompilationContext>>,
    /// Synthetic class name for this closure
    pub(super) class_name: String,
}

impl ClosureCompilationContext {
    /// Create a new closure compilation context
    fn new(class_name: String) -> Self {
        Self {
            captures: Vec::new(),
            parameters: Vec::new(),
            _delegate_type: None,
            _nesting_level: 0,
            _parent: None,
            class_name,
        }
    }

    /// Add a captured variable
    fn add_capture(&mut self, name: String, java_type: JavaType, enclosing_slot: u16) {
        self.captures.push(CapturedVariable {
            name,
            java_type,
            enclosing_slot,
        });
    }

    /// Add a closure parameter
    fn add_parameter(&mut self, name: String, java_type: JavaType) {
        self.parameters.push((name, java_type));
    }
}

/// Exception handler entry for building exception tables
#[derive(Debug, Clone)]
pub struct ExceptionHandler {
    pub start_pc: usize,
    pub end_pc: usize,
    pub handler_pc: usize,
    pub catch_type: Option<String>, // None for finally, Some(class_name) for catch
}

/// Context for finally block execution on all exit paths
#[derive(Debug, Clone)]
struct FinallyContext {
    /// Statements to compile inline at each exit point
    finally_statements: Vec<Statement>,
    /// Slot to store exception for re-throwing (uncaught exceptions)
    exception_slot: u16,
    /// Slot to store return value (if non-void method)
    return_value_slot: Option<u16>,
    /// Break target label (captured from loop context at try-entry)
    break_target: Option<Label>,
    /// Continue target label (captured from loop context at try-entry)
    continue_target: Option<Label>,
}

/// Compiles Groovy AST to JVM bytecode
pub struct GroovyAstCompiler {
    pub(super) stmt_compiler: GroovyStatementCompiler,
    /// Variable name to VariableInfo mapping (with type and scope)
    pub(super) locals: std::collections::HashMap<String, VariableInfo>,
    /// Next available local slot
    pub(super) next_local: u16,
    /// Current scope depth for variable visibility tracking
    pub(super) scope_depth: usize,
    /// Outer bindings displaced by declarations in each active lexical scope.
    scope_shadows: Vec<std::collections::HashMap<String, Option<VariableInfo>>>,
    /// Generated closure class bytecode (class_name -> bytecode)
    /// Used to store generated closure inner class bytecode
    pub(super) closure_classes: std::collections::HashMap<String, Vec<u8>>,
    /// Exception handlers for try/catch blocks
    pub(super) exception_handlers: Vec<ExceptionHandler>,
    /// Stack of finally contexts for nested try-finally blocks
    finally_stack: Vec<FinallyContext>,
    /// Current method return type (used for storing return values before finally)
    pub(super) current_method_return_type: JavaType,
    /// Constructs that parsed successfully but cannot be lowered faithfully.
    lowering_errors: Vec<String>,
}

impl GroovyAstCompiler {
    /// Create a new AST compiler
    pub fn new(constant_pool: ConstantPoolBuilder) -> Self {
        let expr_compiler = GroovyExpressionCompiler::new(constant_pool);
        Self {
            stmt_compiler: GroovyStatementCompiler::new(expr_compiler),
            locals: Default::default(),
            next_local: 1,
            scope_depth: 0,
            scope_shadows: Vec::new(),
            closure_classes: std::collections::HashMap::new(),
            exception_handlers: Vec::new(),
            finally_stack: Vec::new(),
            current_method_return_type: JavaType::Object("java/lang/Object".to_string()),
            lowering_errors: Vec::new(),
        }
    }

    /// Set the current method's return type (used for finally block return value handling)
    pub fn set_method_return_type(&mut self, return_type: JavaType) {
        self.current_method_return_type = return_type;
    }

    /// Get current loop targets from the statement compiler (for finally block context)
    fn get_loop_targets(&self) -> (Option<Label>, Option<Label>) {
        self.stmt_compiler.current_loop_targets()
    }

    pub(super) fn reject_lowering(&mut self, message: impl Into<String>) {
        self.lowering_errors.push(message.into());
    }

    pub(super) fn enter_local_scope(&mut self) {
        self.scope_depth += 1;
        self.scope_shadows.push(Default::default());
    }

    pub(super) fn declare_local(&mut self, name: String, variable: VariableInfo) {
        if let Some(shadows) = self.scope_shadows.last_mut() {
            shadows
                .entry(name.clone())
                .or_insert_with(|| self.locals.get(&name).cloned());
        }
        self.locals.insert(name, variable);
    }

    pub(super) fn exit_local_scope(&mut self) {
        let scope_depth = self.scope_depth;
        self.locals
            .retain(|_, variable| variable.scope_depth != scope_depth);

        let shadows = self
            .scope_shadows
            .pop()
            .expect("local scope exit must match an entry");
        for (name, previous) in shadows {
            if let Some(previous) = previous {
                self.locals.insert(name, previous);
            } else {
                self.locals.remove(&name);
            }
        }
        self.scope_depth = self
            .scope_depth
            .checked_sub(1)
            .expect("local scope depth must not underflow");
    }

    fn compile_break_or_reject(&mut self) {
        if !self.stmt_compiler.compile_break() {
            self.reject_lowering("break outside a loop");
        }
    }

    fn compile_continue_or_reject(&mut self) {
        if !self.stmt_compiler.compile_continue() {
            self.reject_lowering("continue outside a loop");
        }
    }

    /// Reject output if any parsed construct could not be lowered faithfully.
    pub fn validate(&mut self) -> BytecodeResult<()> {
        if !self.lowering_errors.is_empty() {
            return Err(BytecodeError::UnsupportedFeature {
                feature: self.lowering_errors.join("; "),
            });
        }

        if let Err(feature) = self.stmt_compiler.validate_control_flow() {
            return Err(BytecodeError::UnsupportedFeature { feature });
        }

        self.max_stack().map(|_| ())
    }

    /// Compile the finally block inline at the current position
    /// Used for normal completion, return, break, and continue paths
    pub(super) fn compile_inline_finally(&mut self) {
        if let Some(ctx) = self.finally_stack.last().cloned() {
            self.enter_local_scope();
            for stmt in &ctx.finally_statements {
                self.compile_statement(stmt);
            }
            self.exit_local_scope();
        }
    }

    /// Compile a list of statements, returning the type of the last expression
    pub fn compile_statements(&mut self, statements: &[Statement]) -> Option<JavaType> {
        let mut last_type = None;

        for stmt in statements {
            last_type = self.compile_statement(stmt);
        }

        last_type
    }

    /// Get bytecode
    pub fn bytecode(&self) -> &[u8] {
        self.stmt_compiler.bytecode()
    }

    /// Get max stack depth
    pub fn max_stack(&mut self) -> BytecodeResult<u16> {
        self.stmt_compiler
            .expr_compiler()
            .stack_depth()
            .max_depth()
            .map_err(|error| BytecodeError::InternalError {
                message: format!("Groovy operand-stack accounting failed: {error}"),
            })
    }

    /// Return `null` from the current reference-returning method.
    pub(crate) fn compile_null_return(&mut self) {
        let compiler = self.stmt_compiler.expr_compiler();
        compiler.emit_byte(0x01); // ACONST_NULL
        compiler.stack_depth().push1();
        compiler.emit_byte(0xB0); // ARETURN
        compiler.stack_depth().pop1();
    }

    /// Get max locals
    pub fn max_locals(&mut self) -> u16 {
        self.next_local.max(1) // At least 1 for 'this'
    }

    /// Get mutable access to the statement compiler
    pub fn stmt_compiler_mut(&mut self) -> &mut GroovyStatementCompiler {
        &mut self.stmt_compiler
    }

    /// Get mutable access to the constant pool
    pub fn constant_pool_mut(&mut self) -> &mut ConstantPoolBuilder {
        self.stmt_compiler.expr_compiler().constant_pool()
    }

    /// Consume the compiler and return the constant pool
    pub fn into_constant_pool(self) -> ConstantPoolBuilder {
        self.stmt_compiler.into_constant_pool()
    }

    /// Get closure classes that were generated during compilation
    /// Returns a HashMap of (class_name -> bytecode)
    pub fn closure_classes(&self) -> &std::collections::HashMap<String, Vec<u8>> {
        &self.closure_classes
    }

    /// Get mutable access to closure classes
    pub fn closure_classes_mut(&mut self) -> &mut std::collections::HashMap<String, Vec<u8>> {
        &mut self.closure_classes
    }

    /// Get exception handlers for building exception table
    pub fn exception_handlers(&self) -> &[ExceptionHandler] {
        &self.exception_handlers
    }
}

pub(super) fn is_null_expression(expression: &Expression) -> bool {
    match expression {
        Expression::NullLiteral => true,
        Expression::Parenthesized(inner) => is_null_expression(inner),
        _ => false,
    }
}

#[cfg(test)]
#[path = "../groovy_ast_compiler_tests.rs"]
mod tests;
