// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0
//! Java bytecode backend.
//!
//! Turns a [`ClassDecl`] into a real, multi-method `.class` file with correct
//! control flow. This replaces the earlier prototype that flattened everything
//! into a hardcoded `main` and emitted placeholder (always-zero) branch offsets.
//!
//! Design notes:
//! - [`CodeBuilder`] owns a code buffer plus a forward-reference label table.
//!   Branches record a fixup site; offsets are patched in [`CodeBuilder::finish`].
//! - Conditions are compiled in *branch context* (`branch_if_false` /
//!   `branch_if_true`) so `if`/`while`/`for`/`&&`/`||`/`!` produce minimal,
//!   correct jumps. A boolean *value* is materialised on demand via 0/1 + goto.
//! - A small type inference pass picks the right opcode family (int vs long) and
//!   return instruction without a full type checker.

mod code_builder;
mod stdlib;

use crate::constant_pool::ConstantPoolBuilder;
use crate::groovy_producer::checked_u16_count;
use crate::groovy_producer::write_constant_pool_entry;
use crate::java_compiler::{
    BinaryOp, ClassDecl, Expression, FunctionalTarget, JavaType, LambdaBody, MethodDecl, Param,
    Statement, UnaryOp,
};
use crate::producer::{BytecodeError, BytecodeResult};
use code_builder::{CodeBuilder, ExceptionEntry, Label};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use stdlib::{
    is_functional_sam, known_static_method_reference, qualify_runtime_class,
    static_reference_uses_primitive_params, stdlib_instance_arity_is_valid,
    stdlib_instance_return_type, stdlib_static_arity_is_valid, stdlib_static_field_type,
    stdlib_static_param_types, stdlib_static_return_type, value_descriptor,
};

/// Process-unique counter for naming synthetic lambda closure classes.
static LAMBDA_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// ── Opcodes ─────────────────────────────────────────────────────────────────
const ICONST_M1: u8 = 0x02;
const BIPUSH: u8 = 0x10;
const SIPUSH: u8 = 0x11;
const LDC: u8 = 0x12;
const LDC_W: u8 = 0x13;
const LDC2_W: u8 = 0x14;
const ILOAD: u8 = 0x15;
const LLOAD: u8 = 0x16;
const FLOAD: u8 = 0x17;
const DLOAD: u8 = 0x18;
const ALOAD: u8 = 0x19;
const ISTORE: u8 = 0x36;
const LSTORE: u8 = 0x37;
const FSTORE: u8 = 0x38;
const DSTORE: u8 = 0x39;
const ASTORE: u8 = 0x3a;
const POP: u8 = 0x57;
const POP2: u8 = 0x58;
const DUP: u8 = 0x59;
const DUP2: u8 = 0x5c;
const I2L: u8 = 0x85;
const LCMP: u8 = 0x94;
const IFEQ: u8 = 0x99;
const IFNE: u8 = 0x9a;
const IF_ICMPEQ: u8 = 0x9f;
const IF_ICMPNE: u8 = 0xa0;
const IF_ICMPLT: u8 = 0xa1;
const IF_ICMPGE: u8 = 0xa2;
const IF_ICMPGT: u8 = 0xa3;
const IF_ICMPLE: u8 = 0xa4;
const GOTO: u8 = 0xa7;
const IRETURN: u8 = 0xac;
const LRETURN: u8 = 0xad;
const FRETURN: u8 = 0xae;
const DRETURN: u8 = 0xaf;
const ARETURN: u8 = 0xb0;
const RETURN: u8 = 0xb1;
const GETSTATIC: u8 = 0xb2;
const INVOKEVIRTUAL: u8 = 0xb6;
const INVOKESPECIAL: u8 = 0xb7;
const INVOKESTATIC: u8 = 0xb8;

/// Resolved signature of a method available for calls.
#[derive(Clone)]
struct MethodSig {
    descriptor: String,
    return_type: JavaType,
    is_static: bool,
    /// Declared parameter types, in order — used for overload resolution.
    params: Vec<JavaType>,
    /// Internal name of the class that declares this method — used as the
    /// methodref owner so an inherited call names the declaring superclass (our
    /// VM's invokevirtual does not itself walk the hierarchy).
    owner: String,
}

/// A field of the class under compilation, for resolving unqualified
/// (implicit-`this`) and `this.`-qualified field references.
#[derive(Clone)]
struct FieldInfo {
    ty: JavaType,
    is_static: bool,
    /// Internal name of the class that *declares* the field (self or a
    /// superclass), used as the fieldref owner.
    owner: String,
}

/// Per-class info for the compilation unit: superclass link, method signatures,
/// and declared fields. Lets calls/field accesses resolve across siblings and up
/// the inheritance chain.
struct ClassInfo {
    super_name: String,
    methods: HashMap<String, Vec<MethodSig>>,
    /// (name, type, is_static) for each declared field.
    fields: Vec<(String, JavaType, bool)>,
    /// Whether the class is an interface.
    is_interface: bool,
    /// Internal names of the interfaces it implements, or extends when it is an interface.
    interfaces: Vec<String>,
}

/// Every class in the unit, keyed by internal name.
type ClassRegistry = HashMap<String, ClassInfo>;

/// Per-method local variable allocation and type tracking.
struct Locals {
    /// name -> (slot, type)
    map: HashMap<String, (u16, JavaType)>,
    /// Source generic arguments for erased reference locals.
    generic_arguments: HashMap<String, Vec<JavaType>>,
    /// stack of scopes, each holding the names introduced in it
    scopes: Vec<Vec<String>>,
    next_slot: u16,
    max_slot: u16,
}

impl Locals {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            generic_arguments: HashMap::new(),
            scopes: vec![Vec::new()],
            next_slot: 0,
            max_slot: 0,
        }
    }

    fn width(ty: &JavaType) -> u16 {
        if ty.is_category2() {
            2
        } else {
            1
        }
    }

    /// Allocate a slot for a named variable (monotonic; never reuses slots).
    fn declare(&mut self, name: &str, ty: JavaType) -> u16 {
        self.declare_with_generics(name, ty, Vec::new())
    }

    fn declare_with_generics(
        &mut self,
        name: &str,
        ty: JavaType,
        generic_arguments: Vec<JavaType>,
    ) -> u16 {
        let slot = self.next_slot;
        self.next_slot += Self::width(&ty);
        self.max_slot = self.max_slot.max(self.next_slot);
        self.map.insert(name.to_string(), (slot, ty));
        if !generic_arguments.is_empty() {
            self.generic_arguments
                .insert(name.to_string(), generic_arguments);
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(name.to_string());
        }
        slot
    }

    fn lookup(&self, name: &str) -> Option<(u16, JavaType)> {
        self.map.get(name).cloned()
    }

    fn generic_arguments(&self, name: &str) -> Option<&[JavaType]> {
        self.generic_arguments.get(name).map(Vec::as_slice)
    }

    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for name in scope {
                self.map.remove(&name);
                self.generic_arguments.remove(&name);
            }
        }
    }
}

/// Compiles one method body into bytecode.
struct MethodGen<'a> {
    code: CodeBuilder,
    cp: &'a mut ConstantPoolBuilder,
    class_internal: &'a str,
    methods: &'a HashMap<String, Vec<MethodSig>>,
    /// All sibling classes' signatures, for cross-class call resolution.
    registry: &'a ClassRegistry,
    /// Instance/static fields of the class being compiled: name -> info.
    fields: &'a HashMap<String, FieldInfo>,
    locals: Locals,
    /// Source types for values held in erased Object locals (lambda params).
    logical_types: HashMap<String, JavaType>,
    return_type: JavaType,
    /// Stack of enclosing loops and switches, innermost last. `break`/`continue` branch to
    /// the top entry, and first run the finally blocks entered since their target began.
    loop_labels: Vec<JumpTargets>,
    /// The finally blocks of the try statements being compiled, outermost first. A return,
    /// break or continue out of such a try runs them, innermost first, before it jumps.
    finally_blocks: Vec<ActiveFinally>,
    /// For each try statement being compiled, outermost first, the inlined finally copies
    /// inside it. Its exception-table entries leave them out, so an exception from a
    /// finally block is not caught by the try it belongs to, or by a try inside that one.
    try_gaps: Vec<Vec<(Label, Label)>>,
    /// Collector for synthetic lambda closure classes generated while compiling
    /// this method's body. Drained by `compile_unit` and emitted as extra classes.
    lambdas: &'a RefCell<Vec<(String, Vec<u8>)>>,
}

/// A finally block of a try statement being compiled.
#[derive(Clone)]
struct ActiveFinally {
    block: Statement,
    /// The index of its try statement in `MethodGen::try_gaps`.
    try_index: usize,
}

/// Where `break` and `continue` jump inside a loop or switch, with the number of finally
/// blocks that were active when each target began.
#[derive(Clone, Copy)]
struct JumpTargets {
    break_label: Label,
    break_depth: usize,
    continue_label: Label,
    continue_depth: usize,
}

impl JumpTargets {
    /// The targets of a loop, where `break` and `continue` leave the same finally blocks.
    fn new(break_label: Label, continue_label: Label, depth: usize) -> Self {
        Self {
            break_label,
            break_depth: depth,
            continue_label,
            continue_depth: depth,
        }
    }
}

impl<'a> MethodGen<'a> {
    fn unboxed_wrapper_type(ty: &JavaType) -> Option<JavaType> {
        let JavaType::Object(name) = ty else {
            return None;
        };
        match name.as_str() {
            "java/lang/Byte" => Some(JavaType::Byte),
            "java/lang/Short" => Some(JavaType::Short),
            "java/lang/Integer" => Some(JavaType::Int),
            "java/lang/Long" => Some(JavaType::Long),
            "java/lang/Float" => Some(JavaType::Float),
            "java/lang/Double" => Some(JavaType::Double),
            "java/lang/Boolean" => Some(JavaType::Boolean),
            "java/lang/Character" => Some(JavaType::Char),
            _ => None,
        }
    }

    fn promote(a: &JavaType, b: &JavaType) -> JavaType {
        let left = Self::unboxed_wrapper_type(a).unwrap_or_else(|| a.clone());
        let right = Self::unboxed_wrapper_type(b).unwrap_or_else(|| b.clone());
        match (&left, &right) {
            (JavaType::Double, _) | (_, JavaType::Double) => JavaType::Double,
            (JavaType::Float, _) | (_, JavaType::Float) => JavaType::Float,
            (JavaType::Long, _) | (_, JavaType::Long) => JavaType::Long,
            _ => JavaType::Int,
        }
    }

    /// Infer an expression's type without emitting code (for opcode selection).
    fn infer(&self, e: &Expression) -> JavaType {
        match e {
            Expression::InstanceOf { .. } => JavaType::Boolean,
            Expression::IntLiteral(_) => JavaType::Int,
            Expression::CharLiteral(_) => JavaType::Char,
            Expression::LongLiteral(_) => JavaType::Long,
            Expression::FloatLiteral(_) => JavaType::Float,
            Expression::DoubleLiteral(_) => JavaType::Double,
            Expression::BooleanLiteral(_) => JavaType::Boolean,
            Expression::StringLiteral(_) => JavaType::Object("java/lang/String".to_string()),
            Expression::NullLiteral => JavaType::Object("java/lang/Object".to_string()),
            Expression::NameExpr(n) => self
                .logical_types
                .get(n)
                .cloned()
                .or_else(|| self.locals.lookup(n).map(|(_, t)| t))
                .or_else(|| self.fields.get(n).map(|f| f.ty.clone()))
                .unwrap_or(JavaType::Int),
            Expression::BinaryExpr { left, op, right } => match op {
                BinaryOp::LogicalAnd
                | BinaryOp::LogicalOr
                | BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::LessThan
                | BinaryOp::LessOrEqual
                | BinaryOp::GreaterThan
                | BinaryOp::GreaterOrEqual => JavaType::Boolean,
                BinaryOp::Add => {
                    let lt = self.infer(left);
                    let rt = self.infer(right);
                    // `+` is string concatenation only when an operand is a
                    // `String`; a non-String wrapper (`Integer + Integer`) is
                    // arithmetic with auto-unboxing (must match `binary`).
                    let is_string =
                        |t: &JavaType| matches!(t, JavaType::Object(n) if n == "java/lang/String");
                    if is_string(&lt) || is_string(&rt) {
                        JavaType::Object("java/lang/String".to_string())
                    } else {
                        Self::promote(&lt, &rt)
                    }
                }
                _ => Self::promote(&self.infer(left), &self.infer(right)),
            },
            Expression::UnaryExpr { op, operand } => match op {
                UnaryOp::LogicalNot => JavaType::Boolean,
                _ => self.infer(operand),
            },
            Expression::MethodCall {
                receiver,
                method,
                args,
            } => self.infer_method_return(receiver.as_deref(), method, args),
            Expression::Cast { target_type, .. } => target_type.clone(),
            Expression::Ternary {
                then_expr,
                else_expr,
                ..
            } => Self::join_types(&self.infer(then_expr), &self.infer(else_expr)),
            Expression::ArrayInit {
                element_type,
                elements,
            } => {
                let elem = element_type.clone().unwrap_or_else(|| {
                    elements
                        .first()
                        .map(|e| self.infer(e))
                        .unwrap_or(JavaType::Object("java/lang/Object".to_string()))
                });
                JavaType::Array {
                    element_type: Box::new(elem),
                    dimensions: 1,
                }
            }
            Expression::Assignment { value, .. } => self.infer(value),
            Expression::This => JavaType::Object(self.class_internal.to_string()),
            Expression::NewExpr { class, .. } => JavaType::Object(class.clone()),
            Expression::NewArray { element_type, .. } => JavaType::Array {
                element_type: Box::new(element_type.clone()),
                dimensions: 1,
            },
            Expression::NewMultiArray {
                element_type,
                lengths,
            } => JavaType::Array {
                element_type: Box::new(element_type.clone()),
                dimensions: lengths.len() as u8,
            },
            Expression::FieldAccess {
                receiver, field, ..
            } => {
                // Static field on a class name (`Integer.MAX_VALUE`) resolves to
                // its known type; otherwise an instance field of this class.
                if let Some(Expression::NameExpr(name)) = receiver.as_deref() {
                    if self.locals.lookup(name).is_none() && !self.fields.contains_key(name) {
                        let owner = qualify_runtime_class(name);
                        return self
                            .static_field_type(&owner, field)
                            .unwrap_or(JavaType::Int);
                    }
                }
                // Instance field: resolve against the receiver's class.
                let owner = self.receiver_class(receiver.as_deref());
                self.field_type_of(&owner, field)
            }
            Expression::ArrayAccess { array, .. } => match self.infer(array) {
                JavaType::Array {
                    element_type,
                    dimensions,
                } if dimensions > 1 => JavaType::Array {
                    element_type,
                    dimensions: dimensions - 1,
                },
                JavaType::Array { element_type, .. } => *element_type,
                _ => JavaType::Int,
            },
            Expression::MethodReference { target, .. } => target
                .as_ref()
                .map(|target| JavaType::Object(target.interface.clone()))
                .unwrap_or(JavaType::Object("java/lang/Object".to_string())),
            _ => JavaType::Int,
        }
    }

    /// Resolve a same-class method call to a single signature, accounting for
    /// overloads. Resolution order: unique name → unique arity → best type
    /// match → first same-arity candidate. Returns `None` if the name is not a
    /// method of the class under compilation (e.g. a stdlib call).
    fn resolve_method(&self, name: &str, args: &[Expression]) -> Option<MethodSig> {
        self.resolve_method_in(self.class_internal, name, args)
    }

    /// Infer the return type of a method call *without emitting code*, mirroring
    /// the resolution `method_call` performs. Critical for chained calls
    /// (`sb.append(a).append(b)`): the outer call's owner is the inferred type of
    /// the inner call, so a missing stdlib return type here would leave the
    /// methodref owner blank and dispatch wrong at runtime.
    fn infer_method_return(
        &self,
        receiver: Option<&Expression>,
        method: &str,
        args: &[Expression],
    ) -> JavaType {
        // Static call on a bare class name (`Math.abs(x)`): receiver is a name
        // that is neither a local nor a field (a field receiver is an instance
        // call on `this.field`, resolved below).
        if let Some(Expression::NameExpr(name)) = receiver {
            if self.locals.lookup(name).is_none() && !self.fields.contains_key(name) {
                let owner = qualify_runtime_class(name);
                if let Some(sig) = self.resolve_method_in(&owner, method, args) {
                    return sig.return_type;
                }
                let arg_types: Vec<JavaType> = args.iter().map(|a| self.infer(a)).collect();
                return static_jdk_return_type(&owner, method, &arg_types).unwrap_or(JavaType::Int);
            }
        }
        // Instance call: resolve in the receiver's class, then fall back to the
        // stdlib return-type table.
        if let Some(recv) = receiver {
            let owner = match self.infer(recv) {
                JavaType::Object(name) => name,
                _ => self.class_internal.to_string(),
            };
            if let Some(return_type) = self.generic_method_return(recv, &owner, method) {
                return return_type;
            }
            return self
                .resolve_method_in(&owner, method, args)
                .map(|s| s.return_type)
                .or_else(|| self.instance_return_type_fallback(&owner, method, args.len()))
                .unwrap_or(JavaType::Void);
        }
        // No receiver (implicit `this`): a method of the current class.
        self.resolve_method(method, args)
            .map(|s| s.return_type)
            .unwrap_or(JavaType::Void)
    }

    /// Recover selected source-generic method results while keeping the call's
    /// bytecode descriptor erased. The emitted call is followed by CHECKCAST by
    /// [`Self::method_call`], producing valid bytecode for subsequent member
    /// access instead of an invalid `Object.field` reference.
    fn generic_method_return(
        &self,
        receiver: &Expression,
        owner: &str,
        method: &str,
    ) -> Option<JavaType> {
        let Expression::NameExpr(local_name) = receiver else {
            return None;
        };
        let arguments = self.locals.generic_arguments(local_name)?;
        match (owner, method) {
            ("java/util/List" | "java/util/ArrayList", "get")
            | ("java/util/Iterator", "next")
            | ("java/util/Optional", "get") => arguments.first().cloned(),
            ("java/util/Map" | "java/util/HashMap", "get") => arguments.get(1).cloned(),
            _ => None,
        }
    }

    /// Like [`resolve_method`] but for a named class — the current class uses its
    /// own method table; a sibling uses the unit registry. Returns `None` for an
    /// unknown class (e.g. a stdlib type), where the caller falls back to an
    /// inferred descriptor.
    /// The return type of an instance method the unit does not declare: the stdlib table, or a
    /// method `java.lang.Enum` declares, called on a unit enum.
    /// The JDK class that declares `method` for a receiver of class `owner`: `owner` itself when it
    /// is a JDK class that declares it, else the first JDK ancestor, past the unit's own classes,
    /// whose stub declares a method of that name and arity. `None` when none does.
    fn jdk_declaring_class(&self, owner: &str, method: &str, arity: usize) -> Option<String> {
        let jdk = crate::interpreter::JdkStubRegistry::new();
        let mut current = owner.to_string();
        for _ in 0..64 {
            if let Some(info) = self.registry.get(&current) {
                current = info.super_name.clone();
                continue;
            }
            let stub = jdk.get_class(&current)?;
            let declares = stub.methods.values().any(|m| {
                m.name == method
                    && !m.is_static
                    && !m.is_constructor
                    && parse_param_types(&m.descriptor).len() == arity
            });
            if declares {
                return Some(current);
            }
            current = stub.superclass.clone()?;
        }
        None
    }

    fn instance_return_type_fallback(
        &self,
        owner: &str,
        method: &str,
        arity: usize,
    ) -> Option<JavaType> {
        stdlib_instance_return_type(owner, method)
            .or_else(|| {
                let extends_enum = self
                    .registry
                    .get(owner)
                    .is_some_and(|info| info.super_name == "java/lang/Enum");
                if !extends_enum {
                    return None;
                }
                match method {
                    "name" | "toString" => Some(JavaType::Object("java/lang/String".to_string())),
                    "ordinal" => Some(JavaType::Int),
                    _ => None,
                }
            })
            .or_else(|| {
                let declaring = self.jdk_declaring_class(owner, method, arity)?;
                jdk_stub_return_type(&declaring, method, arity)
            })
    }

    /// The overloads of `name` that a unit class has. The class's own, then those of its nearest
    /// superclass in the unit. Failing that, those of the first superinterface, nearest first, that
    /// declares it: a default method is inherited from an interface.
    fn overloads_in_unit(&self, class: &str, name: &str) -> Option<Vec<MethodSig>> {
        let mut seen = std::collections::HashSet::new();
        let mut interfaces = std::collections::VecDeque::new();
        let mut cursor = class.to_string();
        loop {
            if !seen.insert(cursor.clone()) {
                break;
            }
            let Some(info) = self.registry.get(&cursor) else {
                break;
            };
            if let Some(overloads) = info.methods.get(name) {
                return Some(overloads.clone());
            }
            interfaces.extend(info.interfaces.iter().cloned());
            if info.super_name == cursor {
                break;
            }
            cursor = info.super_name.clone();
        }
        while let Some(interface) = interfaces.pop_front() {
            if !seen.insert(interface.clone()) {
                continue;
            }
            let Some(info) = self.registry.get(&interface) else {
                continue;
            };
            if let Some(overloads) = info.methods.get(name) {
                return Some(overloads.clone());
            }
            interfaces.extend(info.interfaces.iter().cloned());
        }
        None
    }

    fn resolve_method_in(&self, class: &str, name: &str, args: &[Expression]) -> Option<MethodSig> {
        // The current class uses its own table; any class (self included) also
        // walks its superclass chain via the registry for inherited methods.
        let candidates: Vec<MethodSig> =
            if class == self.class_internal && self.methods.contains_key(name) {
                self.methods.get(name)?.clone()
            } else {
                self.overloads_in_unit(class, name)?
            };
        let by_arity: Vec<&MethodSig> = candidates
            .iter()
            .filter(|s| s.params.len() == args.len())
            .collect();
        match by_arity.as_slice() {
            [] => None,
            [only] => Some((*only).clone()),
            many => {
                // Multiple same-arity overloads: prefer the one whose declared
                // parameter types accept the inferred argument types exactly,
                // else the first arity match (a deterministic fallback).
                let arg_types: Vec<JavaType> = args.iter().map(|a| self.infer(a)).collect();
                many.iter()
                    .find(|s| {
                        s.params
                            .iter()
                            .zip(&arg_types)
                            .all(|(p, a)| p == a || p.is_assignable_from(a))
                    })
                    .or(many.first())
                    .map(|s| (*s).clone())
            }
        }
    }

    /// Whether `class` (including its in-unit superclass chain) declares at
    /// least one method named `name`. This distinguishes an unknown external
    /// method, whose descriptor still needs inference, from a known method with
    /// no arity-compatible overload, which is a source type error.
    fn has_method_in(&self, class: &str, name: &str) -> bool {
        if class == self.class_internal && self.methods.contains_key(name) {
            return true;
        }
        let mut cursor = class.to_string();
        loop {
            let Some(info) = self.registry.get(&cursor) else {
                return false;
            };
            if info.methods.contains_key(name) {
                return true;
            }
            if info.super_name == cursor {
                return false;
            }
            cursor = info.super_name.clone();
        }
    }

    fn arity_error(class: &str, method: &str, actual: usize) -> BytecodeError {
        BytecodeError::TypeError {
            message: format!(
                "no overload of {}.{} accepts {actual} argument(s)",
                class.replace('/', "."),
                method
            ),
        }
    }

    fn ensure_argument_value(ty: &JavaType) -> BytecodeResult<()> {
        if matches!(ty, JavaType::Void) {
            Err(BytecodeError::TypeError {
                message: "void expression cannot be used as a method argument".to_string(),
            })
        } else {
            Ok(())
        }
    }

    /// Compile an expression, leaving its value on the operand stack.
    fn expr(&mut self, e: &Expression) -> BytecodeResult<JavaType> {
        match e {
            Expression::IntLiteral(v) => {
                self.push_int(*v);
                Ok(JavaType::Int)
            }
            Expression::CharLiteral(v) => {
                // A char is an int on the operand stack; only its static type
                // (Char) differs, which drives descriptor/coercion decisions.
                self.push_int(*v);
                Ok(JavaType::Char)
            }
            Expression::BooleanLiteral(b) => {
                self.push_int(if *b { 1 } else { 0 });
                Ok(JavaType::Boolean)
            }
            Expression::NullLiteral => {
                self.code.op(0x01); // aconst_null
                self.code.push(1);
                Ok(JavaType::Object("java/lang/Object".to_string()))
            }
            Expression::LongLiteral(v) => {
                self.push_long(*v);
                Ok(JavaType::Long)
            }
            Expression::FloatLiteral(v) => {
                self.push_float(*v);
                Ok(JavaType::Float)
            }
            Expression::DoubleLiteral(v) => {
                self.push_double(*v);
                Ok(JavaType::Double)
            }
            Expression::StringLiteral(s) => {
                let idx = self.cp.add_string(s);
                self.ldc(idx);
                self.code.push(1);
                Ok(JavaType::Object("java/lang/String".to_string()))
            }
            Expression::NameExpr(name) => self.load_name(name),
            Expression::BinaryExpr { left, op, right } => self.binary(left, *op, right),
            Expression::UnaryExpr { op, operand } => self.unary(*op, operand),
            Expression::MethodCall {
                receiver,
                method,
                args,
            } => self.method_call(receiver.as_deref(), method, args, true),
            Expression::Assignment { target, value } => self.assign(target, value, true),
            Expression::InstanceOf { value, class } => {
                self.expr(value)?;
                let idx = self.cp.add_class(class);
                self.code.op(0xc1); // instanceof
                self.code.u16(idx);
                self.code.pop(1);
                self.code.push(1);
                Ok(JavaType::Boolean)
            }
            Expression::Cast { target_type, expr } => {
                let from = self.expr(expr)?;
                self.coerce(&from, target_type);
                Ok(target_type.clone())
            }
            Expression::This => {
                self.code.op(ALOAD);
                self.code.u8(0);
                self.code.push(1);
                Ok(JavaType::Object(self.class_internal.to_string()))
            }
            Expression::FieldAccess {
                receiver, field, ..
            } => self.field_load(receiver.as_deref(), field),
            Expression::NewExpr { class, args } => self.new_object(class, args),
            Expression::NewArray {
                element_type,
                length,
            } => self.new_array(element_type, length),
            Expression::NewMultiArray {
                element_type,
                lengths,
            } => self.new_multi_array(element_type, lengths),
            Expression::ArrayAccess { array, index } => self.array_load(array, index),
            Expression::Ternary {
                condition,
                then_expr,
                else_expr,
            } => self.ternary(condition, then_expr, else_expr),
            Expression::ArrayInit {
                element_type,
                elements,
            } => self.array_init(element_type.as_ref(), elements),
            Expression::Lambda {
                params,
                body,
                target,
            } => self.compile_lambda(params, body, target.as_ref()),
            Expression::MethodReference {
                qualifier,
                method,
                is_constructor,
                target,
            } => self.compile_method_reference(qualifier, method, *is_constructor, target.as_ref()),
        }
    }

    fn compile_method_reference(
        &mut self,
        qualifier: &Expression,
        method: &str,
        is_constructor: bool,
        target: Option<&FunctionalTarget>,
    ) -> BytecodeResult<JavaType> {
        let target = target.ok_or_else(|| BytecodeError::UnsupportedFeature {
            feature: "method reference without a target functional type".to_string(),
        })?;
        let params: Vec<String> = (0..target.parameter_types.len())
            .map(|index| format!("$method_ref${index}"))
            .collect();
        let param_exprs: Vec<Expression> = params
            .iter()
            .map(|name| Expression::NameExpr(name.clone()))
            .collect();

        let body = if is_constructor {
            let Expression::NameExpr(class) = qualifier else {
                return Err(BytecodeError::UnsupportedFeature {
                    feature: "non-type constructor reference".to_string(),
                });
            };
            Expression::NewExpr {
                class: qualify_runtime_class(class),
                args: param_exprs,
            }
        } else {
            let bound_receiver = match qualifier {
                Expression::NameExpr(name) => {
                    self.locals.lookup(name).is_some() || self.fields.contains_key(name)
                }
                _ => true,
            };
            if bound_receiver {
                Expression::MethodCall {
                    receiver: Some(Box::new(qualifier.clone())),
                    method: method.to_string(),
                    args: param_exprs,
                }
            } else {
                let Expression::NameExpr(class) = qualifier else {
                    unreachable!("unbound qualifier must be a name")
                };
                let owner = qualify_runtime_class(class);
                let is_static = self
                    .resolve_method_in(&owner, method, &param_exprs)
                    .is_some_and(|signature| signature.is_static)
                    || known_static_method_reference(&owner, method, &target.parameter_types);
                if is_static {
                    let args = if static_reference_uses_primitive_params(&owner) {
                        param_exprs
                            .into_iter()
                            .zip(&target.parameter_types)
                            .map(|(expr, ty)| {
                                Self::unboxed_wrapper_type(ty).map_or(expr.clone(), |primitive| {
                                    Expression::Cast {
                                        target_type: primitive,
                                        expr: Box::new(expr),
                                    }
                                })
                            })
                            .collect()
                    } else {
                        param_exprs
                    };
                    Expression::MethodCall {
                        receiver: Some(Box::new(qualifier.clone())),
                        method: method.to_string(),
                        args,
                    }
                } else {
                    let (receiver, args) = param_exprs.split_first().ok_or_else(|| {
                        BytecodeError::UnsupportedFeature {
                            feature: format!("unbound method reference {class}::{method}"),
                        }
                    })?;
                    Expression::MethodCall {
                        receiver: Some(Box::new(receiver.clone())),
                        method: method.to_string(),
                        args: args.to_vec(),
                    }
                }
            }
        };

        self.compile_lambda(
            &params,
            &LambdaBody::Expression(Box::new(body)),
            Some(target),
        )
    }

    /// Compile a Java lambda to a synthetic closure object.
    ///
    /// The lambda lowers to a class `<Enclosing>$$Lambda$N` extending
    /// `groovy/lang/Closure` with a java-compiled `doCall(Object…)Object` body;
    /// captured locals become constructor-set fields. At this site we emit the
    /// instantiation (`NEW`/`DUP`/owner+thisObject nulls/captures/`<init>`). The
    /// functional-interface SAM call is routed to `doCall` by the interpreter.
    fn compile_lambda(
        &mut self,
        params: &[String],
        body: &LambdaBody,
        target: Option<&FunctionalTarget>,
    ) -> BytecodeResult<JavaType> {
        use std::sync::atomic::Ordering;

        // Capture analysis: free names in the body that name an enclosing local
        // (and are not lambda parameters).
        let mut free = std::collections::HashSet::new();
        collect_free_names_in_lambda_body(body, &mut free);
        let param_set: std::collections::HashSet<&str> =
            params.iter().map(String::as_str).collect();
        let mut cap_names: Vec<String> = free
            .into_iter()
            .filter(|n| !param_set.contains(n.as_str()) && n != "this")
            .filter(|n| self.locals.lookup(n).is_some())
            .collect();
        cap_names.sort(); // deterministic field/arg order
        let captures: Vec<(String, JavaType)> = cap_names
            .into_iter()
            .map(|n| {
                let ty = self.locals.lookup(&n).expect("checked").1;
                (n, ty)
            })
            .collect();

        let id = LAMBDA_COUNTER.fetch_add(1, Ordering::SeqCst);
        let class_name = format!("{}$$Lambda${id}", self.class_internal);

        let bytes =
            build_lambda_class(&class_name, &captures, params, body, target, self.registry)?;
        self.lambdas.borrow_mut().push((class_name.clone(), bytes));

        // Instantiate: NEW; DUP; ACONST_NULL (owner); ACONST_NULL (thisObject);
        // <captured values>; INVOKESPECIAL <init>.
        let class_idx = self.cp.add_class(&class_name);
        self.code.op(0xBB); // NEW
        self.code.u16(class_idx);
        self.code.push(1);
        self.code.op(DUP);
        self.code.push(1);
        self.code.op(0x01); // ACONST_NULL (owner)
        self.code.push(1);
        self.code.op(0x01); // ACONST_NULL (thisObject)
        self.code.push(1);

        let mut init_desc = String::from("(Ljava/lang/Object;Ljava/lang/Object;");
        for (name, ty) in &captures {
            self.load_name(name)?; // pushes the captured value (tracks its own stack)
            init_desc.push_str(&ty.descriptor());
        }
        init_desc.push_str(")V");
        let init_ref = self.cp.add_method_ref(&class_name, "<init>", &init_desc);
        self.code.op(INVOKESPECIAL);
        self.code.u16(init_ref);
        // <init> consumes objectref + owner + thisObject + N captures; the DUP'd
        // reference remains as the lambda value.
        self.code.pop(3 + captures.len() as i32);

        Ok(JavaType::Object(class_name))
    }

    /// Resolve the owning class internal name of a field access receiver.
    fn receiver_class(&self, receiver: Option<&Expression>) -> String {
        match receiver.map(|r| self.infer(r)) {
            Some(JavaType::Object(name)) => name,
            _ => self.class_internal.to_string(),
        }
    }

    /// The declared type of `field` on class `owner` (self, or a sibling/super
    /// resolved through the unit registry, walking the superclass chain).
    /// Defaults to int when unknown.
    fn field_type_of(&self, owner: &str, field: &str) -> JavaType {
        self.resolve_instance_field_info(owner, field)
            .map(|(_, field_type)| field_type)
            .unwrap_or(JavaType::Int)
    }

    fn resolve_instance_field_info(&self, owner: &str, field: &str) -> Option<(String, JavaType)> {
        let mut cursor = owner.to_string();
        let mut visited = std::collections::HashSet::new();
        while visited.insert(cursor.clone()) {
            let info = self.registry.get(&cursor)?;
            if let Some((_, ty, false)) = info.fields.iter().find(|(n, _, _)| n == field) {
                return Some((cursor, ty.clone()));
            }
            if info.super_name == cursor {
                break;
            }
            cursor = info.super_name.clone();
        }
        None
    }

    /// The type of a static field `owner.field` — a sibling class's declared
    /// static field (from the unit registry) or a known stdlib constant.
    fn static_field_type(&self, owner: &str, field: &str) -> Option<JavaType> {
        if let Some(info) = self.registry.get(owner) {
            if let Some((_, ty, true)) = info.fields.iter().find(|(n, _, st)| n == field && *st) {
                return Some(ty.clone());
            }
        }
        stdlib_static_field_type(owner, field)
    }

    /// Load an instance field: `receiver.field` → getfield.
    fn field_load(
        &mut self,
        receiver: Option<&Expression>,
        field: &str,
    ) -> BytecodeResult<JavaType> {
        // `array.length` → the arraylength opcode.
        if field == "length" {
            if let Some(r) = receiver {
                if matches!(self.infer(r), JavaType::Array { .. }) {
                    self.expr(r)?;
                    self.code.op(0xbe); // arraylength
                                        // arrayref consumed, int length pushed (net 0).
                    return Ok(JavaType::Int);
                }
            }
        }
        // Static field access on a class name, e.g. `Integer.MAX_VALUE` or a
        // sibling class's static field — the receiver is a bare name that is
        // not a local variable or a field of the current class.
        if let Some(Expression::NameExpr(name)) = receiver {
            if self.locals.lookup(name).is_none() && !self.fields.contains_key(name) {
                let owner = qualify_runtime_class(name);
                let ty = self
                    .static_field_type(&owner, field)
                    .unwrap_or(JavaType::Int);
                let idx = self.cp.add_field_ref(&owner, field, &ty.descriptor());
                self.code.op(GETSTATIC);
                self.code.u16(idx);
                self.code.push(ret_width(&ty));
                return Ok(ty);
            }
        }
        let receiver_owner = self.receiver_class(receiver);
        let resolved = self.resolve_instance_field_info(&receiver_owner, field);
        // A `length` that names no field of the receiver's class is an array's length, when
        // the receiver's type is not known (a library method's array result, for example). The
        // arraylength opcode then fails at run time for a receiver that is not an array.
        if resolved.is_none() && field == "length" {
            if let Some(r) = receiver {
                self.expr(r)?;
                self.code.op(0xbe); // arraylength
                return Ok(JavaType::Int);
            }
        }
        let (field_owner, ftype) = resolved.ok_or_else(|| BytecodeError::TypeError {
            message: format!(
                "cannot resolve instance field {}.{}",
                receiver_owner.replace('/', "."),
                field
            ),
        })?;
        // Push the object reference.
        match receiver {
            Some(r) => {
                self.expr(r)?;
            }
            None => {
                self.code.op(ALOAD);
                self.code.u8(0);
                self.code.push(1);
            }
        }
        let idx = self
            .cp
            .add_field_ref(&field_owner, field, &ftype.descriptor());
        self.code.op(0xb4); // getfield
        self.code.u16(idx);
        // Pop objectref, push field value (width-aware).
        self.code.pop(1);
        self.code.push(ret_width(&ftype));
        Ok(ftype)
    }

    /// `new C(args)` → new; dup; <args>; invokespecial C.<init>.
    fn new_object(&mut self, class: &str, args: &[Expression]) -> BytecodeResult<JavaType> {
        let class_idx = self.cp.add_class(class);
        self.code.op(0xbb); // new
        self.code.u16(class_idx);
        self.code.push(1);
        self.code.op(DUP);
        self.code.push(1);

        // Resolve the constructor descriptor from the target class's method table
        // (self or a sibling/unit class via the registry), so arguments coerce to
        // the declared parameter types — e.g. `new Vector2(0, 0)` widens its int
        // literals to the `(DD)V` constructor. Falls back to an arity/arg-typed
        // descriptor for unknown (stdlib) classes.
        let descriptor = self
            .resolve_method_in(class, "<init>", args)
            .map(|s| s.descriptor)
            .map_or_else(
                || {
                    if self.has_method_in(class, "<init>") {
                        return Err(Self::arity_error(class, "<init>", args.len()));
                    }
                    let mut d = String::from("(");
                    for a in args {
                        d.push_str(&value_descriptor(&self.infer(a)));
                    }
                    d.push_str(")V");
                    Ok(d)
                },
                Ok,
            )?;
        let param_types = parse_param_types(&descriptor);
        for (i, a) in args.iter().enumerate() {
            let at = self.expr(a)?;
            Self::ensure_argument_value(&at)?;
            if let Some(pt) = param_types.get(i) {
                self.coerce(&at, pt);
            }
        }
        let mref = self.cp.add_method_ref(class, "<init>", &descriptor);
        self.code.op(INVOKESPECIAL);
        self.code.u16(mref);
        // invokespecial consumes the dup'd objectref + args; the original remains.
        self.code.pop(1);
        for pt in &param_types {
            self.code.pop(Locals::width(pt) as i32);
        }
        Ok(JavaType::Object(class.to_string()))
    }

    /// Emit an explicit constructor invocation — `this(args)` / `super(args)` —
    /// against the already-constructed `this` in local 0:
    /// `aload_0; <args>; invokespecial Owner.<init>(desc)`.
    ///
    /// `this(...)` targets this class's `<init>` (resolved through the method
    /// table so the right overload's descriptor is used, and args coerce to its
    /// declared parameter types). `super(...)` targets the superclass `<init>`,
    /// resolved through the unit registry when the super is an in-unit class,
    /// else falling back to a descriptor inferred from the argument types.
    fn explicit_ctor_call(&mut self, is_super: bool, args: &[Expression]) -> BytecodeResult<()> {
        let owner = if is_super {
            self.registry
                .get(self.class_internal)
                .map(|i| i.super_name.clone())
                .unwrap_or_else(|| "java/lang/Object".to_string())
        } else {
            self.class_internal.to_string()
        };

        self.code.op(0x2a); // aload_0 (the receiver to initialize)
        self.code.push(1);

        let descriptor = self
            .resolve_method_in(&owner, "<init>", args)
            .map(|s| s.descriptor)
            .map_or_else(
                || {
                    if self.has_method_in(&owner, "<init>") {
                        return Err(Self::arity_error(&owner, "<init>", args.len()));
                    }
                    let mut d = String::from("(");
                    for a in args {
                        d.push_str(&value_descriptor(&self.infer(a)));
                    }
                    d.push_str(")V");
                    Ok(d)
                },
                Ok,
            )?;
        let param_types = parse_param_types(&descriptor);
        for (i, a) in args.iter().enumerate() {
            let at = self.expr(a)?;
            Self::ensure_argument_value(&at)?;
            if let Some(pt) = param_types.get(i) {
                self.coerce(&at, pt);
            }
        }
        let mref = self.cp.add_method_ref(&owner, "<init>", &descriptor);
        self.code.op(INVOKESPECIAL);
        self.code.u16(mref);
        // invokespecial consumes the receiver + all argument slots.
        self.code.pop(1);
        for pt in &param_types {
            self.code.pop(Locals::width(pt) as i32);
        }
        Ok(())
    }

    /// Desugar an enhanced-for into ordinary statements, choosing the lowering
    /// from the iterable's inferred type:
    ///   array → `{ T[] $a = it; for (int $i=0; $i<$a.length; $i++) { T x=$a[$i]; body } }`
    ///   collection → `{ Iterator $it = it.iterator(); while ($it.hasNext()) { T x=$it.next(); body } }`
    fn lower_foreach(
        &self,
        elem_type: &JavaType,
        var_name: &str,
        iterable: &Expression,
        body: &Statement,
    ) -> Statement {
        if let JavaType::Array { .. } = self.infer(iterable) {
            let arr = fresh_local("fe_arr");
            let idx = fresh_local("fe_i");
            let decl_arr = Statement::VariableDeclaration {
                name: arr.clone(),
                var_type: JavaType::Array {
                    element_type: Box::new(elem_type.clone()),
                    dimensions: 1,
                },
                generic_arguments: Vec::new(),
                initializer: Some(iterable.clone()),
            };
            let decl_idx = Statement::VariableDeclaration {
                name: idx.clone(),
                var_type: JavaType::Int,
                generic_arguments: Vec::new(),
                initializer: Some(Expression::IntLiteral(0)),
            };
            let cond = Expression::BinaryExpr {
                left: Box::new(Expression::NameExpr(idx.clone())),
                op: BinaryOp::LessThan,
                right: Box::new(Expression::FieldAccess {
                    receiver: Some(Box::new(Expression::NameExpr(arr.clone()))),
                    field: "length".to_string(),
                    class: String::new(),
                }),
            };
            let update = Expression::Assignment {
                target: Box::new(Expression::NameExpr(idx.clone())),
                value: Box::new(Expression::BinaryExpr {
                    left: Box::new(Expression::NameExpr(idx.clone())),
                    op: BinaryOp::Add,
                    right: Box::new(Expression::IntLiteral(1)),
                }),
            };
            let bind = Statement::VariableDeclaration {
                name: var_name.to_string(),
                var_type: elem_type.clone(),
                generic_arguments: Vec::new(),
                initializer: Some(Expression::ArrayAccess {
                    array: Box::new(Expression::NameExpr(arr)),
                    index: Box::new(Expression::NameExpr(idx)),
                }),
            };
            return Statement::Block(vec![
                decl_arr,
                Statement::For {
                    init: Some(Box::new(decl_idx)),
                    condition: Some(cond),
                    update: Some(update),
                    body: Box::new(Statement::Block(vec![bind, body.clone()])),
                },
            ]);
        }

        // Collection / Iterable: iterate via `iterator()`/`hasNext()`/`next()`.
        let it = fresh_local("fe_it");
        let decl_it = Statement::VariableDeclaration {
            name: it.clone(),
            var_type: JavaType::Object("java/util/Iterator".to_string()),
            generic_arguments: vec![elem_type.clone()],
            initializer: Some(Expression::MethodCall {
                receiver: Some(Box::new(iterable.clone())),
                method: "iterator".to_string(),
                args: Vec::new(),
            }),
        };
        let cond = Expression::MethodCall {
            receiver: Some(Box::new(Expression::NameExpr(it.clone()))),
            method: "hasNext".to_string(),
            args: Vec::new(),
        };
        let bind = Statement::VariableDeclaration {
            name: var_name.to_string(),
            var_type: elem_type.clone(),
            generic_arguments: Vec::new(),
            initializer: Some(Expression::MethodCall {
                receiver: Some(Box::new(Expression::NameExpr(it))),
                method: "next".to_string(),
                args: Vec::new(),
            }),
        };
        Statement::Block(vec![
            decl_it,
            Statement::While {
                condition: cond,
                body: Box::new(Statement::Block(vec![bind, body.clone()])),
            },
        ])
    }

    /// `new T[len]` → push len; newarray/anewarray.
    fn new_array(
        &mut self,
        element_type: &JavaType,
        length: &Expression,
    ) -> BytecodeResult<JavaType> {
        let lt = self.expr(length)?;
        self.coerce(&lt, &JavaType::Int);
        self.emit_array_create(element_type);
        // length consumed, arrayref produced.
        Ok(JavaType::Array {
            element_type: Box::new(element_type.clone()),
            dimensions: 1,
        })
    }

    /// `new T[a][b]…` → push each dimension length, then `multianewarray` of the
    /// full array type with the dimension count.
    fn new_multi_array(
        &mut self,
        element_type: &JavaType,
        lengths: &[Expression],
    ) -> BytecodeResult<JavaType> {
        let dims = lengths.len() as u8;
        for len in lengths {
            let lt = self.expr(len)?;
            self.coerce(&lt, &JavaType::Int);
        }
        let array_type = JavaType::Array {
            element_type: Box::new(element_type.clone()),
            dimensions: dims,
        };
        let idx = self.cp.add_class(&array_type.descriptor());
        self.code.op(0xc5); // multianewarray
        self.code.u16(idx);
        self.code.u8(dims);
        // Consumes `dims` int sizes, produces one arrayref.
        self.code.pop(dims as i32);
        self.code.push(1);
        Ok(array_type)
    }

    /// Emit the array-creation opcode for `element_type`, assuming the length is
    /// already on the operand stack (consumed, arrayref produced — net stack 0).
    fn emit_array_create(&mut self, element_type: &JavaType) {
        match element_type {
            JavaType::Object(name) => {
                let idx = self.cp.add_class(name);
                self.code.op(0xbd); // anewarray
                self.code.u16(idx);
            }
            JavaType::Array { .. } => {
                // Array of arrays — element is itself a reference type.
                let idx = self.cp.add_class(&element_type.descriptor());
                self.code.op(0xbd); // anewarray
                self.code.u16(idx);
            }
            prim => {
                let atype = match prim {
                    JavaType::Boolean => 4,
                    JavaType::Char => 5,
                    JavaType::Float => 6,
                    JavaType::Double => 7,
                    JavaType::Byte => 8,
                    JavaType::Short => 9,
                    JavaType::Long => 11,
                    _ => 10, // int
                };
                self.code.op(0xbc); // newarray
                self.code.u8(atype);
            }
        }
    }

    /// `{ e0, e1, ... }` → new array of the resolved element type, then store
    /// each element. Element type comes from the declared array type when known,
    /// else inferred from the first element.
    fn array_init(
        &mut self,
        element_type: Option<&JavaType>,
        elements: &[Expression],
    ) -> BytecodeResult<JavaType> {
        let elem = element_type.cloned().unwrap_or_else(|| {
            elements
                .first()
                .map(|e| self.infer(e))
                .unwrap_or(JavaType::Object("java/lang/Object".to_string()))
        });
        self.push_int(elements.len() as i32);
        self.emit_array_create(&elem);
        for (i, e) in elements.iter().enumerate() {
            self.code.op(DUP);
            self.code.push(1);
            self.push_int(i as i32);
            let vt = self.expr(e)?;
            self.coerce(&vt, &elem);
            self.code.op(array_store_op(&elem));
            self.code.pop(2 + ret_width(&elem)); // arrayref + index + value
        }
        Ok(JavaType::Array {
            element_type: Box::new(elem),
            dimensions: 1,
        })
    }

    /// `array[index]` (load) → Xaload.
    fn array_load(&mut self, array: &Expression, index: &Expression) -> BytecodeResult<JavaType> {
        let arr_ty = self.expr(array)?;
        let it = self.expr(index)?;
        self.coerce(&it, &JavaType::Int);
        let elem = array_element_type(&arr_ty);
        self.code.op(array_load_op(&elem));
        // arrayref + index consumed, element pushed.
        self.code.pop(2);
        self.code.push(ret_width(&elem));
        Ok(elem)
    }

    fn push_int(&mut self, v: i32) {
        match v {
            -1..=5 => self.code.op((0x03 + v) as u8), // iconst_<n> (iconst_m1 = 0x02)
            -128..=127 if v != -1 => {
                self.code.op(BIPUSH);
                self.code.u8(v as i8 as u8);
            }
            -32768..=32767 => {
                self.code.op(SIPUSH);
                self.code.i16(v as i16);
            }
            _ => {
                let idx = self.cp.add_integer(v);
                self.ldc(idx);
            }
        }
        if v == -1 {
            // -1 handled by iconst_m1 = 0x02; the 0x03+v arm produced 0x02 only for v=-1.
        }
        self.code.push(1);
    }

    fn push_long(&mut self, v: i64) {
        if v == 0 {
            self.code.op(0x09); // lconst_0
        } else if v == 1 {
            self.code.op(0x0a); // lconst_1
        } else {
            let idx = self.cp.add_long(v);
            self.code.op(LDC2_W);
            self.code.u16(idx);
        }
        self.code.push(2);
    }

    fn push_float(&mut self, v: f32) {
        // Compare bit patterns: exact (and correct for -0.0, which must `ldc`).
        let bits = v.to_bits();
        if bits == 0f32.to_bits() {
            self.code.op(0x0b); // fconst_0
        } else if bits == 1f32.to_bits() {
            self.code.op(0x0c); // fconst_1
        } else if bits == 2f32.to_bits() {
            self.code.op(0x0d); // fconst_2
        } else {
            let idx = self.cp.add_float(v);
            self.ldc(idx);
        }
        self.code.push(1);
    }

    fn push_double(&mut self, v: f64) {
        let bits = v.to_bits();
        if bits == 0f64.to_bits() {
            self.code.op(0x0e); // dconst_0
        } else if bits == 1f64.to_bits() {
            self.code.op(0x0f); // dconst_1
        } else {
            let idx = self.cp.add_double(v);
            self.code.op(LDC2_W);
            self.code.u16(idx);
        }
        self.code.push(2);
    }

    fn ldc(&mut self, idx: u16) {
        if idx <= 0xff {
            self.code.op(LDC);
            self.code.u8(idx as u8);
        } else {
            self.code.op(LDC_W);
            self.code.u16(idx);
        }
    }

    /// Resolve an unqualified name: a local variable first, then a field of the
    /// class under compilation (implicit `this`). This mirrors Java's name
    /// resolution and is what makes bare `count`, `head`, `OCTAL_BASE` work
    /// without an explicit `this.`/`Class.` qualifier.
    fn load_name(&mut self, name: &str) -> BytecodeResult<JavaType> {
        if self.locals.lookup(name).is_some() {
            return self.load_local(name);
        }
        if let Some(info) = self.fields.get(name).cloned() {
            return Ok(self.load_self_field(name, &info));
        }
        Err(BytecodeError::InternalError {
            message: format!("undefined variable: {name}"),
        })
    }

    /// Load a field of the class under compilation (static → getstatic, instance
    /// → `aload_0; getfield`).
    fn load_self_field(&mut self, name: &str, info: &FieldInfo) -> JavaType {
        let owner = info.owner.clone();
        let desc = info.ty.descriptor();
        if info.is_static {
            let idx = self.cp.add_field_ref(&owner, name, &desc);
            self.code.op(GETSTATIC);
            self.code.u16(idx);
            self.code.push(ret_width(&info.ty));
        } else {
            self.code.op(ALOAD);
            self.code.u8(0);
            self.code.push(1);
            let idx = self.cp.add_field_ref(&owner, name, &desc);
            self.code.op(0xb4); // getfield
            self.code.u16(idx);
            self.code.pop(1);
            self.code.push(ret_width(&info.ty));
        }
        info.ty.clone()
    }

    /// Store to a field of the class under compilation (static → putstatic,
    /// instance → `aload_0; <value>; putfield`). When `value_used`, the value is
    /// left on the stack for a surrounding assignment expression.
    fn store_self_field(
        &mut self,
        name: &str,
        info: &FieldInfo,
        value: &Expression,
        value_used: bool,
    ) -> BytecodeResult<()> {
        let owner = info.owner.clone();
        let desc = info.ty.descriptor();
        let cat2 = info.ty.is_category2();
        if info.is_static {
            let vt = self.expr(value)?;
            self.coerce(&vt, &info.ty);
            if value_used {
                self.code.op(if cat2 { DUP2 } else { DUP });
                self.code.push(ret_width(&info.ty));
            }
            let idx = self.cp.add_field_ref(&owner, name, &desc);
            self.code.op(0xb3); // putstatic
            self.code.u16(idx);
            self.code.pop(ret_width(&info.ty));
        } else {
            self.code.op(ALOAD);
            self.code.u8(0);
            self.code.push(1);
            let vt = self.expr(value)?;
            self.coerce(&vt, &info.ty);
            if value_used {
                // Duplicate the value beneath the objectref so it survives the
                // store: dup_x1 (cat1) / dup2_x1 (cat2).
                self.code.op(if cat2 { 0x5d } else { 0x5a });
                self.code.push(ret_width(&info.ty));
            }
            let idx = self.cp.add_field_ref(&owner, name, &desc);
            self.code.op(0xb5); // putfield
            self.code.u16(idx);
            self.code.pop(1 + ret_width(&info.ty));
        }
        Ok(())
    }

    fn load_local(&mut self, name: &str) -> BytecodeResult<JavaType> {
        let (slot, ty) = self
            .locals
            .lookup(name)
            .ok_or_else(|| BytecodeError::InternalError {
                message: format!("undefined variable: {name}"),
            })?;
        let (op, width) = match ty {
            JavaType::Long => (LLOAD, 2),
            JavaType::Float => (FLOAD, 1),
            JavaType::Double => (DLOAD, 2),
            JavaType::Object(_) | JavaType::Array { .. } => (ALOAD, 1),
            _ => (ILOAD, 1),
        };
        self.code.op(op);
        self.code.u8(slot as u8);
        self.code.push(width);
        Ok(ty)
    }

    fn store_local(&mut self, slot: u16, ty: &JavaType) {
        let (op, width) = match ty {
            JavaType::Long => (LSTORE, 2),
            JavaType::Float => (FSTORE, 1),
            JavaType::Double => (DSTORE, 2),
            JavaType::Object(_) | JavaType::Array { .. } => (ASTORE, 1),
            _ => (ISTORE, 1),
        };
        self.code.op(op);
        self.code.u8(slot as u8);
        self.code.pop(width);
    }

    /// Numeric coercion between primitive types. Emits the JVM conversion
    /// opcode and adjusts the tracked stack width (int/float are category-1,
    /// long/double category-2). Int-like source types (`byte`/`short`/`char`)
    /// already sit in an int slot, so only the int→wider conversions apply.
    fn coerce(&mut self, from: &JavaType, to: &JavaType) {
        use JavaType::{Byte, Char, Double, Float, Int, Long, Short};
        // Treat all int-family types uniformly as `int` on the stack.
        let int_like = |t: &JavaType| matches!(t, Int | Short | Byte | Char);
        match (from, to) {
            // Widening from the int family.
            (f, Long) if int_like(f) => {
                self.code.op(I2L);
                self.code.push(1); // int(1) → long(2)
            }
            (f, Float) if int_like(f) => self.code.op(0x86), // i2f (width unchanged)
            (f, Double) if int_like(f) => {
                self.code.op(0x87); // i2d
                self.code.push(1); // int(1) → double(2)
            }
            // From long.
            (Long, Int | Short | Byte | Char) => {
                self.code.op(0x88); // l2i
                self.code.pop(1);
            }
            (Long, Float) => {
                self.code.op(0x89); // l2f
                self.code.pop(1); // long(2) → float(1)
            }
            (Long, Double) => self.code.op(0x8a), // l2d (width unchanged: 2→2)
            // From float.
            (Float, Int | Short | Byte | Char) => self.code.op(0x8b), // f2i
            (Float, Long) => {
                self.code.op(0x8c); // f2l
                self.code.push(1); // float(1) → long(2)
            }
            (Float, Double) => {
                self.code.op(0x8d); // f2d
                self.code.push(1); // float(1) → double(2)
            }
            // From double.
            (Double, Int | Short | Byte | Char) => {
                self.code.op(0x8e); // d2i
                self.code.pop(1); // double(2) → int(1)
            }
            (Double, Long) => self.code.op(0x8f), // d2l (width unchanged: 2→2)
            (Double, Float) => {
                self.code.op(0x90); // d2f
                self.code.pop(1); // double(2) → float(1)
            }
            // Auto-unboxing: a wrapper `Object` coerced to a primitive emits the
            // matching `xValue()` call. Used for boxed lambda params/results, and
            // generally for `Integer`→`int` style conversions. The unbox ref
            // owner follows the *target* (the runtime value is the wrapper).
            (JavaType::Object(_), t) if int_like(t) => {
                self.unbox("java/lang/Integer", "intValue", "()I", 0)
            }
            (JavaType::Object(_), Long) => self.unbox("java/lang/Long", "longValue", "()J", 1),
            (JavaType::Object(_), Float) => self.unbox("java/lang/Float", "floatValue", "()F", 0),
            (JavaType::Object(_), Double) => {
                self.unbox("java/lang/Double", "doubleValue", "()D", 1)
            }
            (JavaType::Object(_), JavaType::Boolean) => {
                self.unbox("java/lang/Boolean", "booleanValue", "()Z", 0);
            }
            _ => {}
        }
    }

    /// Emit `invokevirtual <wrapper>.<name><desc>` to unbox the reference on top
    /// of the stack to a primitive. `extra_push` is the extra stack slots the
    /// result occupies beyond the consumed reference (1 for long/double, else 0).
    fn unbox(&mut self, wrapper: &str, name: &str, desc: &str, extra_push: i32) {
        let m = self.cp.add_method_ref(wrapper, name, desc);
        self.code.op(INVOKEVIRTUAL);
        self.code.u16(m);
        if extra_push > 0 {
            self.code.push(extra_push);
        }
    }

    fn binary(
        &mut self,
        left: &Expression,
        op: BinaryOp,
        right: &Expression,
    ) -> BytecodeResult<JavaType> {
        use BinaryOp::*;

        // String concatenation: `+` where an operand is a `String`. A non-String
        // wrapper operand (`Integer + Integer`, a boxed lambda param) is arithmetic
        // with auto-unboxing, NOT concatenation.
        let is_string = |t: &JavaType| matches!(t, JavaType::Object(n) if n == "java/lang/String");
        if matches!(op, Add) && (is_string(&self.infer(left)) || is_string(&self.infer(right))) {
            return self.string_concat(left, right);
        }

        match op {
            // Comparisons and logical ops produce a boolean value (0/1).
            Equal | NotEqual | LessThan | LessOrEqual | GreaterThan | GreaterOrEqual
            | LogicalAnd | LogicalOr => {
                self.materialize_bool(&Expression::BinaryExpr {
                    left: Box::new(left.clone()),
                    op,
                    right: Box::new(right.clone()),
                })?;
                Ok(JavaType::Boolean)
            }
            // Arithmetic / bitwise / shift.
            _ => {
                let is_shift = matches!(op, LeftShift | RightShift | UnsignedRightShift);
                let lt = self.infer(left);
                let rt = self.infer(right);
                // Shifts: the result type follows the *left* operand and the
                // shift amount stays an int. Other ops promote both operands
                // to their common type (int/long/float/double).
                let result = if is_shift {
                    if matches!(lt, JavaType::Long) {
                        JavaType::Long
                    } else {
                        JavaType::Int
                    }
                } else {
                    Self::promote(&lt, &rt)
                };
                let right_target = if is_shift {
                    JavaType::Int
                } else {
                    result.clone()
                };

                let from_left = self.expr(left)?;
                self.coerce(&from_left, &result);
                let from_right = self.expr(right)?;
                self.coerce(&from_right, &right_target);

                let opcode = match (op, &result) {
                    (Add, JavaType::Long) => 0x61,
                    (Add, JavaType::Float) => 0x62,
                    (Add, JavaType::Double) => 0x63,
                    (Add, _) => 0x60,
                    (Subtract, JavaType::Long) => 0x65,
                    (Subtract, JavaType::Float) => 0x66,
                    (Subtract, JavaType::Double) => 0x67,
                    (Subtract, _) => 0x64,
                    (Multiply, JavaType::Long) => 0x69,
                    (Multiply, JavaType::Float) => 0x6a,
                    (Multiply, JavaType::Double) => 0x6b,
                    (Multiply, _) => 0x68,
                    (Divide, JavaType::Long) => 0x6d,
                    (Divide, JavaType::Float) => 0x6e,
                    (Divide, JavaType::Double) => 0x6f,
                    (Divide, _) => 0x6c,
                    (Modulo, JavaType::Long) => 0x71,
                    (Modulo, JavaType::Float) => 0x72,
                    (Modulo, JavaType::Double) => 0x73,
                    (Modulo, _) => 0x70,
                    (BitwiseAnd, JavaType::Long) => 0x7f,
                    (BitwiseAnd, _) => 0x7e,
                    (BitwiseOr, JavaType::Long) => 0x81,
                    (BitwiseOr, _) => 0x80,
                    (BitwiseXor, JavaType::Long) => 0x83,
                    (BitwiseXor, _) => 0x82,
                    (LeftShift, JavaType::Long) => 0x79,
                    (LeftShift, _) => 0x78,
                    (RightShift, JavaType::Long) => 0x7b,
                    (RightShift, _) => 0x7a,
                    (UnsignedRightShift, JavaType::Long) => 0x7d,
                    (UnsignedRightShift, _) => 0x7c,
                    _ => {
                        return Err(BytecodeError::InternalError {
                            message: format!("unsupported binary op {op:?}"),
                        })
                    }
                };
                self.code.op(opcode);
                // Shifts always consume one int amount slot (net -1); other ops
                // consume two operands and leave one of `result` width.
                if is_shift {
                    self.code.pop(1);
                } else {
                    self.code.pop(ret_width(&result));
                }
                Ok(result)
            }
        }
    }

    /// Compile `left + right` as string concatenation using StringBuilder:
    /// `new StringBuilder().append(left).append(right).toString()`.
    fn string_concat(&mut self, left: &Expression, right: &Expression) -> BytecodeResult<JavaType> {
        let sb = "java/lang/StringBuilder";
        let sb_idx = self.cp.add_class(sb);
        self.code.op(0xbb); // new
        self.code.u16(sb_idx);
        self.code.push(1);
        self.code.op(DUP);
        self.code.push(1);
        let init = self.cp.add_method_ref(sb, "<init>", "()V");
        self.code.op(INVOKESPECIAL);
        self.code.u16(init);
        self.code.pop(1); // <init> consumes the dup'd ref

        self.append(left)?;
        self.append(right)?;

        let to_string = self
            .cp
            .add_method_ref(sb, "toString", "()Ljava/lang/String;");
        self.code.op(INVOKEVIRTUAL);
        self.code.u16(to_string);
        // builder ref consumed, String pushed (net 0).
        Ok(JavaType::Object("java/lang/String".to_string()))
    }

    /// Append one operand to the StringBuilder currently on the stack.
    /// Nested `+` chains are flattened so each leaf is appended in turn.
    fn append(&mut self, e: &Expression) -> BytecodeResult<()> {
        if let Expression::BinaryExpr {
            left,
            op: BinaryOp::Add,
            right,
        } = e
        {
            if matches!(self.infer(left), JavaType::Object(_))
                || matches!(self.infer(right), JavaType::Object(_))
            {
                self.append(left)?;
                self.append(right)?;
                return Ok(());
            }
        }
        let ty = self.expr(e)?;
        let desc = match &ty {
            JavaType::Int | JavaType::Short | JavaType::Byte => "(I)Ljava/lang/StringBuilder;",
            JavaType::Long => "(J)Ljava/lang/StringBuilder;",
            JavaType::Float => "(F)Ljava/lang/StringBuilder;",
            JavaType::Double => "(D)Ljava/lang/StringBuilder;",
            JavaType::Boolean => "(Z)Ljava/lang/StringBuilder;",
            JavaType::Char => "(C)Ljava/lang/StringBuilder;",
            JavaType::Object(n) if n.ends_with("String") => {
                "(Ljava/lang/String;)Ljava/lang/StringBuilder;"
            }
            _ => "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
        };
        let mref = self
            .cp
            .add_method_ref("java/lang/StringBuilder", "append", desc);
        self.code.op(INVOKEVIRTUAL);
        self.code.u16(mref);
        // Pops builder + value, pushes builder back → net -value_width.
        self.code.pop(ret_width(&ty));
        Ok(())
    }

    fn unary(&mut self, op: UnaryOp, operand: &Expression) -> BytecodeResult<JavaType> {
        match op {
            UnaryOp::Negate => {
                let source = self.expr(operand)?;
                // Lambda parameters are represented as Object in `doCall`; use
                // retained target typing when available, otherwise follow the
                // same int default as binary promotion.
                let target =
                    Self::unboxed_wrapper_type(&self.infer(operand)).unwrap_or_else(|| {
                        if matches!(source, JavaType::Object(_)) {
                            JavaType::Int
                        } else {
                            source.clone()
                        }
                    });
                self.coerce(&source, &target);
                match target {
                    JavaType::Long => self.code.op(0x75), // lneg
                    JavaType::Float => self.code.op(0x76),
                    JavaType::Double => self.code.op(0x77),
                    _ => self.code.op(0x74), // ineg
                }
                Ok(target)
            }
            UnaryOp::BitwiseNot => {
                let source = self.expr(operand)?;
                let target =
                    Self::unboxed_wrapper_type(&self.infer(operand)).unwrap_or_else(|| {
                        if matches!(source, JavaType::Object(_)) {
                            JavaType::Int
                        } else {
                            source.clone()
                        }
                    });
                self.coerce(&source, &target);
                match target {
                    JavaType::Long => {
                        self.push_long(-1);
                        self.code.op(0x83); // lxor
                        self.code.pop(2);
                    }
                    _ => {
                        self.code.op(ICONST_M1);
                        self.code.push(1);
                        self.code.op(0x82); // ixor
                        self.code.pop(1);
                    }
                }
                Ok(target)
            }
            UnaryOp::LogicalNot => {
                self.materialize_bool(&Expression::UnaryExpr {
                    op,
                    operand: Box::new(operand.clone()),
                })?;
                Ok(JavaType::Boolean)
            }
        }
    }

    /// Materialise a boolean expression as an int 0/1 on the stack.
    fn materialize_bool(&mut self, cond: &Expression) -> BytecodeResult<()> {
        let l_false = self.code.new_label();
        let l_end = self.code.new_label();
        self.branch_if_false(cond, l_false)?;
        self.code.op(0x04); // iconst_1
        self.code.push(1);
        self.code.branch(GOTO, l_end);
        self.code.place(l_false);
        self.code.op(0x03); // iconst_0
                            // Note: only one of the two arms executes; net push is +1.
        self.code.place(l_end);
        Ok(())
    }

    /// The result type of a `?:` is the join of its two arm types: identical
    /// types pass through, mixed numerics promote, and a reference paired with
    /// `null`/another reference yields the more specific (non-`Object`) one.
    fn join_types(a: &JavaType, b: &JavaType) -> JavaType {
        if a == b {
            return a.clone();
        }
        let is_obj = |t: &JavaType| matches!(t, JavaType::Object(_) | JavaType::Array { .. });
        match (a, b) {
            (JavaType::Object(n), other) | (other, JavaType::Object(n))
                if n == "java/lang/Object" && is_obj(other) =>
            {
                other.clone()
            }
            _ if a.is_numeric() && b.is_numeric() => Self::promote(a, b),
            (o, _) if is_obj(o) => a.clone(),
            (_, o) if is_obj(o) => b.clone(),
            _ => a.clone(),
        }
    }

    /// Compile `cond ? then : els`, leaving the joined-type value on the stack.
    fn ternary(
        &mut self,
        cond: &Expression,
        then_expr: &Expression,
        else_expr: &Expression,
    ) -> BytecodeResult<JavaType> {
        let result = Self::join_types(&self.infer(then_expr), &self.infer(else_expr));
        let l_else = self.code.new_label();
        let l_end = self.code.new_label();

        self.branch_if_false(cond, l_else)?;
        // Stack height with the condition consumed; both arms start from here
        // and each leaves exactly one `result`-width value, so reset between
        // them to avoid double-counting (only one arm runs at runtime).
        let base = self.code.stack_depth();
        let t = self.expr(then_expr)?;
        self.coerce(&t, &result);
        self.code.branch(GOTO, l_end);

        self.code.restore_stack_depth(base);
        self.code.place(l_else);
        let e = self.expr(else_expr)?;
        self.coerce(&e, &result);
        self.code.place(l_end);
        Ok(result)
    }

    /// Emit code that branches to `target` iff `cond` is false.
    fn branch_if_false(&mut self, cond: &Expression, target: Label) -> BytecodeResult<()> {
        match cond {
            Expression::BinaryExpr { left, op, right } if is_comparison(*op) => {
                self.compare_branch(left, *op, right, target, false)
            }
            Expression::BinaryExpr {
                left,
                op: BinaryOp::LogicalAnd,
                right,
            } => {
                // (a && b) false  ⟺  a false  OR  b false
                self.branch_if_false(left, target)?;
                self.branch_if_false(right, target)
            }
            Expression::BinaryExpr {
                left,
                op: BinaryOp::LogicalOr,
                right,
            } => {
                // (a || b) false ⟺ a false AND b false
                let skip = self.code.new_label();
                self.branch_if_true(left, skip)?;
                self.branch_if_false(right, target)?;
                self.code.place(skip);
                Ok(())
            }
            Expression::UnaryExpr {
                op: UnaryOp::LogicalNot,
                operand,
            } => self.branch_if_true(operand, target),
            _ => {
                // Generic boolean value: branch if == 0.
                let source = self.expr(cond)?;
                self.coerce(&source, &JavaType::Boolean);
                self.code.branch(IFEQ, target);
                self.code.pop(1);
                Ok(())
            }
        }
    }

    /// Emit code that branches to `target` iff `cond` is true.
    fn branch_if_true(&mut self, cond: &Expression, target: Label) -> BytecodeResult<()> {
        match cond {
            Expression::BinaryExpr { left, op, right } if is_comparison(*op) => {
                self.compare_branch(left, *op, right, target, true)
            }
            Expression::BinaryExpr {
                left,
                op: BinaryOp::LogicalAnd,
                right,
            } => {
                let skip = self.code.new_label();
                self.branch_if_false(left, skip)?;
                self.branch_if_true(right, target)?;
                self.code.place(skip);
                Ok(())
            }
            Expression::BinaryExpr {
                left,
                op: BinaryOp::LogicalOr,
                right,
            } => {
                self.branch_if_true(left, target)?;
                self.branch_if_true(right, target)
            }
            Expression::UnaryExpr {
                op: UnaryOp::LogicalNot,
                operand,
            } => self.branch_if_false(operand, target),
            _ => {
                let source = self.expr(cond)?;
                self.coerce(&source, &JavaType::Boolean);
                self.code.branch(IFNE, target);
                self.code.pop(1);
                Ok(())
            }
        }
    }

    /// Compile `left <op> right` as a conditional branch to `target`.
    /// When `branch_when_true` is set, branches if the comparison holds;
    /// otherwise branches if it does not hold.
    fn compare_branch(
        &mut self,
        left: &Expression,
        op: BinaryOp,
        right: &Expression,
        target: Label,
        branch_when_true: bool,
    ) -> BytecodeResult<()> {
        let lt = self.infer(left);
        let rt = self.infer(right);
        let operand_ty = Self::promote(&lt, &rt);

        // Effective comparison after accounting for branch polarity.
        let effective = if branch_when_true { op } else { negate_cmp(op) };

        // Reference equality (`==`/`!=` on objects/arrays, including `null`):
        // use ifnull/ifnonnull for null comparisons, else if_acmpeq/if_acmpne.
        let is_ref = |t: &JavaType| matches!(t, JavaType::Object(_) | JavaType::Array { .. });
        if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) && (is_ref(&lt) || is_ref(&rt)) {
            let branch_eq = matches!(effective, BinaryOp::Equal);
            let left_null = matches!(left, Expression::NullLiteral);
            let right_null = matches!(right, Expression::NullLiteral);
            if left_null || right_null {
                let operand = if left_null { right } else { left };
                self.expr(operand)?;
                self.code.pop(1);
                let opcode = if branch_eq { 0xc6 } else { 0xc7 }; // ifnull / ifnonnull
                self.code.branch(opcode, target);
            } else {
                self.expr(left)?;
                self.expr(right)?;
                self.code.pop(2);
                let opcode = if branch_eq { 0xa5 } else { 0xa6 }; // if_acmpeq / if_acmpne
                self.code.branch(opcode, target);
            }
            return Ok(());
        }

        match operand_ty {
            // long/float/double: a cmp opcode leaves -1/0/1, then branch vs zero.
            JavaType::Long | JavaType::Float | JavaType::Double => {
                let from_left = self.expr(left)?;
                self.coerce(&from_left, &operand_ty);
                let from_right = self.expr(right)?;
                self.coerce(&from_right, &operand_ty);
                let operand_w = ret_width(&operand_ty);
                let cmp = match operand_ty {
                    JavaType::Long => LCMP, // 0x94
                    // For float/double pick fcmpg/dcmpg for `<`/`<=` and
                    // fcmpl/dcmpl otherwise, so a NaN operand makes the source
                    // comparison evaluate to false (JLS unordered semantics).
                    JavaType::Float => {
                        if matches!(effective, BinaryOp::LessThan | BinaryOp::LessOrEqual) {
                            0x96 // fcmpg
                        } else {
                            0x95 // fcmpl
                        }
                    }
                    _ => {
                        if matches!(effective, BinaryOp::LessThan | BinaryOp::LessOrEqual) {
                            0x98 // dcmpg
                        } else {
                            0x97 // dcmpl
                        }
                    }
                };
                self.code.op(cmp);
                self.code.pop(operand_w * 2); // two operands in
                self.code.push(1); // one int out
                self.code.pop(1); // the branch consumes the int
                self.code.branch(cmp_zero_opcode(effective), target);
            }
            // int (and, pre-existing, reference) comparison via if_icmp<cond>.
            _ => {
                let from_left = self.expr(left)?;
                self.coerce(&from_left, &operand_ty);
                let from_right = self.expr(right)?;
                self.coerce(&from_right, &operand_ty);
                self.code.pop(2);
                self.code.branch(if_icmp_opcode(effective), target);
            }
        }
        Ok(())
    }

    /// `value_used` is false when the call's result is discarded, as an expression
    /// statement's is. Only then may a method the backend cannot type return void.
    fn method_call(
        &mut self,
        receiver: Option<&Expression>,
        method: &str,
        args: &[Expression],
        value_used: bool,
    ) -> BytecodeResult<JavaType> {
        // Special-case System.out.println for observable output.
        if method == "println" || method == "print" {
            if let Some(Expression::FieldAccess { field, class, .. }) = receiver {
                if field == "out" && (class == "System" || class.ends_with("System")) {
                    let valid_arity = if method == "println" {
                        args.len() <= 1
                    } else {
                        args.len() == 1
                    };
                    if !valid_arity {
                        return Err(Self::arity_error("java/io/PrintStream", method, args.len()));
                    }
                    return self.println(method, args);
                }
            }
        }

        // Unqualified call → a method of the current class (static, or an
        // instance method invoked on the implicit `this`).
        if receiver.is_none() {
            if let Some(sig) = self.resolve_method(method, args) {
                let param_types = parse_param_types(&sig.descriptor);
                // Instance methods take `this` (slot 0) as the receiver.
                if !sig.is_static {
                    self.code.op(ALOAD);
                    self.code.u8(0);
                    self.code.push(1);
                }
                // Push args, coercing to declared parameter types.
                for (i, arg) in args.iter().enumerate() {
                    let at = self.expr(arg)?;
                    Self::ensure_argument_value(&at)?;
                    if let Some(pt) = param_types.get(i) {
                        self.coerce(&at, pt);
                    }
                }
                let idx = self.cp.add_method_ref(&sig.owner, method, &sig.descriptor);
                self.code.op(if sig.is_static {
                    INVOKESTATIC
                } else {
                    INVOKEVIRTUAL
                });
                self.code.u16(idx);
                // Pop receiver (instance only) + args, push return.
                if !sig.is_static {
                    self.code.pop(1);
                }
                for pt in &param_types {
                    self.code.pop(Locals::width(pt) as i32);
                }
                let rw = ret_width(&sig.return_type);
                if rw > 0 {
                    self.code.push(rw);
                }
                return Ok(sig.return_type);
            }
            if self.has_method_in(self.class_internal, method) {
                return Err(Self::arity_error(self.class_internal, method, args.len()));
            }
        }

        // Static call on a class name, e.g. `Math.abs(x)`: the "receiver" is a
        // bare name that is neither a local nor a field. A bare *field* receiver
        // (`field.method()`) is an instance call on `this.field`, handled below —
        // excluding it here keeps it from being miscompiled as `invokestatic
        // field.method`.
        if let Some(Expression::NameExpr(name)) = receiver {
            if self.locals.lookup(name).is_none() && !self.fields.contains_key(name) {
                let owner = qualify_runtime_class(name);
                if matches!(
                    stdlib_static_arity_is_valid(&owner, method, args.len()),
                    Some(false)
                ) {
                    return Err(Self::arity_error(&owner, method, args.len()));
                }
                if let Some(sig) = self.resolve_method_in(&owner, method, args) {
                    if !sig.is_static {
                        return Err(BytecodeError::TypeError {
                            message: format!(
                                "instance method {}.{} cannot be called on the class",
                                owner.replace('/', "."),
                                method
                            ),
                        });
                    }
                    let param_types = parse_param_types(&sig.descriptor);
                    for (i, arg) in args.iter().enumerate() {
                        let arg_type = self.expr(arg)?;
                        Self::ensure_argument_value(&arg_type)?;
                        if let Some(param_type) = param_types.get(i) {
                            self.coerce(&arg_type, param_type);
                        }
                    }
                    let idx = self.cp.add_method_ref(&sig.owner, method, &sig.descriptor);
                    self.code.op(INVOKESTATIC);
                    self.code.u16(idx);
                    for param_type in &param_types {
                        self.code.pop(Locals::width(param_type) as i32);
                    }
                    let width = ret_width(&sig.return_type);
                    if width > 0 {
                        self.code.push(width);
                    }
                    return Ok(sig.return_type);
                }
                if self.has_method_in(&owner, method) {
                    return Err(Self::arity_error(&owner, method, args.len()));
                }
                // Compile args; for known stdlib methods with fixed parameter
                // types, coerce each argument (e.g. int → double for Math.pow).
                let param_types =
                    stdlib_static_param_types(&owner, method, args.len()).or_else(|| {
                        let inferred: Vec<JavaType> = args.iter().map(|a| self.infer(a)).collect();
                        jdk_stub_static_overload(&owner, method, &inferred)
                            .map(|(params, _)| params)
                    });
                let mut arg_types = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    let at = self.expr(a)?;
                    Self::ensure_argument_value(&at)?;
                    match param_types.as_ref().and_then(|p| p.get(i)) {
                        Some(pt) => {
                            self.coerce(&at, pt);
                            arg_types.push(pt.clone());
                        }
                        None => {
                            // An erased lambda local is stored as Object, but
                            // its retained target type still determines the
                            // referenced method descriptor (e.g. parseInt's
                            // String parameter).
                            let logical = self.infer(a);
                            arg_types.push(if matches!(at, JavaType::Object(_)) {
                                logical
                            } else {
                                at
                            });
                        }
                    }
                }
                // The descriptor names the return type, so a guessed one is a call to a method that
                // may not exist. An unknown return type is refused, as an instance call's is.
                let Some(ret) = static_jdk_return_type(&owner, method, &arg_types) else {
                    return Err(BytecodeError::UnsupportedFeature {
                        feature: format!(
                            "the return type of {}.{method} is not known to the backend",
                            owner.replace('/', ".")
                        ),
                    });
                };
                let mut desc = String::from("(");
                for t in &arg_types {
                    desc.push_str(&value_descriptor(t));
                }
                desc.push(')');
                desc.push_str(&ret.descriptor());
                let idx = self.cp.add_method_ref(&owner, method, &desc);
                self.code.op(INVOKESTATIC);
                self.code.u16(idx);
                for t in &arg_types {
                    self.code.pop(Locals::width(t) as i32);
                }
                let rw = ret_width(&ret);
                if rw > 0 {
                    self.code.push(rw);
                }
                return Ok(ret);
            }
        }

        // Instance method call `receiver.method(args)` → invokevirtual.
        if let Some(recv) = receiver {
            let owner = match self.infer(recv) {
                JavaType::Object(name) => name,
                _ => self.class_internal.to_string(),
            };
            if matches!(
                stdlib_instance_arity_is_valid(&owner, method, args.len()),
                Some(false)
            ) {
                return Err(Self::arity_error(&owner, method, args.len()));
            }
            let generic_return = self.generic_method_return(recv, &owner, method);
            // `String.formatted(Object...)` receives one Object[] at the JVM
            // level. Source arguments must be boxed and packed into that array;
            // inferring a descriptor from each source argument would produce a
            // nonexistent overload such as `(I)Ljava/lang/String;`.
            if owner == "java/lang/String" && method == "formatted" {
                self.expr(recv)?;
                self.push_int(args.len() as i32);
                self.emit_array_create(&JavaType::Object("java/lang/Object".to_string()));
                for (i, arg) in args.iter().enumerate() {
                    self.code.op(DUP);
                    self.code.push(1);
                    self.push_int(i as i32);
                    let arg_ty = self.expr(arg)?;
                    Self::ensure_argument_value(&arg_ty)?;
                    box_to_object(self, &arg_ty);
                    self.code.op(0x53); // aastore
                    self.code.pop(3); // arrayref + index + value
                }
                let descriptor = "([Ljava/lang/Object;)Ljava/lang/String;";
                let idx = self.cp.add_method_ref(&owner, method, descriptor);
                self.code.op(INVOKEVIRTUAL);
                self.code.u16(idx);
                self.code.pop(2); // receiver + Object[]
                self.code.push(1); // String result
                return Ok(JavaType::Object("java/lang/String".to_string()));
            }
            // Compile receiver, then args.
            self.expr(recv)?;
            // Resolve the descriptor from the receiver's class (self or sibling).
            let sig = self.resolve_method_in(&owner, method, args);
            if sig.is_none() && self.has_method_in(&owner, method) {
                return Err(Self::arity_error(&owner, method, args.len()));
            }
            // For unresolved stdlib calls, a known return type keeps the
            // descriptor and stack tracking correct (e.g. String.equals → Z).
            let stdlib_ret = if sig.is_none() {
                self.instance_return_type_fallback(&owner, method, args.len())
            } else {
                None
            };
            // A call to a functional-interface SAM (`f.apply(...)`, `s.get()`, …)
            // on a lambda value returns the lambda's (boxed) result; type it as
            // `Object` so the surrounding code keeps and unboxes it. The name alone
            // does not make a SAM: `List.get(int)` has a `get` too, so an owner whose stub
            // declares that method keeps the declared descriptor, unless it is a
            // `java.util.function` interface.
            let is_sam = sig.is_none()
                && is_functional_sam(&owner, method)
                && (owner.starts_with("java/util/function/")
                    || jdk_stub_param_types(&owner, method, args.len()).is_none());
            // A JDK parameter the stub declares as a reference takes a primitive argument boxed,
            // as `list.add(10)` on a List<Integer> does. A primitive parameter stays primitive.
            // A JDK method the stub registry declares takes exactly the stub's parameter types, so the
            // descriptor names them: `add(Object)` for `list.add(new P())`.
            // An inherited JDK method (`equals` on a class that does not override it) is declared by
            // the JDK ancestor, and that ancestor's stub gives its parameter types and owner.
            let jdk_owner = if sig.is_none() {
                self.jdk_declaring_class(&owner, method, args.len())
            } else {
                None
            };
            let stub_params = if sig.is_none() && !is_sam {
                jdk_stub_param_types(jdk_owner.as_deref().unwrap_or(&owner), method, args.len())
            } else {
                None
            };
            let arg_boxes: Vec<bool> = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let at = self.infer(a);
                    is_primitive_type(&at)
                        && stub_params
                            .as_ref()
                            .and_then(|params| params.get(i))
                            .is_some_and(is_reference_type)
                })
                .collect();
            let descriptor = sig
                .as_ref()
                .map(|s| s.descriptor.clone())
                .unwrap_or_else(|| {
                    let mut d = String::from("(");
                    for (i, a) in args.iter().enumerate() {
                        if is_sam {
                            // Generic SAM parameters erase to Object. Keeping
                            // that descriptor lets us preserve the primitive's
                            // actual wrapper type at the call site.
                            d.push_str("Ljava/lang/Object;");
                        } else if let Some(param) =
                            stub_params.as_ref().and_then(|params| params.get(i))
                        {
                            d.push_str(&param.descriptor());
                        } else {
                            d.push_str(&value_descriptor(&self.infer(a)));
                        }
                    }
                    d.push(')');
                    let ret_desc = if is_sam {
                        "Ljava/lang/Object;".to_string()
                    } else {
                        stdlib_ret
                            .as_ref()
                            .map_or_else(|| "V".to_string(), JavaType::descriptor)
                    };
                    d.push_str(&ret_desc);
                    d
                });
            // For an inherited method the declaring superclass owns the ref, so
            // our VM (whose invokevirtual does not walk the hierarchy) resolves
            // it; otherwise the static receiver type.
            let ref_owner = sig
                .as_ref()
                .map(|s| s.owner.clone())
                .or(jdk_owner)
                .unwrap_or_else(|| owner.clone());
            let param_types = parse_param_types(&descriptor);
            for (i, a) in args.iter().enumerate() {
                let at = self.expr(a)?;
                Self::ensure_argument_value(&at)?;
                if is_sam || arg_boxes[i] {
                    box_to_object(self, &at);
                } else if let Some(pt) = param_types.get(i) {
                    self.coerce(&at, pt);
                }
            }
            let owner_is_interface = self
                .registry
                .get(&ref_owner)
                .is_some_and(|info| info.is_interface)
                || crate::interpreter::JdkStubRegistry::new().is_interface(&ref_owner);
            if owner_is_interface {
                // invokeinterface takes the argument slots, counting the receiver, and a zero byte.
                let slots: usize = 1 + param_types
                    .iter()
                    .map(|pt| usize::from(Locals::width(pt)))
                    .sum::<usize>();
                let idx = self
                    .cp
                    .add_interface_method_ref(&ref_owner, method, &descriptor);
                self.code.op(0xb9); // invokeinterface
                self.code.u16(idx);
                self.code.u8(u8::try_from(slots).unwrap_or(u8::MAX));
                self.code.u8(0);
            } else {
                let idx = self.cp.add_method_ref(&ref_owner, method, &descriptor);
                self.code.op(INVOKEVIRTUAL);
                self.code.u16(idx);
            }
            // Pop receiver + args; push return value.
            self.code.pop(1);
            for pt in &param_types {
                self.code.pop(Locals::width(pt) as i32);
            }
            // A functional-interface call returns the boxed result, whatever the stubbed method says.
            let fallback_ret = if is_sam { None } else { stdlib_ret };
            let ret = match sig.map(|s| s.return_type).or(fallback_ret) {
                Some(ty) => ty,
                None if is_sam => JavaType::Object("java/lang/Object".to_string()),
                // The backend has no return type for this method. Its descriptor says
                // void, so a value used from it would underflow the operand stack.
                None if value_used => {
                    return Err(BytecodeError::UnsupportedFeature {
                        feature: format!(
                            "the return type of {}.{method} is not known to the backend, so its value cannot be used",
                            owner.replace('/', ".")
                        ),
                    });
                }
                None => JavaType::Void,
            };
            let rw = ret_width(&ret);
            if rw > 0 {
                self.code.push(rw);
            }
            if let Some(logical_return) = generic_return {
                if matches!(ret, JavaType::Object(_)) && logical_return != ret {
                    let JavaType::Object(class_name) = &logical_return else {
                        return Ok(ret);
                    };
                    let class_index = self.cp.add_class(class_name);
                    self.code.op(0xc0); // checkcast
                    self.code.u16(class_index);
                }
                return Ok(logical_return);
            }
            return Ok(ret);
        }

        if receiver.is_none()
            && self
                .instance_return_type_fallback(self.class_internal, method, args.len())
                .is_some()
        {
            // `ordinal()` inside an enum is `this.ordinal()`.
            return self.method_call(Some(&Expression::This), method, args, value_used);
        }
        Err(BytecodeError::InternalError {
            message: format!("unsupported method call: {method}"),
        })
    }

    /// Emit `System.out.println(arg)` / `print`.
    fn println(&mut self, method: &str, args: &[Expression]) -> BytecodeResult<JavaType> {
        let field_idx = self
            .cp
            .add_field_ref("java/lang/System", "out", "Ljava/io/PrintStream;");
        self.code.op(GETSTATIC);
        self.code.u16(field_idx);
        self.code.push(1);

        let desc = if let Some(arg) = args.first() {
            let at = self.expr(arg)?;
            Self::ensure_argument_value(&at)?;
            match at {
                JavaType::Int | JavaType::Short | JavaType::Byte => "(I)V",
                JavaType::Long => "(J)V",
                JavaType::Boolean => "(Z)V",
                JavaType::Char => "(C)V",
                JavaType::Float => "(F)V",
                JavaType::Double => "(D)V",
                JavaType::Object(ref n) if n == "java/lang/String" => "(Ljava/lang/String;)V",
                JavaType::Object(ref n) => {
                    // `println(Object)`: the PrintStream native cannot re-enter
                    // the interpreter to run a user `toString`, so call it here
                    // and print the resulting String. Virtual dispatch honors an
                    // overridden toString; a class without one falls back to the
                    // VM's native toString.
                    let m = self
                        .cp
                        .add_method_ref(n, "toString", "()Ljava/lang/String;");
                    self.code.op(INVOKEVIRTUAL);
                    self.code.u16(m);
                    // arg ref consumed, String pushed — net stack unchanged.
                    "(Ljava/lang/String;)V"
                }
                _ => "(Ljava/lang/Object;)V",
            }
        } else {
            "()V"
        };
        let mref = self.cp.add_method_ref("java/io/PrintStream", method, desc);
        self.code.op(INVOKEVIRTUAL);
        self.code.u16(mref);
        // Pop receiver + arg(s).
        self.code.pop(1);
        if !args.is_empty() {
            let pt = parse_param_types(desc);
            for p in &pt {
                self.code.pop(Locals::width(p) as i32);
            }
        }
        Ok(JavaType::Void)
    }

    fn assign(
        &mut self,
        target: &Expression,
        value: &Expression,
        value_used: bool,
    ) -> BytecodeResult<JavaType> {
        match target {
            Expression::NameExpr(name) => {
                // Local variable first, then an implicit-`this` field.
                if let Some((slot, ty)) = self.locals.lookup(name) {
                    let vt = self.expr(value)?;
                    self.coerce(&vt, &ty);
                    if value_used {
                        // Duplicate the value so it remains after the store.
                        if ty.is_category2() {
                            self.code.op(DUP2);
                            self.code.push(2);
                        } else {
                            self.code.op(DUP);
                            self.code.push(1);
                        }
                    }
                    self.store_local(slot, &ty);
                    return Ok(ty);
                }
                if let Some(info) = self.fields.get(name).cloned() {
                    self.store_self_field(name, &info, value, value_used)?;
                    return Ok(info.ty);
                }
                Err(BytecodeError::InternalError {
                    message: format!("assignment to undefined variable: {name}"),
                })
            }
            // `receiver.field = value` → putfield. `value_used` (compound/nested
            // assignment value) is not yet supported for field targets.
            Expression::FieldAccess {
                receiver, field, ..
            } => {
                let receiver_owner = self.receiver_class(receiver.as_deref());
                let (field_owner, ftype) = self
                    .resolve_instance_field_info(&receiver_owner, field)
                    .ok_or_else(|| BytecodeError::TypeError {
                        message: format!(
                            "cannot resolve instance field {}.{}",
                            receiver_owner.replace('/', "."),
                            field
                        ),
                    })?;
                match receiver.as_deref() {
                    Some(r) => {
                        self.expr(r)?;
                    }
                    None => {
                        self.code.op(ALOAD);
                        self.code.u8(0);
                        self.code.push(1);
                    }
                }
                let vt = self.expr(value)?;
                self.coerce(&vt, &ftype);
                let idx = self
                    .cp
                    .add_field_ref(&field_owner, field, &ftype.descriptor());
                self.code.op(0xb5); // putfield
                self.code.u16(idx);
                self.code.pop(1 + ret_width(&ftype)); // objectref + value
                Ok(ftype)
            }
            // `array[index] = value` → Xastore.
            Expression::ArrayAccess { array, index } => {
                let arr_ty = self.expr(array)?;
                let it = self.expr(index)?;
                self.coerce(&it, &JavaType::Int);
                let elem = array_element_type(&arr_ty);
                let vt = self.expr(value)?;
                self.coerce(&vt, &elem);
                self.code.op(array_store_op(&elem));
                self.code.pop(2 + ret_width(&elem)); // arrayref + index + value
                Ok(elem)
            }
            other => Err(BytecodeError::InternalError {
                message: format!("unsupported assignment target: {other:?}"),
            }),
        }
    }

    /// Discard a value of the given type left on the operand stack.
    fn discard(&mut self, t: &JavaType) {
        match ret_width(t) {
            2 => {
                self.code.op(POP2);
                self.code.pop(2);
            }
            1 => {
                self.code.op(POP);
                self.code.pop(1);
            }
            _ => {}
        }
    }

    /// Evaluate an expression for its side effects, discarding any value.
    fn expr_stmt(&mut self, e: &Expression) -> BytecodeResult<()> {
        if let Expression::Assignment { target, value } = e {
            self.assign(target, value, false)?;
            return Ok(());
        }
        // A discarded call may return void, so its return type need not be known.
        let t = match e {
            Expression::MethodCall {
                receiver,
                method,
                args,
            } => self.method_call(receiver.as_deref(), method, args, false)?,
            _ => self.expr(e)?,
        };
        self.discard(&t);
        Ok(())
    }

    // ── Statements ──────────────────────────────────────────────────────────
    /// Compile a statement. Returns `true` if control can fall through to the
    /// next statement (i.e. the statement does not always return/throw). This
    /// lets control-flow codegen avoid emitting dead jumps that would target a
    /// position past the end of the method.
    fn stmt(&mut self, s: &Statement) -> BytecodeResult<bool> {
        match s {
            Statement::Expression(e) => {
                self.expr_stmt(e)?;
                Ok(true)
            }
            Statement::ExplicitConstructorCall { is_super, args } => {
                self.explicit_ctor_call(*is_super, args)?;
                Ok(true)
            }
            Statement::ForEach {
                elem_type,
                var_name,
                iterable,
                body,
            } => {
                // Lowering depends on the iterable's type, known only now.
                let lowered = self.lower_foreach(elem_type, var_name, iterable, body);
                self.stmt(&lowered)
            }
            Statement::VariableDeclaration {
                name,
                var_type,
                generic_arguments,
                initializer,
            } => {
                // `var x = expr` arrives as the sentinel type Object("var")
                // (`var` is a reserved type name, never a real class); resolve
                // it from the initializer's inferred type.
                let resolved = match var_type {
                    JavaType::Object(n) if n == "var" => initializer
                        .as_ref()
                        .map(|init| self.infer(init))
                        .unwrap_or(JavaType::Object("java/lang/Object".to_string())),
                    other => other.clone(),
                };
                let slot = self.locals.declare_with_generics(
                    name,
                    resolved.clone(),
                    generic_arguments.clone(),
                );
                if let Some(init) = initializer {
                    let vt = self.expr(init)?;
                    self.coerce(&vt, &resolved);
                    self.store_local(slot, &resolved);
                }
                Ok(true)
            }
            Statement::Block(stmts) => {
                self.locals.push_scope();
                let mut falls = true;
                for st in stmts {
                    falls = self.stmt(st)?;
                }
                self.locals.pop_scope();
                Ok(falls)
            }
            // Scope-transparent: compile each child in the current scope (no
            // push/pop), so multi-declarator locals stay visible afterwards.
            Statement::Multi(stmts) => {
                let mut falls = true;
                for st in stmts {
                    falls = self.stmt(st)?;
                }
                Ok(falls)
            }
            Statement::If {
                condition,
                then_stmt,
                else_stmt,
            } => {
                let l_else = self.code.new_label();
                self.branch_if_false(condition, l_else)?;
                let then_falls = self.stmt(then_stmt)?;
                if let Some(else_s) = else_stmt {
                    let l_end = self.code.new_label();
                    // Only bridge over the else arm if the then arm can reach here.
                    if then_falls {
                        self.code.branch(GOTO, l_end);
                    }
                    self.code.place(l_else);
                    let else_falls = self.stmt(else_s)?;
                    self.code.place(l_end);
                    Ok(then_falls || else_falls)
                } else {
                    self.code.place(l_else);
                    Ok(true)
                }
            }
            Statement::While { condition, body } => {
                let l_top = self.code.new_label();
                let l_end = self.code.new_label();
                self.code.place(l_top);
                self.branch_if_false(condition, l_end)?;
                // `continue` re-tests the condition at l_top; `break` exits.
                self.loop_labels
                    .push(JumpTargets::new(l_end, l_top, self.finally_blocks.len()));
                self.stmt(body)?;
                self.loop_labels.pop();
                self.code.branch(GOTO, l_top);
                self.code.place(l_end);
                Ok(true)
            }
            Statement::For {
                init,
                condition,
                update,
                body,
            } => {
                self.locals.push_scope();
                if let Some(init) = init {
                    self.stmt(init)?;
                }
                let l_top = self.code.new_label();
                let l_cont = self.code.new_label();
                let l_end = self.code.new_label();
                self.code.place(l_top);
                if let Some(cond) = condition {
                    self.branch_if_false(cond, l_end)?;
                }
                // `continue` jumps to the update step (l_cont), not the test.
                self.loop_labels
                    .push(JumpTargets::new(l_end, l_cont, self.finally_blocks.len()));
                self.stmt(body)?;
                self.loop_labels.pop();
                self.code.place(l_cont);
                if let Some(update) = update {
                    self.expr_stmt(update)?;
                }
                self.code.branch(GOTO, l_top);
                self.code.place(l_end);
                self.locals.pop_scope();
                Ok(true)
            }
            Statement::DoWhile { condition, body } => {
                let l_top = self.code.new_label();
                let l_cont = self.code.new_label();
                let l_end = self.code.new_label();
                self.code.place(l_top);
                // `continue` jumps to the bottom condition test (l_cont).
                self.loop_labels
                    .push(JumpTargets::new(l_end, l_cont, self.finally_blocks.len()));
                self.stmt(body)?;
                self.loop_labels.pop();
                self.code.place(l_cont);
                self.branch_if_true(condition, l_top)?;
                self.code.place(l_end);
                Ok(true)
            }
            Statement::Break(label) => {
                if label.is_some() {
                    return Err(BytecodeError::InternalError {
                        message: "labelled break not supported".to_string(),
                    });
                }
                let (target, depth) = self
                    .loop_labels
                    .last()
                    .map(|targets| (targets.break_label, targets.break_depth))
                    .ok_or_else(|| BytecodeError::InternalError {
                        message: "break outside loop".to_string(),
                    })?;
                self.emit_finally_blocks_above(depth)?;
                self.code.branch(GOTO, target);
                Ok(false) // break transfers control; no fall-through
            }
            Statement::Continue(label) => {
                if label.is_some() {
                    return Err(BytecodeError::InternalError {
                        message: "labelled continue not supported".to_string(),
                    });
                }
                let (target, depth) = self
                    .loop_labels
                    .last()
                    .map(|targets| (targets.continue_label, targets.continue_depth))
                    .ok_or_else(|| BytecodeError::InternalError {
                        message: "continue outside loop".to_string(),
                    })?;
                self.emit_finally_blocks_above(depth)?;
                self.code.branch(GOTO, target);
                Ok(false)
            }
            Statement::Throw(e) => {
                self.expr(e)?;
                self.code.op(0xbf); // athrow
                self.code.pop(1);
                Ok(false) // throw does not fall through
            }
            Statement::Return(None) => {
                self.emit_finally_blocks_above(0)?;
                if matches!(
                    self.return_type,
                    JavaType::Object(_) | JavaType::Array { .. }
                ) {
                    // Synthetic lambda `doCall` methods always return Object,
                    // including void-compatible lambda blocks.
                    self.code.op(0x01); // ACONST_NULL
                    self.code.push(1);
                    self.code.op(ARETURN);
                } else {
                    self.code.op(RETURN);
                }
                Ok(false)
            }
            Statement::Return(Some(e)) => {
                let t = self.expr(e)?;
                let return_type = self.return_type.clone();
                if matches!(return_type, JavaType::Object(_)) {
                    box_to_object(self, &t);
                } else {
                    self.coerce(&t, &return_type);
                }
                if !self.finally_blocks.is_empty() {
                    // The value is kept in a temporary while the finally blocks run, so
                    // they see the same stack they would see on their normal exit.
                    let name = format!("$return{}", self.locals.next_slot);
                    let slot = self.locals.declare(&name, return_type.clone());
                    self.store_local(slot, &return_type);
                    self.emit_finally_blocks_above(0)?;
                    self.load_local(&name)?;
                }
                let op = match return_type {
                    JavaType::Long => LRETURN,
                    JavaType::Float => FRETURN,
                    JavaType::Double => DRETURN,
                    JavaType::Object(_) | JavaType::Array { .. } => ARETURN,
                    JavaType::Void => RETURN,
                    _ => IRETURN,
                };
                self.code.op(op);
                Ok(false)
            }
            Statement::Try {
                body,
                catches,
                finally_block,
            } => self.try_stmt(body, catches, finally_block.as_deref()),
            Statement::Switch { scrutinee, cases } => self.switch_stmt(scrutinee, cases),
        }
    }

    /// Compile a traditional `switch` (int/char keys, colon arms with
    /// fall-through) using `lookupswitch`. Arm bodies are emitted in order so
    /// fall-through is implicit; `break` exits to the switch end.
    /// Lower a switch whose labels name enum constants to a switch on the scrutinee's `ordinal()`.
    /// The constants of an enum are its leading static fields of the enum's type, in declaration
    /// order, so a constant's ordinal is its position among them.
    fn enum_switch_as_ordinals(
        &mut self,
        scrutinee: &Expression,
        cases: &[crate::java_compiler::SwitchCase],
    ) -> BytecodeResult<(Expression, Vec<crate::java_compiler::SwitchCase>)> {
        let JavaType::Object(enum_name) = self.infer(scrutinee) else {
            return Err(BytecodeError::TypeError {
                message: "switch case labels name enum constants, but the scrutinee is not an enum"
                    .to_string(),
            });
        };
        let enum_type = JavaType::Object(enum_name.clone());
        let constants: Vec<String> = self
            .registry
            .get(&enum_name)
            .filter(|info| info.super_name == "java/lang/Enum")
            .map(|info| {
                info.fields
                    .iter()
                    .take_while(|(_, ty, is_static)| *is_static && *ty == enum_type)
                    .map(|(name, _, _)| name.clone())
                    .collect()
            })
            .ok_or_else(|| BytecodeError::TypeError {
                message: format!("{} is not an enum", enum_name.replace('/', ".")),
            })?;

        let mut lowered = Vec::new();
        for case in cases {
            let mut values = case.values.clone();
            for name in &case.names {
                let ordinal = constants
                    .iter()
                    .position(|constant| constant == name)
                    .ok_or_else(|| BytecodeError::TypeError {
                        message: format!(
                            "{name} is not a constant of {}",
                            enum_name.replace('/', ".")
                        ),
                    })?;
                values.push(
                    i32::try_from(ordinal).map_err(|_| BytecodeError::TypeError {
                        message: "enum with more constants than an int can index".to_string(),
                    })?,
                );
            }
            lowered.push(crate::java_compiler::SwitchCase {
                values,
                names: Vec::new(),
                is_default: case.is_default,
                body: case.body.clone(),
            });
        }
        let ordinal = Expression::MethodCall {
            receiver: Some(Box::new(scrutinee.clone())),
            method: "ordinal".to_string(),
            args: Vec::new(),
        };
        Ok((ordinal, lowered))
    }

    fn switch_stmt(
        &mut self,
        scrutinee: &Expression,
        cases: &[crate::java_compiler::SwitchCase],
    ) -> BytecodeResult<bool> {
        if cases.iter().any(|case| !case.names.is_empty()) {
            let (ordinal_scrutinee, ordinal_cases) =
                self.enum_switch_as_ordinals(scrutinee, cases)?;
            return self.switch_stmt(&ordinal_scrutinee, &ordinal_cases);
        }
        let st = self.expr(scrutinee)?;
        self.coerce(&st, &JavaType::Int);
        self.code.pop(1); // lookupswitch consumes the key

        let l_end = self.code.new_label();
        let case_labels: Vec<Label> = cases.iter().map(|_| self.code.new_label()).collect();
        let mut default_label = l_end;
        let mut pairs: Vec<(i32, Label)> = Vec::new();
        for (i, c) in cases.iter().enumerate() {
            if c.is_default {
                default_label = case_labels[i];
            }
            for &v in &c.values {
                pairs.push((v, case_labels[i]));
            }
        }
        // lookupswitch requires match-offset pairs sorted by match value.
        pairs.sort_by_key(|(v, _)| *v);

        let switch_pos = self.code.code_len();
        self.code.op(0xab); // lookupswitch
        let pad = (4 - (self.code.code_len() % 4)) % 4;
        for _ in 0..pad {
            self.code.u8(0);
        }
        self.code.wide_branch(switch_pos, default_label);
        self.code.u32(pairs.len() as u32);
        for (v, lbl) in &pairs {
            self.code.i32(*v);
            self.code.wide_branch(switch_pos, *lbl);
        }

        // `break` exits the switch; `continue` keeps targeting the enclosing
        // loop (a switch is not a continue target).
        let depth = self.finally_blocks.len();
        let (continue_label, continue_depth) = self
            .loop_labels
            .last()
            .map(|targets| (targets.continue_label, targets.continue_depth))
            .unwrap_or((l_end, depth));
        self.loop_labels.push(JumpTargets {
            break_label: l_end,
            break_depth: depth,
            continue_label,
            continue_depth,
        });
        self.locals.push_scope();
        for (i, c) in cases.iter().enumerate() {
            self.code.place(case_labels[i]);
            for s in &c.body {
                self.stmt(s)?;
            }
        }
        self.locals.pop_scope();
        self.loop_labels.pop();
        self.code.place(l_end);
        Ok(true)
    }

    /// Compiles `compile` with `finally_block` (if any) active, so that a jump out of it runs
    /// the finally block.
    fn with_finally_active(
        &mut self,
        finally_block: Option<&Statement>,
        try_index: usize,
        compile: impl FnOnce(&mut Self) -> BytecodeResult<bool>,
    ) -> BytecodeResult<bool> {
        let Some(block) = finally_block else {
            return compile(self);
        };
        self.finally_blocks.push(ActiveFinally {
            block: block.clone(),
            try_index,
        });
        let result = compile(self);
        self.finally_blocks.pop();
        result
    }

    /// Runs the finally blocks entered since `depth`, innermost first, for a jump out of
    /// them. Each runs with only the blocks outside it active, so a jump inside a finally
    /// does not run it again.
    fn emit_finally_blocks_above(&mut self, depth: usize) -> BytecodeResult<()> {
        let mut index = self.finally_blocks.len();
        while index > depth {
            index -= 1;
            let ActiveFinally { block, try_index } = self.finally_blocks[index].clone();
            let inner = self.finally_blocks.split_off(index);
            let result = self.inline_finally(&block, try_index);
            self.finally_blocks.extend(inner);
            result?;
        }
        Ok(())
    }

    /// Emits a copy of the finally block of the try at `try_index`, and leaves it out of
    /// the exception ranges of that try and of the tries inside it, as javac does.
    fn inline_finally(&mut self, block: &Statement, try_index: usize) -> BytecodeResult<bool> {
        let start = self.code.new_label();
        self.code.place(start);
        let falls = self.stmt(block)?;
        let end = self.code.new_label();
        self.code.place(end);
        for gaps in &mut self.try_gaps[try_index..] {
            gaps.push((start, end));
        }
        Ok(falls)
    }

    /// Compile `try { body } catch (E e) { … } finally { … }`.
    ///
    /// Emits a real exception table: one entry per catch (protecting the try
    /// body) plus, when a `finally` is present, a catch-all entry covering the
    /// body and the catch handlers that runs the finally block and rethrows.
    /// The finally block is also emitted inline on each normal exit edge, and on each
    /// return, break or continue out of the body or a catch (see `finally_blocks`).
    /// These inline copies are left out of the exception table (see `try_gaps`).
    fn try_stmt(
        &mut self,
        body: &Statement,
        catches: &[crate::java_compiler::CatchClause],
        finally_block: Option<&Statement>,
    ) -> BytecodeResult<bool> {
        let l_start = self.code.new_label();
        let l_body_end = self.code.new_label();
        let l_after = self.code.new_label();
        let try_index = self.try_gaps.len();
        self.try_gaps.push(Vec::new());

        self.code.place(l_start);
        let body_falls =
            self.with_finally_active(finally_block, try_index, |gen| gen.stmt(body))?;
        self.code.place(l_body_end);
        // The try completes normally when its body or one of its catches does (JLS 14.22).
        // A jump to the join label is only emitted for such an edge, so the label is only
        // reachable from code that falls through to it.
        let mut falls = body_falls;
        if body_falls {
            if let Some(fin) = finally_block {
                self.inline_finally(fin, try_index)?;
            }
            self.code.branch(GOTO, l_after);
        }

        // One handler per catch clause, each protecting the try body.
        let mut catch_handlers = Vec::new();
        for c in catches {
            let l_handler = self.code.new_label();
            self.code.place(l_handler);
            // The VM enters a handler with only the exception ref on the stack.
            self.code.clear_stack();
            self.code.push(1);
            self.locals.push_scope();
            let slot = self
                .locals
                .declare(&c.variable, JavaType::Object(c.exception_type.clone()));
            self.store_local(slot, &JavaType::Object(c.exception_type.clone()));
            let catch_falls =
                self.with_finally_active(finally_block, try_index, |gen| gen.stmt(&c.body))?;
            self.locals.pop_scope();
            if catch_falls {
                falls = true;
                if let Some(fin) = finally_block {
                    self.inline_finally(fin, try_index)?;
                }
                self.code.branch(GOTO, l_after);
            }
            let ct = self.cp.add_class(&c.exception_type);
            catch_handlers.push((l_handler, ct));
        }
        let gaps = self.try_gaps.pop().unwrap_or_default();
        for (l_handler, ct) in catch_handlers {
            self.code
                .add_exception(l_start, l_body_end, &gaps, l_handler, ct);
        }

        // finally: a catch-all over the body + catch handlers that runs the
        // finally block and rethrows (covers uncaught and in-catch exceptions).
        if let Some(fin) = finally_block {
            let l_finally = self.code.new_label();
            self.code.place(l_finally);
            self.code.clear_stack();
            self.code.push(1);
            let slot = self.locals.declare(
                "$finally_exc",
                JavaType::Object("java/lang/Throwable".to_string()),
            );
            self.store_local(slot, &JavaType::Object("java/lang/Throwable".to_string()));
            self.stmt(fin)?;
            self.code.op(ALOAD);
            self.code.u8(slot as u8);
            self.code.push(1);
            self.code.op(0xbf); // athrow
            self.code.pop(1);
            // Protect everything from the try start up to (not including) the
            // finally handler itself.
            self.code
                .add_exception(l_start, l_finally, &gaps, l_finally, 0);
        }

        self.code.place(l_after);
        Ok(falls)
    }
}

fn is_comparison(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::LessThan
            | BinaryOp::LessOrEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterOrEqual
    )
}

fn negate_cmp(op: BinaryOp) -> BinaryOp {
    match op {
        BinaryOp::Equal => BinaryOp::NotEqual,
        BinaryOp::NotEqual => BinaryOp::Equal,
        BinaryOp::LessThan => BinaryOp::GreaterOrEqual,
        BinaryOp::LessOrEqual => BinaryOp::GreaterThan,
        BinaryOp::GreaterThan => BinaryOp::LessOrEqual,
        BinaryOp::GreaterOrEqual => BinaryOp::LessThan,
        other => other,
    }
}

fn if_icmp_opcode(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Equal => IF_ICMPEQ,
        BinaryOp::NotEqual => IF_ICMPNE,
        BinaryOp::LessThan => IF_ICMPLT,
        BinaryOp::LessOrEqual => IF_ICMPLE,
        BinaryOp::GreaterThan => IF_ICMPGT,
        BinaryOp::GreaterOrEqual => IF_ICMPGE,
        _ => IF_ICMPEQ,
    }
}

fn cmp_zero_opcode(op: BinaryOp) -> u8 {
    // After lcmp: result is -1/0/1; branch against zero.
    match op {
        BinaryOp::Equal => IFEQ,
        BinaryOp::NotEqual => IFNE,
        BinaryOp::LessThan => 0x9b,       // iflt
        BinaryOp::LessOrEqual => 0x9e,    // ifle
        BinaryOp::GreaterThan => 0x9d,    // ifgt
        BinaryOp::GreaterOrEqual => 0x9c, // ifge
        _ => IFEQ,
    }
}

/// The element type of an array type (defaults to int for non-arrays).
fn array_element_type(ty: &JavaType) -> JavaType {
    match ty {
        JavaType::Array {
            element_type,
            dimensions,
        } if *dimensions > 1 => JavaType::Array {
            element_type: element_type.clone(),
            dimensions: dimensions - 1,
        },
        JavaType::Array { element_type, .. } => (**element_type).clone(),
        _ => JavaType::Int,
    }
}

/// `Xaload` opcode for the given element type.
fn array_load_op(elem: &JavaType) -> u8 {
    match elem {
        JavaType::Long => 0x2f,                               // laload
        JavaType::Float => 0x30,                              // faload
        JavaType::Double => 0x31,                             // daload
        JavaType::Object(_) | JavaType::Array { .. } => 0x32, // aaload
        JavaType::Boolean | JavaType::Byte => 0x33,           // baload
        JavaType::Char => 0x34,                               // caload
        JavaType::Short => 0x35,                              // saload
        _ => 0x2e,                                            // iaload
    }
}

/// `Xastore` opcode for the given element type.
fn array_store_op(elem: &JavaType) -> u8 {
    match elem {
        JavaType::Long => 0x50,                               // lastore
        JavaType::Float => 0x51,                              // fastore
        JavaType::Double => 0x52,                             // dastore
        JavaType::Object(_) | JavaType::Array { .. } => 0x53, // aastore
        JavaType::Boolean | JavaType::Byte => 0x54,           // bastore
        JavaType::Char => 0x55,                               // castore
        JavaType::Short => 0x56,                              // sastore
        _ => 0x4f,                                            // iastore
    }
}

/// A process-unique synthetic local name (`$fe_it$N`) for desugared constructs.
/// The `$` prefix cannot collide with a Java identifier; uniqueness across the
/// whole compilation is sufficient since locals are method-scoped.
fn fresh_local(prefix: &str) -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("${prefix}${n}")
}

fn ret_width(ty: &JavaType) -> i32 {
    match ty {
        JavaType::Void => 0,
        JavaType::Long | JavaType::Double => 2,
        _ => 1,
    }
}

/// The parameter types of a JDK method, read from the stub registry's descriptor of the stubbed
/// method with this name and arity. Overloads of that arity that declare different parameter types
/// are not known, so no types are returned for them.
fn jdk_stub_param_types(owner: &str, method: &str, arity: usize) -> Option<Vec<JavaType>> {
    let registry = crate::interpreter::JdkStubRegistry::new();
    let class = registry.get_class(owner)?;
    let mut overloads = Vec::new();
    for stub in class.methods.values() {
        if stub.name != method
            || stub.is_constructor
            || parse_param_types(&stub.descriptor).len() != arity
        {
            continue;
        }
        overloads.push(parse_param_types(&stub.descriptor));
    }
    let first = overloads.first()?.clone();
    overloads
        .iter()
        .all(|params| *params == first)
        .then_some(first)
}

fn is_primitive_type(ty: &JavaType) -> bool {
    matches!(
        ty,
        JavaType::Int
            | JavaType::Long
            | JavaType::Float
            | JavaType::Double
            | JavaType::Boolean
            | JavaType::Char
            | JavaType::Byte
            | JavaType::Short
    )
}

fn is_reference_type(ty: &JavaType) -> bool {
    matches!(ty, JavaType::Object(_) | JavaType::Array { .. })
}

/// The return type of a static JDK method the backend calls: the stdlib table, then the overload the
/// stub registry selects for the argument types, then the stub's return type when every overload of
/// that arity returns the same type. `None` when none of these knows it.
fn static_jdk_return_type(owner: &str, method: &str, arg_types: &[JavaType]) -> Option<JavaType> {
    stdlib_static_return_type(owner, method, arg_types)
        .or_else(|| jdk_stub_static_overload(owner, method, arg_types).map(|(_, ret)| ret))
        .or_else(|| jdk_stub_return_type(owner, method, arg_types.len()))
}

/// The parameter and return types of the static overload of a JDK method that a call with these
/// argument types selects. An overload applies when each argument has its parameter's type or
/// widens to it (JLS 5.1.2). Of the applicable overloads, the call selects the one whose parameters
/// widen to every other's, as `Math.max(3, 4L)` selects `max(long, long)`. `None` when no overload
/// applies or no one is the narrowest.
fn jdk_stub_static_overload(
    owner: &str,
    method: &str,
    arg_types: &[JavaType],
) -> Option<(Vec<JavaType>, JavaType)> {
    let registry = crate::interpreter::JdkStubRegistry::new();
    let class = registry.get_class(owner)?;
    let applicable: Vec<(Vec<JavaType>, JavaType)> = class
        .methods
        .values()
        .filter(|stub| stub.is_static && !stub.is_constructor && stub.name == method)
        .filter_map(|stub| {
            let params = parse_param_types(&stub.descriptor);
            let ret = descriptor_return_type(&stub.descriptor)?;
            let fits = params.len() == arg_types.len()
                && params
                    .iter()
                    .zip(arg_types)
                    .all(|(param, arg)| widens_to(arg, param));
            fits.then_some((params, ret))
        })
        .collect();
    let mut narrowest = applicable.iter().filter(|(params, _)| {
        applicable.iter().all(|(other, _)| {
            params
                .iter()
                .zip(other)
                .all(|(param, other)| widens_to(param, other))
        })
    });
    let chosen = narrowest.next()?.clone();
    narrowest.next().is_none().then_some(chosen)
}

/// Whether a value of type `from` converts to `to` without a cast: the same type, a primitive
/// widening (JLS 5.1.2), or any reference to `Object`. Other reference conversions need the class
/// hierarchy, which this check does not have, so they do not count.
fn widens_to(from: &JavaType, to: &JavaType) -> bool {
    use JavaType::*;
    if from == to {
        return true;
    }
    if matches!(to, JavaType::Object(name) if name == "java/lang/Object") {
        return is_reference_type(from);
    }
    match from {
        Byte => matches!(to, Short | Int | Long | Float | Double),
        Short | Char => matches!(to, Int | Long | Float | Double),
        Int => matches!(to, Long | Float | Double),
        Long => matches!(to, Float | Double),
        Float => matches!(to, Double),
        _ => false,
    }
}

/// The return type of a JDK method, read from the stub registry's descriptor of the stubbed method
/// with this name and arity. Overloads of that arity that return different types are not known, so
/// no type is returned for them.
fn jdk_stub_return_type(owner: &str, method: &str, arity: usize) -> Option<JavaType> {
    let registry = crate::interpreter::JdkStubRegistry::new();
    let class = registry.get_class(owner)?;
    let mut returns = Vec::new();
    for stub in class.methods.values() {
        if stub.name != method
            || stub.is_constructor
            || parse_param_types(&stub.descriptor).len() != arity
        {
            continue;
        }
        returns.push(descriptor_return_type(&stub.descriptor)?);
    }
    let first = returns.first()?.clone();
    returns.iter().all(|ty| *ty == first).then_some(first)
}

/// The return type of a method descriptor like `(IJ)V`. `None` when the descriptor has no `)`.
fn descriptor_return_type(descriptor: &str) -> Option<JavaType> {
    let ret = &descriptor[descriptor.rfind(')')? + 1..];
    if ret == "V" {
        Some(JavaType::Void)
    } else {
        parse_param_types(&format!("({ret})")).into_iter().next()
    }
}

/// Parse the parameter types out of a method descriptor like `(IJ)V`.
fn parse_param_types(descriptor: &str) -> Vec<JavaType> {
    let mut types = Vec::new();
    let bytes: Vec<char> = descriptor.chars().collect();
    let mut i = 0;
    // Skip '('
    if bytes.first() == Some(&'(') {
        i = 1;
    }
    while i < bytes.len() && bytes[i] != ')' {
        let mut dims = 0u8;
        while bytes[i] == '[' {
            dims += 1;
            i += 1;
        }
        let base = match bytes[i] {
            'I' => JavaType::Int,
            'J' => JavaType::Long,
            'F' => JavaType::Float,
            'D' => JavaType::Double,
            'Z' => JavaType::Boolean,
            'B' => JavaType::Byte,
            'C' => JavaType::Char,
            'S' => JavaType::Short,
            'L' => {
                let start = i + 1;
                while i < bytes.len() && bytes[i] != ';' {
                    i += 1;
                }
                let name: String = bytes[start..i].iter().collect();
                JavaType::Object(name)
            }
            _ => JavaType::Int,
        };
        i += 1;
        if dims > 0 {
            types.push(JavaType::Array {
                element_type: Box::new(base),
                dimensions: dims,
            });
        } else {
            types.push(base);
        }
    }
    types
}

/// Collect the free identifier names referenced in a lambda body, so the lambda
/// can capture those that name an enclosing local. Method names and field names
/// are not variables and are skipped; nested-lambda params may be over-collected
/// (harmless — only names that resolve to an enclosing local are captured).
fn collect_free_names(expr: &Expression, out: &mut std::collections::HashSet<String>) {
    match expr {
        Expression::NameExpr(n) => {
            out.insert(n.clone());
        }
        Expression::BinaryExpr { left, right, .. } => {
            collect_free_names(left, out);
            collect_free_names(right, out);
        }
        Expression::UnaryExpr { operand, .. } => collect_free_names(operand, out),
        Expression::MethodCall { receiver, args, .. } => {
            if let Some(r) = receiver {
                collect_free_names(r, out);
            }
            for a in args {
                collect_free_names(a, out);
            }
        }
        Expression::Assignment { target, value } => {
            collect_free_names(target, out);
            collect_free_names(value, out);
        }
        Expression::Lambda { body, .. } => collect_free_names_in_lambda_body(body, out),
        Expression::MethodReference { qualifier, .. } => collect_free_names(qualifier, out),
        Expression::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_free_names(condition, out);
            collect_free_names(then_expr, out);
            collect_free_names(else_expr, out);
        }
        Expression::Cast { expr, .. } => collect_free_names(expr, out),
        Expression::FieldAccess { receiver, .. } => {
            if let Some(r) = receiver.as_deref() {
                collect_free_names(r, out);
            }
        }
        Expression::ArrayAccess { array, index } => {
            collect_free_names(array, out);
            collect_free_names(index, out);
        }
        Expression::NewExpr { args, .. } => {
            for a in args {
                collect_free_names(a, out);
            }
        }
        Expression::ArrayInit { elements, .. } => {
            for e in elements {
                collect_free_names(e, out);
            }
        }
        _ => {}
    }
}

fn collect_free_names_in_lambda_body(
    body: &LambdaBody,
    out: &mut std::collections::HashSet<String>,
) {
    match body {
        LambdaBody::Expression(expr) => collect_free_names(expr, out),
        LambdaBody::Block(statements) => {
            for statement in statements {
                collect_free_names_in_statement(statement, out);
            }
        }
    }
}

fn collect_free_names_in_statement(
    statement: &Statement,
    out: &mut std::collections::HashSet<String>,
) {
    match statement {
        Statement::Expression(expr) | Statement::Throw(expr) => collect_free_names(expr, out),
        Statement::VariableDeclaration { initializer, .. } => {
            if let Some(expr) = initializer {
                collect_free_names(expr, out);
            }
        }
        Statement::If {
            condition,
            then_stmt,
            else_stmt,
        } => {
            collect_free_names(condition, out);
            collect_free_names_in_statement(then_stmt, out);
            if let Some(statement) = else_stmt {
                collect_free_names_in_statement(statement, out);
            }
        }
        Statement::While { condition, body } | Statement::DoWhile { condition, body } => {
            collect_free_names(condition, out);
            collect_free_names_in_statement(body, out);
        }
        Statement::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(statement) = init {
                collect_free_names_in_statement(statement, out);
            }
            if let Some(expr) = condition {
                collect_free_names(expr, out);
            }
            if let Some(expr) = update {
                collect_free_names(expr, out);
            }
            collect_free_names_in_statement(body, out);
        }
        Statement::Block(statements) | Statement::Multi(statements) => {
            for statement in statements {
                collect_free_names_in_statement(statement, out);
            }
        }
        Statement::Return(expr) => {
            if let Some(expr) = expr {
                collect_free_names(expr, out);
            }
        }
        Statement::Try {
            body,
            catches,
            finally_block,
        } => {
            collect_free_names_in_statement(body, out);
            for catch in catches {
                collect_free_names_in_statement(&catch.body, out);
            }
            if let Some(statement) = finally_block {
                collect_free_names_in_statement(statement, out);
            }
        }
        Statement::Switch { scrutinee, cases } => {
            collect_free_names(scrutinee, out);
            for case in cases {
                for statement in &case.body {
                    collect_free_names_in_statement(statement, out);
                }
            }
        }
        Statement::ForEach { iterable, body, .. } => {
            collect_free_names(iterable, out);
            collect_free_names_in_statement(body, out);
        }
        Statement::ExplicitConstructorCall { args, .. } => {
            for arg in args {
                collect_free_names(arg, out);
            }
        }
        Statement::Break(_) | Statement::Continue(_) => {}
    }
}

/// Box the primitive value on top of the stack to its wrapper `Object`, so a
/// `doCall` can return `Object` for any lambda body. References pass through.
fn box_to_object(gen: &mut MethodGen, ty: &JavaType) {
    let (cls, desc) = match ty {
        JavaType::Int | JavaType::Short | JavaType::Byte | JavaType::Char => {
            ("java/lang/Integer", "(I)Ljava/lang/Integer;")
        }
        JavaType::Long => ("java/lang/Long", "(J)Ljava/lang/Long;"),
        JavaType::Float => ("java/lang/Float", "(F)Ljava/lang/Float;"),
        JavaType::Double => ("java/lang/Double", "(D)Ljava/lang/Double;"),
        JavaType::Boolean => ("java/lang/Boolean", "(Z)Ljava/lang/Boolean;"),
        _ => return,
    };
    let m = gen.cp.add_method_ref(cls, "valueOf", desc);
    gen.code.op(INVOKESTATIC);
    gen.code.u16(m);
    // valueOf pops the (cat-1 or cat-2) primitive and pushes a reference; net
    // stack depth is unchanged for cat-1, shrinks by one slot for cat-2.
    if matches!(ty, JavaType::Long | JavaType::Double) {
        gen.code.pop(1);
    }
}

/// Build the `<init>(Object owner, Object thisObject, <captures>)V` bytecode for
/// a lambda closure class: chain to `Closure.<init>`, then store each capture.
fn build_lambda_init(
    cp: &mut ConstantPoolBuilder,
    class_name: &str,
    super_init_ref: u16,
    captures: &[(String, JavaType)],
) -> Vec<u8> {
    let mut bc = vec![0x2A, 0x2B, 0x2C, 0xB7]; // aload_0, aload_1, aload_2, invokespecial
    bc.extend_from_slice(&super_init_ref.to_be_bytes());
    // Captured args start at slot 3 (0=this, 1=owner, 2=thisObject).
    let mut slot = 3u8;
    for (name, ty) in captures {
        bc.push(0x2A); // aload_0 (this)
        let (load_op, width) = match ty {
            JavaType::Long => (0x16u8, 2u8),                           // lload
            JavaType::Float => (0x17, 1),                              // fload
            JavaType::Double => (0x18, 2),                             // dload
            JavaType::Object(_) | JavaType::Array { .. } => (0x19, 1), // aload
            _ => (0x15, 1),                                            // iload (int family)
        };
        bc.push(load_op);
        bc.push(slot);
        slot += width;
        let field_ref = cp.add_field_ref(class_name, name, &ty.descriptor());
        bc.push(0xB5); // putfield
        bc.extend_from_slice(&field_ref.to_be_bytes());
    }
    bc.push(0xB1); // return
    bc
}

/// Generate a complete lambda closure class: `<init>` + a java-compiled `doCall`,
/// extending `groovy/lang/Closure` with the standard field layout
/// (`delegate`/`owner`/`resolveStrategy`/captures) the interpreter expects.
fn build_lambda_class(
    class_name: &str,
    captures: &[(String, JavaType)],
    params: &[String],
    body: &LambdaBody,
    target: Option<&FunctionalTarget>,
    registry: &ClassRegistry,
) -> BytecodeResult<Vec<u8>> {
    let mut cp = ConstantPoolBuilder::new();
    let class_name_idx = cp.add_class(class_name);
    let super_class_idx = cp.add_class("groovy/lang/Closure");
    let super_init_ref = cp.add_method_ref(
        "groovy/lang/Closure",
        "<init>",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
    );

    let init_name_idx = cp.add_utf8("<init>");
    let mut init_desc = String::from("(Ljava/lang/Object;Ljava/lang/Object;");
    for (_, ty) in captures {
        init_desc.push_str(&ty.descriptor());
    }
    init_desc.push_str(")V");
    let init_desc_idx = cp.add_utf8(&init_desc);

    let docall_name_idx = cp.add_utf8("doCall");
    let mut docall_desc = String::from("(");
    for _ in params {
        docall_desc.push_str("Ljava/lang/Object;");
    }
    docall_desc.push_str(")Ljava/lang/Object;");
    let docall_desc_idx = cp.add_utf8(&docall_desc);
    let code_attr_idx = cp.add_utf8("Code");

    // Standard Groovy Closure fields (must match closure_runtime::field_offset).
    let delegate_name_idx = cp.add_utf8("delegate");
    let object_desc_idx = cp.add_utf8("Ljava/lang/Object;");
    let owner_name_idx = cp.add_utf8("owner");
    let resolve_name_idx = cp.add_utf8("resolveStrategy");
    let int_desc_idx = cp.add_utf8("I");

    let cap_field_idx: Vec<(u16, u16)> = captures
        .iter()
        .map(|(name, ty)| (cp.add_utf8(name), cp.add_utf8(&ty.descriptor())))
        .collect();

    // Compile the doCall body. Locals: this(0), params(1..); captures resolve as
    // `getfield this.<cap>` via a synthetic field table.
    let mut locals = Locals::new();
    locals.declare("this", JavaType::Object(class_name.to_string()));
    for p in params {
        locals.declare(p, JavaType::Object("java/lang/Object".to_string()));
    }
    let logical_types = target.map_or_else(HashMap::new, |target| {
        params
            .iter()
            .zip(&target.parameter_types)
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect()
    });
    let mut field_table: HashMap<String, FieldInfo> = HashMap::new();
    for (name, ty) in captures {
        field_table.insert(
            name.clone(),
            FieldInfo {
                ty: ty.clone(),
                is_static: false,
                owner: class_name.to_string(),
            },
        );
    }
    let empty_sigs: HashMap<String, Vec<MethodSig>> = HashMap::new();
    let nested = RefCell::new(Vec::new());
    let (docall_code, docall_max_locals, docall_max_stack) = {
        let mut gen = MethodGen {
            code: CodeBuilder::new(),
            cp: &mut cp,
            class_internal: class_name,
            methods: &empty_sigs,
            registry,
            fields: &field_table,
            locals,
            logical_types,
            return_type: JavaType::Object("java/lang/Object".to_string()),
            loop_labels: Vec::new(),
            finally_blocks: Vec::new(),
            try_gaps: Vec::new(),
            lambdas: &nested,
        };
        match body {
            LambdaBody::Expression(expr) => {
                let result_ty = gen.expr(expr)?;
                box_to_object(&mut gen, &result_ty);
                gen.code.op(ARETURN);
            }
            LambdaBody::Block(statements) => {
                let falls_through = gen.stmt(&Statement::Block(statements.clone()))?;
                if falls_through {
                    // A void-compatible block has no value; the SAM dispatcher
                    // discards this closure-level null for void methods.
                    gen.code.op(0x01); // ACONST_NULL
                    gen.code.push(1);
                    gen.code.op(ARETURN);
                }
            }
        }
        let ml = gen.locals.max_slot.max(1);
        let ms = gen.code.max_stack().max(2) as u16;
        let (code, _exc) = gen.code.finish("doCall")?;
        (code, ml, ms)
    };

    let init_code = build_lambda_init(&mut cp, class_name, super_init_ref, captures);

    assemble_lambda_class(
        cp.get_entries(),
        class_name_idx,
        super_class_idx,
        &[
            (delegate_name_idx, object_desc_idx),
            (owner_name_idx, object_desc_idx),
            (resolve_name_idx, int_desc_idx),
        ],
        &cap_field_idx,
        code_attr_idx,
        (
            init_name_idx,
            init_desc_idx,
            &init_code,
            checked_u16_count(3 + captures.len(), "lambda local slots")?,
        ),
        (
            docall_name_idx,
            docall_desc_idx,
            &docall_code,
            docall_max_locals,
            docall_max_stack,
        ),
    )
}

/// Serialise a lambda closure `.class`. Mirrors the layout produced by the
/// Groovy closure generator so the interpreter loads it identically.
#[allow(clippy::too_many_arguments)]
fn assemble_lambda_class(
    cp_entries: &[crate::producer::ConstantPoolItem],
    this_class: u16,
    super_class: u16,
    std_fields: &[(u16, u16)],
    cap_fields: &[(u16, u16)],
    code_attr_idx: u16,
    init: (u16, u16, &[u8], u16),
    docall: (u16, u16, &[u8], u16, u16),
) -> BytecodeResult<Vec<u8>> {
    let mut b = Vec::new();
    b.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x00, 0x00, 0x41]); // magic + v65
    let cp_count = checked_u16_count(cp_entries.len() + 1, "lambda constant pool")?;
    b.extend_from_slice(&cp_count.to_be_bytes());
    for entry in cp_entries {
        write_constant_pool_entry(&mut b, entry).expect("cp entry");
    }
    b.extend_from_slice(&[0x00, 0x21]); // ACC_PUBLIC | ACC_SUPER
    b.extend_from_slice(&this_class.to_be_bytes());
    b.extend_from_slice(&super_class.to_be_bytes());
    b.extend_from_slice(&[0x00, 0x00]); // interfaces_count

    let fields_count = checked_u16_count(std_fields.len() + cap_fields.len(), "lambda fields")?;
    b.extend_from_slice(&fields_count.to_be_bytes());
    for (name_idx, desc_idx) in std_fields {
        b.extend_from_slice(&[0x00, 0x02]); // ACC_PRIVATE
        b.extend_from_slice(&name_idx.to_be_bytes());
        b.extend_from_slice(&desc_idx.to_be_bytes());
        b.extend_from_slice(&[0x00, 0x00]);
    }
    for (name_idx, desc_idx) in cap_fields {
        b.extend_from_slice(&[0x00, 0x12]); // ACC_PRIVATE | ACC_FINAL
        b.extend_from_slice(&name_idx.to_be_bytes());
        b.extend_from_slice(&desc_idx.to_be_bytes());
        b.extend_from_slice(&[0x00, 0x00]);
    }

    b.extend_from_slice(&[0x00, 0x02]); // methods_count = <init> + doCall

    let (init_name, init_desc, init_code, init_max_locals) = init;
    write_lambda_method(
        &mut b,
        code_attr_idx,
        init_name,
        init_desc,
        init_code,
        5,
        init_max_locals,
    );

    let (dc_name, dc_desc, dc_code, dc_max_locals, dc_max_stack) = docall;
    write_lambda_method(
        &mut b,
        code_attr_idx,
        dc_name,
        dc_desc,
        dc_code,
        dc_max_stack,
        dc_max_locals,
    );

    b.extend_from_slice(&[0x00, 0x00]); // class attributes_count
    Ok(b)
}

/// Write one `public` method with a single `Code` attribute (no exceptions).
fn write_lambda_method(
    b: &mut Vec<u8>,
    code_attr_idx: u16,
    name_idx: u16,
    desc_idx: u16,
    code: &[u8],
    max_stack: u16,
    max_locals: u16,
) {
    b.extend_from_slice(&[0x00, 0x01]); // ACC_PUBLIC
    b.extend_from_slice(&name_idx.to_be_bytes());
    b.extend_from_slice(&desc_idx.to_be_bytes());
    b.extend_from_slice(&[0x00, 0x01]); // attributes_count
    b.extend_from_slice(&code_attr_idx.to_be_bytes());
    let code_len = code.len() as u32;
    let attr_len = 12 + code_len;
    b.extend_from_slice(&attr_len.to_be_bytes());
    b.extend_from_slice(&max_stack.to_be_bytes());
    b.extend_from_slice(&max_locals.to_be_bytes());
    b.extend_from_slice(&code_len.to_be_bytes());
    b.extend_from_slice(code);
    b.extend_from_slice(&[0x00, 0x00]); // exception_table_length
    b.extend_from_slice(&[0x00, 0x00]); // attributes_count
}

/// The bridge methods of a class, as javac emits them. A class that overrides a supertype method at
/// a specialised type, such as `compareTo(A)` for `Comparable<A>`, is also called through the
/// supertype's erased descriptor, `compareTo(Object)`. A covariant return is bridged the same way.
/// The bridge casts its arguments and calls the declared method.
///
/// A supertype's `Object` parameter stands for a type variable only when the class names the
/// declared type as one of its type arguments (`supertype_arguments`). Without that, the `Object`
/// is real, as in `equals(Object)`, and matching it would bridge an overload.
fn bridge_methods(class: &ClassDecl, registry: &ClassRegistry) -> Vec<MethodDecl> {
    if class.is_interface || class.is_annotation {
        return Vec::new();
    }
    let declared: HashSet<(String, String)> = class
        .methods
        .iter()
        .map(|m| (m.name.clone(), m.descriptor()))
        .collect();
    let supertype_methods = inherited_supertype_methods(class, registry);
    let mut bridged: HashSet<(String, String)> = HashSet::new();
    let mut bridges = Vec::new();
    for m in class
        .methods
        .iter()
        .filter(|m| !m.is_static && m.name != "<init>")
    {
        let own_descriptor = m.descriptor();
        for (name, erased) in &supertype_methods {
            if *name != m.name || *erased == own_descriptor {
                continue;
            }
            let key = (name.clone(), erased.clone());
            if declared.contains(&key) || bridged.contains(&key) {
                continue;
            }
            let erased_params = parse_param_types(erased);
            let Some(erased_return) = descriptor_return_type(erased) else {
                continue;
            };
            if erased_params.len() != m.params.len() {
                continue;
            }
            let params_match = erased_params.iter().zip(&m.params).all(|(erased, param)| {
                *erased == param.ty
                    || (matches!(erased, JavaType::Object(n) if n == "java/lang/Object")
                        && class.supertype_arguments.contains(&param.ty))
            });
            let return_matches = erased_return == m.return_type
                || (is_reference_type(&erased_return) && is_reference_type(&m.return_type));
            if params_match && return_matches {
                bridged.insert(key);
                bridges.push(bridge_method(m, &erased_params, erased_return));
            }
        }
    }
    bridges
}

/// The non-static methods of the supertypes a class inherits from, as `(name, descriptor)`: its
/// superclasses other than Object, and every interface it implements directly, through a superclass,
/// or through another interface. Constructors are left out.
fn inherited_supertype_methods(
    class: &ClassDecl,
    registry: &ClassRegistry,
) -> Vec<(String, String)> {
    let jdk = crate::interpreter::JdkStubRegistry::new();
    let mut methods = Vec::new();
    let mut pending: Vec<String> = class.interfaces.clone();
    let mut super_name = class.super_name.clone();
    // The superclass chain ends at Object, or at a class this backend does not know.
    for _ in 0..64 {
        if super_name == "java/lang/Object" {
            break;
        }
        if let Some(info) = registry.get(&super_name) {
            pending.extend(info.interfaces.iter().cloned());
            push_unit_methods(info, &mut methods);
            super_name = info.super_name.clone();
        } else if let Some(stub) = jdk.get_class(&super_name) {
            pending.extend(stub.interfaces.iter().cloned());
            push_stub_methods(stub, &mut methods);
            match &stub.superclass {
                Some(next) => super_name = next.clone(),
                None => break,
            }
        } else {
            break;
        }
    }
    let mut seen = HashSet::new();
    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(info) = registry.get(&name) {
            pending.extend(info.interfaces.iter().cloned());
            push_unit_methods(info, &mut methods);
        } else if let Some(stub) = jdk.get_class(&name) {
            pending.extend(stub.interfaces.iter().cloned());
            push_stub_methods(stub, &mut methods);
        }
    }
    methods
}

/// Append the non-static methods a unit class declares, as `(name, descriptor)`.
fn push_unit_methods(info: &ClassInfo, methods: &mut Vec<(String, String)>) {
    for (name, sigs) in &info.methods {
        for sig in sigs.iter().filter(|sig| !sig.is_static) {
            methods.push((name.clone(), sig.descriptor.clone()));
        }
    }
}

/// Append the non-static, non-constructor methods a JDK stub declares, as `(name, descriptor)`.
fn push_stub_methods(stub: &crate::interpreter::StubClass, methods: &mut Vec<(String, String)>) {
    for method in stub.methods.values() {
        if !method.is_static && !method.is_constructor {
            methods.push((method.name.clone(), method.descriptor.clone()));
        }
    }
}

/// The method a bridge with the erased parameter and return types forwards to. Its parameters are
/// cast to the declared parameter types, and its body is `return this.m((T) p0, ...)`.
fn bridge_method(
    target: &MethodDecl,
    erased_params: &[JavaType],
    erased_return: JavaType,
) -> MethodDecl {
    let params: Vec<Param> = erased_params
        .iter()
        .enumerate()
        .map(|(i, ty)| Param {
            name: format!("p{i}"),
            ty: ty.clone(),
        })
        .collect();
    let args: Vec<Expression> = params
        .iter()
        .zip(&target.params)
        .map(|(param, declared)| {
            let arg = Expression::NameExpr(param.name.clone());
            if param.ty == declared.ty {
                arg
            } else {
                Expression::Cast {
                    target_type: declared.ty.clone(),
                    expr: Box::new(arg),
                }
            }
        })
        .collect();
    let call = Expression::MethodCall {
        receiver: Some(Box::new(Expression::This)),
        method: target.name.clone(),
        args,
    };
    let body = if erased_return == JavaType::Void {
        vec![Statement::Expression(call), Statement::Return(None)]
    } else {
        vec![Statement::Return(Some(call))]
    };
    MethodDecl {
        is_abstract: false,
        name: target.name.clone(),
        params,
        return_type: erased_return,
        is_static: false,
        body,
    }
}

/// Method signatures for one class, keyed by name → overloads.
fn class_method_sigs(class: &ClassDecl) -> HashMap<String, Vec<MethodSig>> {
    let mut sigs: HashMap<String, Vec<MethodSig>> = HashMap::new();
    for m in &class.methods {
        sigs.entry(m.name.clone()).or_default().push(MethodSig {
            descriptor: m.descriptor(),
            return_type: m.return_type.clone(),
            is_static: m.is_static,
            params: m.params.iter().map(|p| p.ty.clone()).collect(),
            owner: class.name.clone(),
        });
    }
    sigs
}

/// Build the class registry (super links, methods, fields) for a unit.
fn build_registry(classes: &[ClassDecl]) -> ClassRegistry {
    classes
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                ClassInfo {
                    super_name: c.super_name.clone(),
                    methods: class_method_sigs(c),
                    fields: c
                        .fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone(), f.is_static))
                        .collect(),
                    is_interface: c.is_interface,
                    interfaces: c.interfaces.clone(),
                },
            )
        })
        .collect()
}

/// Build the field table for `class`, including fields inherited from in-unit
/// superclasses. A subclass field shadows a same-named super field. `owner` is
/// the declaring class, used as the fieldref owner.
fn build_field_table(class: &ClassDecl, registry: &ClassRegistry) -> HashMap<String, FieldInfo> {
    let mut table: HashMap<String, FieldInfo> = HashMap::new();
    let mut cursor = class.name.clone();
    let mut seen = std::collections::HashSet::new();
    while seen.insert(cursor.clone()) {
        let Some(info) = registry.get(&cursor) else {
            break;
        };
        for (name, ty, is_static) in &info.fields {
            table.entry(name.clone()).or_insert_with(|| FieldInfo {
                ty: ty.clone(),
                is_static: *is_static,
                owner: cursor.clone(),
            });
        }
        if info.super_name == cursor {
            break;
        }
        cursor = info.super_name.clone();
    }
    // A nested class also names the static fields of the classes around it. Its own and
    // inherited fields shadow them, which `or_insert_with` keeps.
    for (owner, field) in &class.enclosing_static_fields {
        table
            .entry(field.name.clone())
            .or_insert_with(|| FieldInfo {
                ty: field.ty.clone(),
                is_static: true,
                owner: owner.clone(),
            });
    }
    table
}

/// Compile a single class with a registry of just itself (no siblings).
pub fn compile_class(class: &ClassDecl) -> BytecodeResult<Vec<u8>> {
    let registry = build_registry(std::slice::from_ref(class));
    let lambdas = RefCell::new(Vec::new());
    compile_class_in_unit(class, &registry, &lambdas)
}

/// Compile every class in a compilation unit, sharing one signature registry so
/// cross-class calls resolve. Returns `(name, result)` per class. Synthetic
/// lambda closure classes generated during compilation are appended.
pub fn compile_unit(classes: &[ClassDecl]) -> Vec<(String, BytecodeResult<Vec<u8>>)> {
    let registry = build_registry(classes);
    let lambdas = RefCell::new(Vec::new());
    let mut out: Vec<(String, BytecodeResult<Vec<u8>>)> = classes
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                compile_class_in_unit(c, &registry, &lambdas),
            )
        })
        .collect();
    for (name, bytes) in lambdas.into_inner() {
        out.push((name, Ok(bytes)));
    }
    out
}

/// Compile `class`, resolving cross-class calls against `registry` (all classes
/// in the compilation unit).
/// JDK classes the interpreter represents natively. An instance of one is a heap collection, not an
/// object, so a user class cannot extend one: its inherited methods would run on an object that is
/// not a collection. Keep in step with the allocation arms in interpreter/executor/handlers/objects.rs.
const NATIVELY_REPRESENTED_JDK_CLASSES: &[&str] = &[
    "java/util/ArrayList",
    "java/util/HashMap",
    "java/util/LinkedHashMap",
    "java/util/TreeMap",
    "java/util/TreeSet",
    "java/util/Properties",
    "java/util/Hashtable",
    "java/util/HashSet",
    "java/util/LinkedHashSet",
    "java/util/LinkedList",
];

/// Refuse a class whose superclass chain, through the unit, reaches a natively represented JDK class.
fn check_superclass_is_not_native(
    class: &ClassDecl,
    registry: &ClassRegistry,
) -> BytecodeResult<()> {
    let mut current = class.super_name.clone();
    for _ in 0..64 {
        if let Some(native) = NATIVELY_REPRESENTED_JDK_CLASSES
            .iter()
            .find(|native| **native == current)
        {
            return Err(BytecodeError::UnsupportedFeature {
                feature: format!(
                    "class {} extends {}, whose instances the interpreter represents natively",
                    class.name.replace('/', "."),
                    native.replace('/', ".")
                ),
            });
        }
        match registry.get(&current) {
            Some(info) => current = info.super_name.clone(),
            None => return Ok(()),
        }
    }
    Ok(())
}

fn compile_class_in_unit(
    class: &ClassDecl,
    registry: &ClassRegistry,
    lambdas: &RefCell<Vec<(String, Vec<u8>)>>,
) -> BytecodeResult<Vec<u8>> {
    check_superclass_is_not_native(class, registry)?;
    let mut cp = ConstantPoolBuilder::new();

    // The method signature table for this class (resolving self-calls/recursion).
    let sigs = class_method_sigs(class);

    // Field table (self + inherited) for get/putfield, get/putstatic, inference.
    let field_types = build_field_table(class, registry);

    let class_name_idx = cp.add_class(&class.name);
    let super_idx = cp.add_class(&class.super_name);
    let interface_idxs: Vec<u16> = class
        .interfaces
        .iter()
        .map(|name| cp.add_class(name))
        .collect();

    let super_init = cp.add_method_ref(&class.super_name, "<init>", "()V");
    let init_name_idx = cp.add_utf8("<init>");
    let init_desc_idx = cp.add_utf8("()V");
    let code_attr_idx = cp.add_utf8("Code");

    // Members the initializers need: a `<clinit>` for the static initializers, and a default
    // constructor when none is declared but the instance initializers must run.
    let mut methods = class.methods.clone();
    if !class.static_init.is_empty() {
        methods.push(MethodDecl {
            is_abstract: false,
            name: "<clinit>".to_string(),
            params: Vec::new(),
            return_type: JavaType::Void,
            is_static: true,
            body: class.static_init.clone(),
        });
    }
    if !methods.iter().any(|m| m.name == "<init>") && !class.instance_init.is_empty() {
        methods.push(MethodDecl {
            is_abstract: false,
            name: "<init>".to_string(),
            params: Vec::new(),
            return_type: JavaType::Void,
            is_static: false,
            body: Vec::new(),
        });
    }
    let has_user_ctor = methods.iter().any(|m| m.name == "<init>");

    // Compile each user method first (this populates the constant pool).
    struct CompiledMethod {
        access: u16,
        name_idx: u16,
        desc_idx: u16,
        max_stack: u16,
        max_locals: u16,
        code: Vec<u8>,
        exceptions: Vec<ExceptionEntry>,
        /// An abstract method has no code and no Code attribute.
        is_abstract: bool,
    }
    let mut compiled = Vec::new();

    // Bridge methods carry their own flag, so the loop below can mark them.
    let bridges = bridge_methods(class, registry);
    for (m, is_bridge) in methods
        .iter()
        .map(|m| (m, false))
        .chain(bridges.iter().map(|m| (m, true)))
    {
        if m.is_abstract {
            // ACC_PUBLIC | ACC_ABSTRACT, with the name and descriptor and nothing else.
            compiled.push(CompiledMethod {
                access: 0x0401,
                name_idx: cp.add_utf8(&m.name),
                desc_idx: cp.add_utf8(&m.descriptor()),
                max_stack: 0,
                max_locals: 0,
                code: Vec::new(),
                exceptions: Vec::new(),
                is_abstract: true,
            });
            continue;
        }
        let is_ctor = m.name == "<init>";
        let mut locals = Locals::new();
        // `this` occupies slot 0 for instance methods and constructors.
        if !m.is_static {
            locals.declare("this", JavaType::Object(class.name.clone()));
        }
        for p in &m.params {
            locals.declare(&p.name, p.ty.clone());
        }

        let mut gen = MethodGen {
            code: CodeBuilder::new(),
            cp: &mut cp,
            class_internal: &class.name,
            methods: &sigs,
            registry,
            fields: &field_types,
            locals,
            logical_types: HashMap::new(),
            return_type: m.return_type.clone(),
            loop_labels: Vec::new(),
            finally_blocks: Vec::new(),
            try_gaps: Vec::new(),
            lambdas,
        };

        // A constructor must begin by invoking another constructor. If the source
        // wrote an explicit `this(...)`/`super(...)` (always the first statement),
        // that invocation chains for us — `gen.stmt` emits it. Otherwise the
        // compiler synthesizes the implicit no-arg `super()` call here.
        let has_explicit_ctor_call = is_ctor
            && matches!(
                m.body.first(),
                Some(Statement::ExplicitConstructorCall { .. })
            );
        if is_ctor && !has_explicit_ctor_call {
            gen.code.op(0x2a); // aload_0
            gen.code.push(1);
            gen.code.op(INVOKESPECIAL);
            gen.code.u16(super_init);
            gen.code.pop(1);
        }

        // Instance initializers run after the superclass constructor, and not in a constructor that
        // delegates to another with `this(...)`.
        let delegates = is_ctor
            && matches!(
                m.body.first(),
                Some(Statement::ExplicitConstructorCall {
                    is_super: false,
                    ..
                })
            );
        let body: Vec<Statement> = if is_ctor && !delegates {
            let (head, tail) = m.body.split_at(usize::from(has_explicit_ctor_call));
            head.iter()
                .chain(class.instance_init.iter())
                .chain(tail.iter())
                .cloned()
                .collect()
        } else {
            m.body.clone()
        };

        let mut falls = true;
        for st in &body {
            falls = gen.stmt(st)?;
        }
        // Void methods and constructors that can fall off the end must return.
        if falls && matches!(m.return_type, JavaType::Void) {
            gen.code.op(RETURN);
        }

        let max_stack = gen.code.max_stack().max(2) as u16;
        let max_locals = gen.locals.max_slot.max(1);
        let (code, exceptions) = gen.code.finish(&m.name)?;

        let name_idx = cp.add_utf8(&m.name);
        let desc_idx = cp.add_utf8(&m.descriptor());
        let mut access = 0x0001; // ACC_PUBLIC
        if is_bridge {
            access |= 0x1040; // ACC_BRIDGE | ACC_SYNTHETIC
        }
        if m.is_static {
            access |= 0x0008; // ACC_STATIC
        }

        compiled.push(CompiledMethod {
            access,
            name_idx,
            desc_idx,
            max_stack,
            max_locals,
            code,
            exceptions,
            is_abstract: false,
        });
    }

    // Field constant-pool entries (name + descriptor utf8), computed before the
    // pool is frozen for serialization.
    let field_entries: Vec<(u16, u16, bool)> = class
        .fields
        .iter()
        .map(|f| {
            let n = cp.add_utf8(&f.name);
            let d = cp.add_utf8(&f.ty.descriptor());
            (n, d, f.is_static)
        })
        .collect();

    // ── Serialize the class file ──────────────────────────────────────────
    let mut out = Vec::new();
    out.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]); // magic
    out.extend_from_slice(&[0x00, 0x00]); // minor
    out.extend_from_slice(&[0x00, 0x34]); // major = 52 (Java 8)

    let entries = cp.get_entries();
    let cp_count = checked_u16_count(entries.len() + 1, "constant pool")?;
    out.extend_from_slice(&cp_count.to_be_bytes());
    // CONSTANT_Long / CONSTANT_Double occupy two pool slots: the entry itself
    // plus an unusable phantom slot. The builder stores a placeholder entry for
    // that phantom slot to keep indices aligned, but no bytes must be written
    // for it — skip the entry immediately following a Long/Double.
    let mut skip_phantom = false;
    for entry in entries {
        if skip_phantom {
            skip_phantom = false;
            continue;
        }
        write_constant_pool_entry(&mut out, entry)?;
        skip_phantom = matches!(
            entry,
            crate::producer::ConstantPoolItem::Long(_)
                | crate::producer::ConstantPoolItem::Double(_)
        );
    }

    // ACC_PUBLIC | ACC_SUPER for a class, with ACC_ABSTRACT when it is abstract.
    // ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT for an interface.
    let class_access: u16 = if class.is_annotation {
        0x2601 // ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT | ACC_ANNOTATION
    } else if class.is_interface {
        0x0601
    } else if class.is_abstract() {
        0x0421
    } else {
        0x0021
    };
    out.extend_from_slice(&class_access.to_be_bytes());
    out.extend_from_slice(&class_name_idx.to_be_bytes());
    out.extend_from_slice(&super_idx.to_be_bytes());
    out.extend_from_slice(&checked_u16_count(interface_idxs.len(), "interfaces")?.to_be_bytes());
    for idx in &interface_idxs {
        out.extend_from_slice(&idx.to_be_bytes());
    }

    // fields
    out.extend_from_slice(&checked_u16_count(field_entries.len(), "fields")?.to_be_bytes());
    for (name_idx, desc_idx, is_static) in &field_entries {
        // An interface field is public static final. A class field is public, and static if declared so.
        let access: u16 = if class.is_interface {
            0x0019
        } else if *is_static {
            0x0009
        } else {
            0x0001
        };
        out.extend_from_slice(&access.to_be_bytes());
        out.extend_from_slice(&name_idx.to_be_bytes());
        out.extend_from_slice(&desc_idx.to_be_bytes());
        out.extend_from_slice(&[0x00, 0x00]); // attributes_count
    }

    // methods_count = user methods (+ synthesized <init> only when none defined)
    // An interface has no constructor to synthesize.
    let synth_init = !has_user_ctor && !class.is_interface;
    let methods_count = checked_u16_count(compiled.len() + usize::from(synth_init), "methods")?;
    out.extend_from_slice(&methods_count.to_be_bytes());

    // Synthesized default <init> (omitted when the source declares a constructor).
    if synth_init {
        out.extend_from_slice(&0x0001u16.to_be_bytes()); // ACC_PUBLIC
        out.extend_from_slice(&init_name_idx.to_be_bytes());
        out.extend_from_slice(&init_desc_idx.to_be_bytes());
        out.extend_from_slice(&[0x00, 0x01]); // attributes_count
        out.extend_from_slice(&code_attr_idx.to_be_bytes());
        out.extend_from_slice(&17u32.to_be_bytes()); // attribute_length
        out.extend_from_slice(&1u16.to_be_bytes()); // max_stack
        out.extend_from_slice(&1u16.to_be_bytes()); // max_locals
        out.extend_from_slice(&5u32.to_be_bytes()); // code_length
        out.push(0x2a); // aload_0
        out.push(INVOKESPECIAL);
        out.extend_from_slice(&super_init.to_be_bytes());
        out.push(RETURN);
        out.extend_from_slice(&[0x00, 0x00]); // exception_table_length
        out.extend_from_slice(&[0x00, 0x00]); // attributes_count
    }

    // User methods
    for m in &compiled {
        out.extend_from_slice(&m.access.to_be_bytes());
        out.extend_from_slice(&m.name_idx.to_be_bytes());
        out.extend_from_slice(&m.desc_idx.to_be_bytes());
        if m.is_abstract {
            out.extend_from_slice(&[0x00, 0x00]); // attributes_count: none, there is no code
            continue;
        }
        out.extend_from_slice(&[0x00, 0x01]); // attributes_count (Code)
        out.extend_from_slice(&code_attr_idx.to_be_bytes());
        let code_len = m.code.len() as u32;
        // Code attribute = max_stack(2)+max_locals(2)+code_length(4)+code
        //   + exception_table_length(2) + 8·entries + attributes_count(2).
        let exc_bytes = 8 * m.exceptions.len() as u32;
        let attr_len = 12 + code_len + exc_bytes;
        out.extend_from_slice(&attr_len.to_be_bytes());
        out.extend_from_slice(&m.max_stack.to_be_bytes());
        out.extend_from_slice(&m.max_locals.to_be_bytes());
        out.extend_from_slice(&code_len.to_be_bytes());
        out.extend_from_slice(&m.code);
        out.extend_from_slice(
            &checked_u16_count(m.exceptions.len(), "exception table")?.to_be_bytes(),
        );
        for e in &m.exceptions {
            out.extend_from_slice(&e.start_pc.to_be_bytes());
            out.extend_from_slice(&e.end_pc.to_be_bytes());
            out.extend_from_slice(&e.handler_pc.to_be_bytes());
            out.extend_from_slice(&e.catch_type.to_be_bytes());
        }
        out.extend_from_slice(&[0x00, 0x00]); // attributes_count
    }

    out.extend_from_slice(&[0x00, 0x00]); // class attributes_count
    Ok(out)
}
