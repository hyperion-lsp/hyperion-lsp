// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Expression evaluation against a suspended frame.
//!
//! Names resolve to locals, then fields of `this`, then static fields, then
//! classes and packages. Method calls run in the debuggee through JDWP
//! `InvokeMethod` on the frame's thread with `INVOKE_SINGLE_THREADED`, so no
//! other thread runs while the call executes. Arithmetic, comparison, logic
//! and string concatenation are computed in the adapter.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use hyperion_jdwp::commands::Command;
use hyperion_jdwp::replies::{self, tag, JdwpValue, MethodInfo};

use super::expr::{self, BinaryOp, Expr, Literal, UnaryOp};
use super::presentation::{jni_sig_to_display, sig_to_tag};
use super::state::ScopeInfo;
use super::DebugSession;
use crate::types::{EvaluateArguments, EvaluateResponseBody};

/// Longest a user-requested method call may run in the debuggee.
const INVOKE_TIMEOUT: Duration = Duration::from_secs(5);
const ACC_STATIC: i32 = 0x0008;
const ACC_ABSTRACT: i32 = 0x0400;

/// A value produced while evaluating an expression.
#[derive(Debug, Clone)]
pub(super) enum EvalValue {
    /// A primitive, `null` or a reference living in the debuggee.
    Jdwp(JdwpValue),
    /// A string built by the adapter (a literal or a concatenation) that has
    /// not been created in the debuggee.
    Str(String),
    /// A loaded class, usable as the receiver of a static access.
    Class { type_id: u64, signature: String },
    /// A dotted prefix that may turn out to be a package (`java.util`).
    Package(String),
}

/// A primitive value with Java's numeric promotion rules applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Prim {
    Bool(bool),
    Char(u16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

impl Prim {
    const fn rank(self) -> u8 {
        match self {
            Self::Bool(_) => 0,
            Self::Char(_) | Self::Int(_) => 1,
            Self::Long(_) => 2,
            Self::Float(_) => 3,
            Self::Double(_) => 4,
        }
    }

    fn as_i64(self) -> i64 {
        match self {
            Self::Bool(b) => i64::from(b),
            Self::Char(c) => i64::from(c),
            Self::Int(i) => i64::from(i),
            Self::Long(l) => l,
            Self::Float(f) => f as i64,
            Self::Double(d) => d as i64,
        }
    }

    fn as_f64(self) -> f64 {
        match self {
            Self::Bool(b) => f64::from(u8::from(b)),
            Self::Char(c) => f64::from(c),
            Self::Int(i) => f64::from(i),
            Self::Long(l) => l as f64,
            Self::Float(f) => f64::from(f),
            Self::Double(d) => d,
        }
    }

    fn as_f32(self) -> f32 {
        match self {
            Self::Bool(b) => f32::from(u8::from(b)),
            Self::Char(c) => f32::from(c),
            Self::Int(i) => i as f32,
            Self::Long(l) => l as f32,
            Self::Float(f) => f,
            Self::Double(d) => d as f32,
        }
    }

    pub(super) const fn to_jdwp(self) -> JdwpValue {
        match self {
            Self::Bool(b) => JdwpValue::Boolean(b),
            Self::Char(c) => JdwpValue::Char(c),
            Self::Int(i) => JdwpValue::Int(i),
            Self::Long(l) => JdwpValue::Long(l),
            Self::Float(f) => JdwpValue::Float(f),
            Self::Double(d) => JdwpValue::Double(d),
        }
    }

    pub(super) fn from_jdwp(value: &JdwpValue) -> Option<Self> {
        Some(match value {
            JdwpValue::Boolean(b) => Self::Bool(*b),
            JdwpValue::Char(c) => Self::Char(*c),
            JdwpValue::Byte(b) => Self::Int(i32::from(*b)),
            JdwpValue::Short(s) => Self::Int(i32::from(*s)),
            JdwpValue::Int(i) => Self::Int(*i),
            JdwpValue::Long(l) => Self::Long(*l),
            JdwpValue::Float(f) => Self::Float(*f),
            JdwpValue::Double(d) => Self::Double(*d),
            _ => return None,
        })
    }

    /// Java's string conversion of a primitive.
    pub(super) fn display(self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
            Self::Char(c) => {
                char::from_u32(u32::from(c)).map_or_else(|| "?".to_owned(), String::from)
            }
            Self::Int(i) => i.to_string(),
            Self::Long(l) => l.to_string(),
            Self::Float(f) => java_float(f64::from(f), f.to_string()),
            Self::Double(d) => java_float(d, d.to_string()),
        }
    }
}

fn java_float(value: f64, rust: String) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
    } else if value.fract() == 0.0 && !rust.contains('.') && !rust.contains('e') {
        format!("{rust}.0")
    } else {
        rust
    }
}

/// Apply a binary arithmetic or comparison operator with Java semantics.
pub(super) fn apply_binary(op: BinaryOp, a: Prim, b: Prim) -> Result<Prim, String> {
    use BinaryOp::{Add, And, Div, Eq, Ge, Gt, Le, Lt, Mul, Ne, Or, Rem, Sub};
    if let (Prim::Bool(x), Prim::Bool(y)) = (a, b) {
        return match op {
            And => Ok(Prim::Bool(x && y)),
            Or => Ok(Prim::Bool(x || y)),
            Eq => Ok(Prim::Bool(x == y)),
            Ne => Ok(Prim::Bool(x != y)),
            _ => Err("operator is not defined for booleans".to_owned()),
        };
    }
    if matches!(a, Prim::Bool(_)) || matches!(b, Prim::Bool(_)) {
        return Err("cannot mix a boolean with a number".to_owned());
    }
    let rank = a.rank().max(b.rank());
    if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) {
        let ordering = match rank {
            4 => a.as_f64().partial_cmp(&b.as_f64()),
            3 => a.as_f32().partial_cmp(&b.as_f32()),
            _ => Some(a.as_i64().cmp(&b.as_i64())),
        };
        use std::cmp::Ordering::{Equal, Greater, Less};
        let result = match (op, ordering) {
            (Eq, o) => o == Some(Equal),
            (Ne, o) => o != Some(Equal),
            (Lt, o) => o == Some(Less),
            (Le, o) => matches!(o, Some(Less | Equal)),
            (Gt, o) => o == Some(Greater),
            (Ge, o) => matches!(o, Some(Greater | Equal)),
            _ => unreachable!("comparison operators are matched above"),
        };
        return Ok(Prim::Bool(result));
    }
    if rank <= 2 && matches!(op, Div | Rem) && b.as_i64() == 0 {
        return Err("/ by zero".to_owned());
    }
    Ok(match rank {
        1 => {
            let (x, y) = (a.as_i64() as i32, b.as_i64() as i32);
            Prim::Int(match op {
                Add => x.wrapping_add(y),
                Sub => x.wrapping_sub(y),
                Mul => x.wrapping_mul(y),
                Div => x.wrapping_div(y),
                Rem => x.wrapping_rem(y),
                _ => return Err("operator needs boolean operands".to_owned()),
            })
        }
        2 => {
            let (x, y) = (a.as_i64(), b.as_i64());
            Prim::Long(match op {
                Add => x.wrapping_add(y),
                Sub => x.wrapping_sub(y),
                Mul => x.wrapping_mul(y),
                Div => x.wrapping_div(y),
                Rem => x.wrapping_rem(y),
                _ => return Err("operator needs boolean operands".to_owned()),
            })
        }
        3 => {
            let (x, y) = (a.as_f32(), b.as_f32());
            Prim::Float(match op {
                Add => x + y,
                Sub => x - y,
                Mul => x * y,
                Div => x / y,
                Rem => x % y,
                _ => return Err("operator needs boolean operands".to_owned()),
            })
        }
        _ => {
            let (x, y) = (a.as_f64(), b.as_f64());
            Prim::Double(match op {
                Add => x + y,
                Sub => x - y,
                Mul => x * y,
                Div => x / y,
                Rem => x % y,
                _ => return Err("operator needs boolean operands".to_owned()),
            })
        }
    })
}

/// Split a method signature's parameter list: `(I[JLjava/lang/String;)V`.
pub(super) fn method_parameters(signature: &str) -> Vec<String> {
    let mut params = Vec::new();
    let inner = signature
        .strip_prefix('(')
        .and_then(|rest| rest.split(')').next())
        .unwrap_or("");
    let bytes: Vec<char> = inner.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        while bytes[i] == '[' {
            i += 1;
        }
        if bytes[i] == 'L' {
            while bytes[i] != ';' {
                i += 1;
            }
        }
        i += 1;
        params.push(bytes[start..i].iter().collect());
    }
    params
}

fn box_signature(prim_sig: &str) -> Option<&'static str> {
    Some(match prim_sig {
        "Z" => "Ljava/lang/Boolean;",
        "B" => "Ljava/lang/Byte;",
        "C" => "Ljava/lang/Character;",
        "S" => "Ljava/lang/Short;",
        "I" => "Ljava/lang/Integer;",
        "J" => "Ljava/lang/Long;",
        "F" => "Ljava/lang/Float;",
        "D" => "Ljava/lang/Double;",
        _ => return None,
    })
}

pub(super) fn unbox_signature(signature: &str) -> Option<&'static str> {
    Some(match signature {
        "Ljava/lang/Boolean;" => "Z",
        "Ljava/lang/Byte;" => "B",
        "Ljava/lang/Character;" => "C",
        "Ljava/lang/Short;" => "S",
        "Ljava/lang/Integer;" => "I",
        "Ljava/lang/Long;" => "J",
        "Ljava/lang/Float;" => "F",
        "Ljava/lang/Double;" => "D",
        _ => return None,
    })
}

const fn prim_signature(prim: Prim) -> &'static str {
    match prim {
        Prim::Bool(_) => "Z",
        Prim::Char(_) => "C",
        Prim::Int(_) => "I",
        Prim::Long(_) => "J",
        Prim::Float(_) => "F",
        Prim::Double(_) => "D",
    }
}

/// Can a primitive of signature `from` widen to `to` (JLS 5.1.2)?
fn widens(from: &str, to: &str) -> bool {
    const ORDER: &str = "BSIJFD";
    if from == to {
        return true;
    }
    match (from, to) {
        ("C", "I" | "J" | "F" | "D") => true,
        ("Z" | "C", _) | (_, "Z" | "C") => false,
        _ => match (ORDER.find(from), ORDER.find(to)) {
            (Some(a), Some(b)) => a < b,
            _ => false,
        },
    }
}

/// Convert a primitive to the primitive type named by `target` (JNI letter).
fn convert_prim(prim: Prim, target: &str) -> Option<JdwpValue> {
    Some(match target {
        "Z" => match prim {
            Prim::Bool(b) => JdwpValue::Boolean(b),
            _ => return None,
        },
        "B" => JdwpValue::Byte(prim.as_i64() as i8),
        "S" => JdwpValue::Short(prim.as_i64() as i16),
        "C" => JdwpValue::Char(prim.as_i64() as u16),
        "I" => JdwpValue::Int(prim.as_i64() as i32),
        "J" => JdwpValue::Long(prim.as_i64()),
        "F" => JdwpValue::Float(prim.as_f32()),
        "D" => JdwpValue::Double(prim.as_f64()),
        _ => return None,
    })
}

type EvalFuture<'a> = Pin<Box<dyn Future<Output = Result<EvalValue, String>> + Send + 'a>>;

impl DebugSession {
    // ---- Handler: evaluate ----

    pub async fn handle_evaluate(
        &mut self,
        args: EvaluateArguments,
    ) -> Result<EvaluateResponseBody, String> {
        let key = args.frame_id.ok_or("evaluate requires a frame_id")?;
        let scope = self.scope(key)?;
        let expression = expr::parse(&args.expression)?;
        self.allow_invoke = args.context.as_deref() != Some("hover");
        if !self.allow_invoke && expression.invokes_methods() {
            return Err(
                "method calls are not run for hovers; evaluate the expression in the debug console"
                    .to_owned(),
            );
        }
        // Code run in the debuggee must not stop at breakpoints or exceptions
        // (the call would never return): take them out for the duration.
        self.suspend_stopping_events().await;
        let outcome = async {
            let value = self.eval(&expression, key).await?;
            self.present_eval_value(value, scope.thread_id).await
        }
        .await;
        self.resume_stopping_events().await;
        let presented = outcome?;
        Ok(EvaluateResponseBody {
            result: presented.text,
            type_: presented.type_name,
            variables_reference: presented.variables_reference,
        })
    }

    fn scope(&self, key: u64) -> Result<ScopeInfo, String> {
        self.variables
            .scopes
            .get(&key)
            .copied()
            .ok_or_else(|| "unknown frame_id for evaluate".to_owned())
    }

    fn eval<'a>(&'a mut self, expression: &'a Expr, key: u64) -> EvalFuture<'a> {
        Box::pin(async move {
            match expression {
                Expr::Literal(literal) => Ok(match literal {
                    Literal::Int(v) => EvalValue::Jdwp(JdwpValue::Int(*v)),
                    Literal::Long(v) => EvalValue::Jdwp(JdwpValue::Long(*v)),
                    Literal::Float(v) => EvalValue::Jdwp(JdwpValue::Float(*v)),
                    Literal::Double(v) => EvalValue::Jdwp(JdwpValue::Double(*v)),
                    Literal::Bool(v) => EvalValue::Jdwp(JdwpValue::Boolean(*v)),
                    Literal::Char(v) => EvalValue::Jdwp(JdwpValue::Char(*v)),
                    Literal::Str(v) => EvalValue::Str(v.clone()),
                    Literal::Null => EvalValue::Jdwp(JdwpValue::Null),
                }),
                Expr::This => self.this_value(key).await.map(EvalValue::Jdwp),
                Expr::Name(name) => self.resolve_name(name, key).await,
                Expr::Field(base, name) => {
                    let base = self.eval(base, key).await?;
                    self.select_field(base, name, key).await
                }
                Expr::Index(array, index) => {
                    let array = self.eval(array, key).await?;
                    let index = self.eval(index, key).await?;
                    self.index_array(array, index, key).await
                }
                Expr::Call(receiver, name, args) => {
                    self.call_method(receiver.as_deref(), name, args, key).await
                }
                Expr::Unary(op, operand) => {
                    let operand = self.eval(operand, key).await?;
                    let prim = self.require_prim(&operand, key).await?;
                    match (op, prim) {
                        (UnaryOp::Not, Prim::Bool(b)) => {
                            Ok(EvalValue::Jdwp(JdwpValue::Boolean(!b)))
                        }
                        (UnaryOp::Not, _) => Err("'!' needs a boolean".to_owned()),
                        (UnaryOp::Neg, Prim::Bool(_)) => Err("'-' needs a number".to_owned()),
                        (UnaryOp::Neg, Prim::Double(d)) => {
                            Ok(EvalValue::Jdwp(JdwpValue::Double(-d)))
                        }
                        (UnaryOp::Neg, Prim::Float(f)) => Ok(EvalValue::Jdwp(JdwpValue::Float(-f))),
                        (UnaryOp::Neg, Prim::Long(l)) => {
                            Ok(EvalValue::Jdwp(JdwpValue::Long(l.wrapping_neg())))
                        }
                        (UnaryOp::Neg, other) => Ok(EvalValue::Jdwp(JdwpValue::Int(
                            (other.as_i64() as i32).wrapping_neg(),
                        ))),
                    }
                }
                Expr::Binary(op, left, right) => self.binary(*op, left, right, key).await,
                Expr::Conditional(condition, then, otherwise) => {
                    let condition = self.eval(condition, key).await?;
                    match self.require_prim(&condition, key).await? {
                        Prim::Bool(true) => self.eval(then, key).await,
                        Prim::Bool(false) => self.eval(otherwise, key).await,
                        _ => Err("condition of '?:' must be a boolean".to_owned()),
                    }
                }
            }
        })
    }

    async fn binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        key: u64,
    ) -> Result<EvalValue, String> {
        let a = self.eval(left, key).await?;
        // Short-circuit logic.
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            let Prim::Bool(lhs) = self.require_prim(&a, key).await? else {
                return Err("logical operators need booleans".to_owned());
            };
            if (op == BinaryOp::And && !lhs) || (op == BinaryOp::Or && lhs) {
                return Ok(EvalValue::Jdwp(JdwpValue::Boolean(lhs)));
            }
            let b = self.eval(right, key).await?;
            let Prim::Bool(rhs) = self.require_prim(&b, key).await? else {
                return Err("logical operators need booleans".to_owned());
            };
            return Ok(EvalValue::Jdwp(JdwpValue::Boolean(rhs)));
        }
        let b = self.eval(right, key).await?;

        // String concatenation: `+` with a String on either side.
        if op == BinaryOp::Add && (self.is_string(&a) || self.is_string(&b)) {
            let thread = self.scope(key)?.thread_id;
            let left = self.display_string(&a, thread).await?;
            let right = self.display_string(&b, thread).await?;
            return Ok(EvalValue::Str(format!("{left}{right}")));
        }

        // `==` / `!=` on strings compares contents (what a debugger user means).
        if matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
            if let Some(equal) = self.reference_equality(&a, &b).await {
                let result = if op == BinaryOp::Eq { equal } else { !equal };
                return Ok(EvalValue::Jdwp(JdwpValue::Boolean(result)));
            }
        }

        let x = self.require_prim(&a, key).await?;
        let y = self.require_prim(&b, key).await?;
        Ok(EvalValue::Jdwp(apply_binary(op, x, y)?.to_jdwp()))
    }

    /// Equality of two references, `null`s and string contents. `None` when
    /// either side is (or unboxes to) a primitive.
    async fn reference_equality(&mut self, a: &EvalValue, b: &EvalValue) -> Option<bool> {
        let is_reference = |v: &EvalValue| {
            matches!(
                v,
                EvalValue::Str(_) | EvalValue::Jdwp(JdwpValue::Null | JdwpValue::Object { .. })
            )
        };
        if !is_reference(a) || !is_reference(b) {
            return None;
        }
        // A boxed number compared with a primitive is numeric; two boxes of
        // the same identity are equal.
        if self.is_boxed(a).await || self.is_boxed(b).await {
            let (a_prim, b_prim) = (self.try_prim(a).await, self.try_prim(b).await);
            if let (Some(x), Some(y)) = (a_prim, b_prim) {
                return apply_binary(BinaryOp::Eq, x, y)
                    .ok()
                    .map(|r| r == Prim::Bool(true));
            }
        }
        if self.is_string(a) && self.is_string(b) {
            let (x, y) = (self.string_contents(a).await, self.string_contents(b).await);
            return Some(x == y);
        }
        match (a, b) {
            (EvalValue::Jdwp(JdwpValue::Null), EvalValue::Jdwp(JdwpValue::Null)) => Some(true),
            (
                EvalValue::Jdwp(JdwpValue::Object { object_id: x, .. }),
                EvalValue::Jdwp(JdwpValue::Object { object_id: y, .. }),
            ) => Some(x == y),
            _ => Some(false),
        }
    }

    // ---- Names ----

    async fn resolve_name(&mut self, name: &str, key: u64) -> Result<EvalValue, String> {
        let scope = self.scope(key)?;
        if let Some(value) = self.read_local(key, name).await? {
            return Ok(EvalValue::Jdwp(value));
        }
        // Fields of `this`, then static fields of the enclosing classes.
        if let Ok(JdwpValue::Object { object_id, .. }) = self.this_value(key).await {
            if let Some(type_id) = self.runtime_type(object_id).await {
                if let Some((declaring, field)) = self.lookup_field(type_id, name).await {
                    return self
                        .read_field(Some(object_id), declaring, &field)
                        .await
                        .map(EvalValue::Jdwp);
                }
            }
        }
        let mut class_id = Some(scope.class_id);
        while let Some(current) = class_id {
            if let Some((declaring, field)) = self.lookup_field(current, name).await {
                if field.mod_bits & ACC_STATIC != 0 {
                    return self
                        .read_field(None, declaring, &field)
                        .await
                        .map(EvalValue::Jdwp);
                }
            }
            class_id = self.outer_class(current).await;
        }
        if let Some(found) = self.find_class_named(name, scope.class_id).await {
            return Ok(found);
        }
        Ok(EvalValue::Package(name.to_owned()))
    }

    /// The enclosing class of a nested class (`a/Outer$Inner` -> `a/Outer`).
    async fn outer_class(&mut self, class_id: u64) -> Option<u64> {
        let signature = self.get_class_signature(class_id).await?;
        let (outer, _) = signature.trim_end_matches(';').rsplit_once('$')?;
        let outer = format!("{outer};");
        self.resolve_class(&outer).await
    }

    async fn find_class_named(&mut self, name: &str, current_class: u64) -> Option<EvalValue> {
        // Same package first, then java.lang, then any unique loaded class.
        let current = self
            .get_class_signature(current_class)
            .await
            .unwrap_or_default();
        let package = current
            .trim_start_matches('L')
            .rsplit_once('/')
            .map_or("", |(pkg, _)| pkg);
        let mut candidates = Vec::new();
        if package.is_empty() {
            candidates.push(format!("L{name};"));
        } else {
            candidates.push(format!("L{package}/{name};"));
        }
        candidates.push(format!("Ljava/lang/{name};"));
        for signature in candidates {
            if let Some(type_id) = self.resolve_class(&signature).await {
                return Some(EvalValue::Class { type_id, signature });
            }
        }
        // Unique by simple name among everything loaded.
        let classes = match &self.types.all_classes {
            Some(classes) => classes.clone(),
            None => self.refresh_all_classes().await,
        };
        let suffix = format!("/{name};");
        let mut matches = classes.iter().filter(|class| {
            class.signature.ends_with(&suffix) || class.signature == format!("L{name};")
        });
        let first = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some(EvalValue::Class {
            type_id: first.type_id,
            signature: first.signature.clone(),
        })
    }

    async fn read_local(&self, key: u64, name: &str) -> Result<Option<JdwpValue>, String> {
        let scope = self.scope(key)?;
        let id_sizes = *self.id_sizes()?;
        let reply = self
            .conn()?
            .send_command(Command::method_variable_table_with_generic(
                scope.class_id,
                scope.method_id,
                id_sizes.reference_type_id_size,
                id_sizes.method_id_size,
            ))
            .await;
        // Compiled without `-g`: no local names, but fields still resolve.
        let Ok(reply) = reply else {
            return Ok(None);
        };
        let mut data = reply.data;
        let (_arg_count, variables) =
            replies::parse_variable_table_with_generic(&mut data).map_err(|e| e.to_string())?;
        let Some(variable) = variables.iter().find(|v| {
            v.name == name
                && scope.code_index >= v.code_index
                && scope.code_index < v.code_index + u64::from(v.length)
        }) else {
            return Ok(None);
        };
        let frame = scope.frame_id;
        let reply = self
            .conn()?
            .send_command(Command::stack_frame_get_values(
                scope.thread_id,
                frame,
                &[(variable.slot, sig_to_tag(&variable.signature))],
                id_sizes.object_id_size,
                id_sizes.frame_id_size,
            ))
            .await
            .map_err(|e| format!("failed to read '{name}': {e}"))?;
        let mut data = reply.data;
        Ok(replies::parse_stack_frame_values(&mut data, &id_sizes)
            .map_err(|e| e.to_string())?
            .into_iter()
            .next())
    }

    pub(super) async fn this_value(&self, key: u64) -> Result<JdwpValue, String> {
        let scope = self.scope(key)?;
        let id_sizes = *self.id_sizes()?;
        let frame = scope.frame_id;
        let reply = self
            .conn()?
            .send_command(Command::stack_frame_this_object(
                scope.thread_id,
                frame,
                id_sizes.object_id_size,
                id_sizes.frame_id_size,
            ))
            .await
            .map_err(|e| format!("failed to read `this`: {e}"))?;
        let mut data = reply.data;
        match replies::parse_this_object(&mut data, &id_sizes).map_err(|e| e.to_string())? {
            JdwpValue::Null | JdwpValue::Object { object_id: 0, .. } => {
                Err("there is no `this` in a static context".to_owned())
            }
            value => Ok(value),
        }
    }

    // ---- Fields ----

    /// Find a field by name in `type_id`, its superclasses and interfaces.
    /// Returns the declaring class and the field.
    pub(super) async fn lookup_field(
        &mut self,
        type_id: u64,
        name: &str,
    ) -> Option<(u64, replies::FieldInfo)> {
        let mut pending = vec![type_id];
        let mut seen = std::collections::HashSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current) {
                continue;
            }
            if let Ok(fields) = self.get_fields(current).await {
                if let Some(field) = fields.iter().find(|f| f.name == name) {
                    return Some((current, field.clone()));
                }
            }
            // Superclass first: push it last so it is popped first.
            let mut supers = self.supertypes(current).await;
            supers.reverse();
            pending.extend(supers);
        }
        None
    }

    pub(super) async fn read_field(
        &self,
        object_id: Option<u64>,
        declaring: u64,
        field: &replies::FieldInfo,
    ) -> Result<JdwpValue, String> {
        let id_sizes = *self.id_sizes()?;
        if field.mod_bits & ACC_STATIC != 0 {
            let reply = self
                .conn()?
                .send_command(Command::reference_type_get_values(
                    declaring,
                    &[field.field_id],
                    &id_sizes,
                ))
                .await
                .map_err(|e| format!("failed to read static field '{}': {e}", field.name))?;
            let mut data = reply.data;
            return replies::parse_object_get_values(&mut data, &id_sizes)
                .map_err(|e| e.to_string())?
                .into_iter()
                .next()
                .ok_or_else(|| format!("no value for field '{}'", field.name));
        }
        let object_id = object_id
            .ok_or_else(|| format!("field '{}' is not static and needs an object", field.name))?;
        let reply = self
            .conn()?
            .send_command(Command::object_get_values(
                object_id,
                &[field.field_id],
                id_sizes.object_id_size,
                id_sizes.field_id_size,
            ))
            .await
            .map_err(|e| format!("failed to read field '{}': {e}", field.name))?;
        let mut data = reply.data;
        replies::parse_object_get_values(&mut data, &id_sizes)
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .ok_or_else(|| format!("no value for field '{}'", field.name))
    }

    async fn select_field(
        &mut self,
        base: EvalValue,
        name: &str,
        key: u64,
    ) -> Result<EvalValue, String> {
        match base {
            EvalValue::Package(package) => {
                let signature = format!("L{}/{name};", package.replace('.', "/"));
                if let Some(type_id) = self.resolve_class(&signature).await {
                    return Ok(EvalValue::Class { type_id, signature });
                }
                Ok(EvalValue::Package(format!("{package}.{name}")))
            }
            EvalValue::Class { type_id, signature } => {
                if let Some((declaring, field)) = self.lookup_field(type_id, name).await {
                    if field.mod_bits & ACC_STATIC != 0 {
                        return self
                            .read_field(None, declaring, &field)
                            .await
                            .map(EvalValue::Jdwp);
                    }
                    return Err(format!("field '{name}' is not static"));
                }
                let nested = format!("{}${name};", signature.trim_end_matches(';'));
                if let Some(type_id) = self.resolve_class(&nested).await {
                    return Ok(EvalValue::Class {
                        type_id,
                        signature: nested,
                    });
                }
                Err(format!(
                    "no static field '{name}' in {}",
                    jni_sig_to_display(&signature)
                ))
            }
            EvalValue::Jdwp(JdwpValue::Null) => Err(format!("cannot read '{name}' of null")),
            EvalValue::Jdwp(JdwpValue::Object { tag, object_id }) => {
                if tag == tag::ARRAY {
                    if name == "length" {
                        return self
                            .array_length(object_id)
                            .await
                            .map(|len| EvalValue::Jdwp(JdwpValue::Int(len)));
                    }
                    return Err(format!(
                        "cannot resolve '{name}' on an array (only 'length')"
                    ));
                }
                let type_id = self
                    .runtime_type(object_id)
                    .await
                    .ok_or("failed to read the object's type")?;
                let Some((declaring, field)) = self.lookup_field(type_id, name).await else {
                    // Kotlin properties, records and beans: `size`, `age`,
                    // `active` read through `getSize()`/`age()`/`isActive()`.
                    if let Some(value) = self.property_getter(object_id, type_id, name, key).await {
                        return Ok(EvalValue::Jdwp(value));
                    }
                    let type_name = self
                        .types
                        .signature_by_id
                        .get(&type_id)
                        .map_or_else(|| "the object".to_owned(), |s| jni_sig_to_display(s));
                    return Err(format!("cannot resolve '{name}' on {type_name}"));
                };
                self.read_field(Some(object_id), declaring, &field)
                    .await
                    .map(EvalValue::Jdwp)
            }
            EvalValue::Str(_) => Err(format!("cannot resolve '{name}' on a String value")),
            EvalValue::Jdwp(_) => Err(format!("cannot resolve '{name}' on a primitive value")),
        }
    }

    /// Read `name` through an accessor method when the object has no such field.
    async fn property_getter(
        &mut self,
        object_id: u64,
        type_id: u64,
        name: &str,
        key: u64,
    ) -> Option<JdwpValue> {
        if !self.allow_invoke {
            return None;
        }
        let thread_id = self.scope(key).ok()?.thread_id;
        let mut capitalized = name.chars();
        let capitalized: String = capitalized
            .next()?
            .to_uppercase()
            .chain(capitalized)
            .collect();
        for accessor in [
            format!("get{capitalized}"),
            format!("is{capitalized}"),
            name.to_owned(),
        ] {
            let Ok((declaring, method)) = self.find_method(type_id, &accessor, &[], false).await
            else {
                continue;
            };
            if !method.signature.starts_with("()") || method.signature == "()V" {
                continue;
            }
            return self
                .invoke(
                    thread_id,
                    Some(object_id),
                    declaring,
                    &method,
                    &[],
                    INVOKE_TIMEOUT,
                )
                .await
                .ok();
        }
        None
    }

    pub(super) async fn array_length(&self, array_id: u64) -> Result<i32, String> {
        let id_sizes = *self.id_sizes()?;
        let reply = self
            .conn()?
            .send_command(Command::array_length(array_id, id_sizes.object_id_size))
            .await
            .map_err(|e| format!("failed to get array length: {e}"))?;
        let mut data = reply.data;
        replies::parse_array_length(&mut data).map_err(|e| e.to_string())
    }

    async fn index_array(
        &mut self,
        array: EvalValue,
        index: EvalValue,
        key: u64,
    ) -> Result<EvalValue, String> {
        let EvalValue::Jdwp(JdwpValue::Object {
            tag: tag::ARRAY,
            object_id,
        }) = array
        else {
            return Err("only arrays can be indexed".to_owned());
        };
        let index = match self.require_prim(&index, key).await? {
            Prim::Int(i) => i,
            Prim::Char(c) => i32::from(c),
            _ => return Err("array index must be an int".to_owned()),
        };
        let length = self.array_length(object_id).await?;
        if index < 0 || index >= length {
            return Err(format!("index {index} out of bounds for length {length}"));
        }
        let id_sizes = *self.id_sizes()?;
        let reply = self
            .conn()?
            .send_command(Command::array_get_values(
                object_id,
                index,
                1,
                id_sizes.object_id_size,
            ))
            .await
            .map_err(|e| format!("failed to read the array element: {e}"))?;
        let mut data = reply.data;
        replies::parse_array_values(&mut data, &id_sizes)
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .map(EvalValue::Jdwp)
            .ok_or_else(|| "no array element returned".to_owned())
    }

    // ---- Method calls ----

    async fn call_method(
        &mut self,
        receiver: Option<&Expr>,
        name: &str,
        args: &[Expr],
        key: u64,
    ) -> Result<EvalValue, String> {
        let scope = self.scope(key)?;
        // Evaluate the receiver before the arguments, like Java.
        let target = match receiver {
            Some(expression) => self.eval(expression, key).await?,
            None => match self.this_value(key).await {
                Ok(this) => EvalValue::Jdwp(this),
                Err(_) => EvalValue::Class {
                    type_id: scope.class_id,
                    signature: self
                        .get_class_signature(scope.class_id)
                        .await
                        .unwrap_or_default(),
                },
            },
        };
        let mut evaluated = Vec::with_capacity(args.len());
        for argument in args {
            evaluated.push(self.eval(argument, key).await?);
        }

        let (object, type_id) = match target {
            EvalValue::Class { type_id, .. } => (None, type_id),
            EvalValue::Jdwp(JdwpValue::Null) => {
                return Err(format!("cannot call '{name}()' on null"));
            }
            EvalValue::Jdwp(JdwpValue::Object { object_id, .. }) => {
                let type_id = self
                    .runtime_type(object_id)
                    .await
                    .ok_or("failed to read the object's type")?;
                (Some(object_id), type_id)
            }
            EvalValue::Str(text) => {
                let id = self.create_string(&text).await?;
                let type_id = self
                    .runtime_type(id)
                    .await
                    .ok_or("failed to read the string's type")?;
                (Some(id), type_id)
            }
            EvalValue::Jdwp(_) => return Err(format!("cannot call '{name}()' on a primitive")),
            EvalValue::Package(package) => {
                return Err(format!("cannot resolve symbol '{package}'"));
            }
        };

        let (declaring, method) = self
            .find_method(type_id, name, &evaluated, object.is_none())
            .await?;
        let params = method_parameters(&method.signature);
        let mut call_args = Vec::with_capacity(params.len());
        for (argument, param) in evaluated.into_iter().zip(params.iter()) {
            call_args.push(self.coerce(argument, param, scope.thread_id).await?);
        }
        let result = self
            .invoke(
                scope.thread_id,
                object,
                declaring,
                &method,
                &call_args,
                INVOKE_TIMEOUT,
            )
            .await?;
        Ok(EvalValue::Jdwp(result))
    }

    /// Find the best-matching method by name and argument types, searching
    /// the class, its superclasses, then interfaces (default methods).
    pub(super) async fn find_method(
        &mut self,
        type_id: u64,
        name: &str,
        args: &[EvalValue],
        static_only: bool,
    ) -> Result<(u64, MethodInfo), String> {
        let mut best: Option<(u32, u64, MethodInfo)> = None;
        let mut pending = vec![type_id];
        let mut seen = std::collections::HashSet::new();
        let mut order = 0u32;
        while let Some(current) = pending.pop() {
            if !seen.insert(current) {
                continue;
            }
            order += 1;
            if let Some(methods) = self.get_methods(current).await {
                for method in methods {
                    if method.name != name
                        || method.mod_bits & ACC_ABSTRACT != 0
                        || (static_only && method.mod_bits & ACC_STATIC == 0)
                    {
                        continue;
                    }
                    let params = method_parameters(&method.signature);
                    if params.len() != args.len() {
                        continue;
                    }
                    let mut total = 0;
                    let mut compatible = true;
                    for (argument, param) in args.iter().zip(&params) {
                        match self.argument_score(argument, param).await {
                            Some(score) => total += score,
                            None => {
                                compatible = false;
                                break;
                            }
                        }
                    }
                    // Nearer classes win ties (overrides shadow supertypes).
                    let ranked = total * 100 + 99u32.saturating_sub(order);
                    if compatible
                        && best
                            .as_ref()
                            .is_none_or(|(current_best, _, _)| ranked > *current_best)
                    {
                        best = Some((ranked, current, method));
                    }
                }
            }
            let mut supers = self.supertypes(current).await;
            supers.reverse();
            pending.extend(supers);
        }
        best.map(|(_, declaring, method)| (declaring, method))
            .ok_or_else(|| {
                let type_name = self
                    .types
                    .signature_by_id
                    .get(&type_id)
                    .map_or_else(|| "the object".to_owned(), |s| jni_sig_to_display(s));
                format!(
                    "no method '{name}' taking {} argument(s) of these types in {type_name}",
                    args.len()
                )
            })
    }

    /// How well `argument` fits a parameter of JNI signature `param`
    /// (higher is better, `None` = not assignable).
    async fn argument_score(&mut self, argument: &EvalValue, param: &str) -> Option<u32> {
        let is_primitive_param = param.len() == 1;
        match argument {
            EvalValue::Jdwp(value) if Prim::from_jdwp(value).is_some() => {
                let prim = Prim::from_jdwp(value)?;
                let own = match value {
                    JdwpValue::Byte(_) => "B",
                    JdwpValue::Short(_) => "S",
                    _ => prim_signature(prim),
                };
                if is_primitive_param {
                    if own == param {
                        Some(10)
                    } else if widens(own, param) {
                        Some(7)
                    } else {
                        None
                    }
                } else if param == "Ljava/lang/Object;"
                    || param == "Ljava/lang/Number;"
                        && !matches!(prim, Prim::Bool(_) | Prim::Char(_))
                    || param == "Ljava/io/Serializable;"
                    || param == "Ljava/lang/Comparable;"
                {
                    Some(3)
                } else if unbox_signature(param).is_some_and(|prim_sig| prim_sig == own) {
                    Some(4)
                } else {
                    None
                }
            }
            EvalValue::Jdwp(JdwpValue::Null) => (!is_primitive_param).then_some(2),
            EvalValue::Str(_) => {
                if is_primitive_param {
                    None
                } else if param == "Ljava/lang/String;" {
                    Some(10)
                } else if matches!(
                    param,
                    "Ljava/lang/Object;"
                        | "Ljava/lang/CharSequence;"
                        | "Ljava/lang/Comparable;"
                        | "Ljava/io/Serializable;"
                ) {
                    Some(4)
                } else {
                    None
                }
            }
            EvalValue::Jdwp(JdwpValue::Object { object_id, .. }) => {
                let type_id = self.runtime_type(*object_id).await?;
                if is_primitive_param {
                    // Unboxing.
                    let signature = self.get_class_signature(type_id).await?;
                    let prim_sig = unbox_signature(&signature)?;
                    return widens(prim_sig, param).then_some(4);
                }
                if param == "Ljava/lang/Object;" {
                    return Some(3);
                }
                let signature = self.get_class_signature(type_id).await?;
                if signature == param {
                    return Some(10);
                }
                if param.starts_with('[') || signature.starts_with('[') {
                    return None;
                }
                self.is_subtype(type_id, param).await.then_some(8)
            }
            _ => None,
        }
    }

    /// Turn an evaluated argument into the value the method's parameter needs.
    async fn coerce(
        &mut self,
        argument: EvalValue,
        param: &str,
        thread_id: u64,
    ) -> Result<JdwpValue, String> {
        if param.len() == 1 {
            let prim = self
                .try_prim(&argument)
                .await
                .ok_or("argument is not a primitive")?;
            return convert_prim(prim, param).ok_or_else(|| "argument type mismatch".to_owned());
        }
        match argument {
            EvalValue::Str(text) => {
                let id = self.create_string(&text).await?;
                Ok(JdwpValue::Object {
                    tag: tag::STRING,
                    object_id: id,
                })
            }
            EvalValue::Jdwp(value) => match Prim::from_jdwp(&value) {
                Some(prim) => {
                    // Box into the parameter's own box class when it names
                    // one (`Long` for a `long` value), else the natural one.
                    let target = unbox_signature(param).unwrap_or_else(|| prim_signature(prim));
                    let converted = convert_prim(prim, target).ok_or("cannot box the argument")?;
                    self.box_primitive(&converted, target, thread_id).await
                }
                None => Ok(value),
            },
            EvalValue::Class { .. } | EvalValue::Package(_) => {
                Err("a class cannot be passed as an argument".to_owned())
            }
        }
    }

    async fn box_primitive(
        &mut self,
        value: &JdwpValue,
        prim_sig: &str,
        thread_id: u64,
    ) -> Result<JdwpValue, String> {
        let box_sig = box_signature(prim_sig).ok_or("unsupported primitive")?;
        let class_id = self
            .resolve_class(box_sig)
            .await
            .ok_or_else(|| format!("{box_sig} is not loaded"))?;
        let methods = self
            .get_methods(class_id)
            .await
            .ok_or("failed to list methods")?;
        let wanted = format!("({prim_sig}){box_sig}");
        let method = methods
            .into_iter()
            .find(|m| m.name == "valueOf" && m.signature == wanted)
            .ok_or("valueOf not found")?;
        self.invoke(
            thread_id,
            None,
            class_id,
            &method,
            std::slice::from_ref(value),
            INVOKE_TIMEOUT,
        )
        .await
    }

    pub(super) async fn create_string(&self, text: &str) -> Result<u64, String> {
        let id_sizes = *self.id_sizes()?;
        let reply = self
            .conn()?
            .send_command(Command::vm_create_string(text))
            .await
            .map_err(|e| format!("failed to create a string in the debuggee: {e}"))?;
        let mut data = reply.data;
        replies::parse_create_string(&mut data, &id_sizes).map_err(|e| e.to_string())
    }

    /// Run a method in the debuggee on `thread_id` (single-threaded) with a
    /// time limit. `object` is `None` for static methods.
    pub(super) async fn invoke(
        &mut self,
        thread_id: u64,
        object: Option<u64>,
        class_id: u64,
        method: &MethodInfo,
        args: &[JdwpValue],
        limit: Duration,
    ) -> Result<JdwpValue, String> {
        if self.invoke_disabled {
            return Err(
                "method calls are disabled: an earlier call did not finish in time".to_owned(),
            );
        }
        let id_sizes = *self.id_sizes()?;
        let command = match object {
            Some(object_id) => Command::object_invoke_method(
                object_id,
                thread_id,
                class_id,
                method.method_id,
                args,
                &id_sizes,
            ),
            None => {
                Command::class_invoke_method(class_id, thread_id, method.method_id, args, &id_sizes)
            }
        };
        let outcome = tokio::time::timeout(limit, self.conn()?.send_command(command)).await;
        let reply = match outcome {
            Ok(reply) => reply,
            Err(_) => {
                self.invoke_disabled = true;
                return Err(format!(
                    "'{}' did not finish within {}s",
                    method.name,
                    limit.as_secs()
                ));
            }
        };
        // The invocation resumed the thread: every frame id it had is stale.
        self.refresh_frames(thread_id).await;
        let reply = reply.map_err(|e| format!("invoking '{}' failed: {e}", method.name))?;
        let mut data = reply.data;
        let result =
            replies::parse_invoke_result(&mut data, &id_sizes).map_err(|e| e.to_string())?;
        if let Some(exception) = result.exception {
            let description = self
                .describe_exception(exception)
                .await
                .unwrap_or_else(|| "an exception".to_owned());
            return Err(format!("'{}' threw {description}", method.name));
        }
        Ok(result.value)
    }

    /// Re-point every scope of `thread_id` at the thread's current frame ids.
    pub(super) async fn refresh_frames(&mut self, thread_id: u64) {
        let Ok(conn) = self.conn() else {
            return;
        };
        let id_sizes = *conn.id_sizes();
        let Ok(reply) = conn
            .send_command(Command::thread_frames(
                thread_id,
                0,
                -1,
                id_sizes.object_id_size,
            ))
            .await
        else {
            return;
        };
        let mut data = reply.data;
        let Ok(frames) = replies::parse_thread_frames(&mut data, &id_sizes) else {
            return;
        };
        for scope in self.variables.scopes.values_mut() {
            if scope.thread_id != thread_id {
                continue;
            }
            if let Some(frame) = usize::try_from(scope.depth)
                .ok()
                .and_then(|d| frames.get(d))
            {
                scope.frame_id = frame.frame_id;
            }
        }
    }

    // ---- Types ----

    /// Runtime class of an object.
    pub(super) async fn runtime_type(&mut self, object_id: u64) -> Option<u64> {
        let id_sizes = *self.id_sizes().ok()?;
        let reply = self
            .conn()
            .ok()?
            .send_command(Command::object_reference_type(
                object_id,
                id_sizes.object_id_size,
            ))
            .await
            .ok()?;
        let mut data = reply.data;
        let (_, type_id) = replies::parse_object_reference_type(&mut data, &id_sizes).ok()?;
        // Warm the signature cache; every caller wants it next.
        let _ = self.get_class_signature(type_id).await;
        Some(type_id)
    }

    /// Direct superclass and interfaces of a class (cached).
    pub(super) async fn supertypes(&mut self, type_id: u64) -> Vec<u64> {
        if let Some(cached) = self.types.supertypes.get(&type_id) {
            return cached.clone();
        }
        let mut supers = Vec::new();
        if let Ok(conn) = self.conn() {
            let id_sizes = *conn.id_sizes();
            // Arrays and interfaces have no superclass: the JDWP command
            // simply fails for them, which is fine.
            if let Ok(reply) = conn
                .send_command(Command::class_type_superclass(
                    type_id,
                    id_sizes.reference_type_id_size,
                ))
                .await
            {
                let mut data = reply.data;
                if let Ok(id) = replies::parse_class_type_superclass(&mut data, &id_sizes) {
                    if id != 0 {
                        supers.push(id);
                    }
                }
            }
            if let Ok(reply) = conn
                .send_command(Command::reference_type_interfaces(
                    type_id,
                    id_sizes.reference_type_id_size,
                ))
                .await
            {
                let mut data = reply.data;
                if let Ok(ids) = replies::parse_interfaces(&mut data, &id_sizes) {
                    supers.extend(ids);
                }
            }
        }
        self.types.supertypes.insert(type_id, supers.clone());
        supers
    }

    /// Is `type_id` (or one of its supertypes) the type with `signature`?
    pub(super) async fn is_subtype(&mut self, type_id: u64, signature: &str) -> bool {
        let mut pending = vec![type_id];
        let mut seen = std::collections::HashSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current) {
                continue;
            }
            if self.get_class_signature(current).await.as_deref() == Some(signature) {
                return true;
            }
            pending.extend(self.supertypes(current).await);
        }
        false
    }

    // ---- Value conversion ----

    const fn is_string(&self, value: &EvalValue) -> bool {
        match value {
            EvalValue::Str(_) => true,
            EvalValue::Jdwp(JdwpValue::Object { tag, .. }) => *tag == tag::STRING,
            _ => false,
        }
    }

    async fn is_boxed(&mut self, value: &EvalValue) -> bool {
        let EvalValue::Jdwp(JdwpValue::Object { object_id, .. }) = value else {
            return false;
        };
        let Some(type_id) = self.runtime_type(*object_id).await else {
            return false;
        };
        self.get_class_signature(type_id)
            .await
            .is_some_and(|sig| unbox_signature(&sig).is_some())
    }

    async fn string_contents(&self, value: &EvalValue) -> Option<String> {
        match value {
            EvalValue::Str(text) => Some(text.clone()),
            EvalValue::Jdwp(JdwpValue::Object { object_id, .. }) => {
                self.resolve_string_value(*object_id).await
            }
            _ => None,
        }
    }

    /// A primitive from a value, unboxing `Integer`, `Boolean`, ... objects.
    async fn try_prim(&mut self, value: &EvalValue) -> Option<Prim> {
        match value {
            EvalValue::Jdwp(jdwp) => {
                if let Some(prim) = Prim::from_jdwp(jdwp) {
                    return Some(prim);
                }
                let JdwpValue::Object { object_id, .. } = jdwp else {
                    return None;
                };
                let type_id = self.runtime_type(*object_id).await?;
                let signature = self.get_class_signature(type_id).await?;
                unbox_signature(&signature)?;
                let (declaring, field) = self.lookup_field(type_id, "value").await?;
                let inner = self
                    .read_field(Some(*object_id), declaring, &field)
                    .await
                    .ok()?;
                Prim::from_jdwp(&inner)
            }
            _ => None,
        }
    }

    async fn require_prim(&mut self, value: &EvalValue, _key: u64) -> Result<Prim, String> {
        self.try_prim(value).await.ok_or_else(|| {
            match value {
                EvalValue::Jdwp(JdwpValue::Null) => "null cannot be used as a number or boolean",
                EvalValue::Str(_) | EvalValue::Jdwp(JdwpValue::Object { .. }) => {
                    "the operand is not a number or boolean"
                }
                _ => "the operand is not a value",
            }
            .to_owned()
        })
    }

    /// Java's string conversion of a value, as used by `+` concatenation.
    pub(super) async fn display_string(
        &mut self,
        value: &EvalValue,
        thread_id: u64,
    ) -> Result<String, String> {
        Ok(match value {
            EvalValue::Str(text) => text.clone(),
            EvalValue::Jdwp(JdwpValue::Null) => "null".to_owned(),
            EvalValue::Jdwp(jdwp) => {
                if let Some(prim) = Prim::from_jdwp(jdwp) {
                    return Ok(prim.display());
                }
                let JdwpValue::Object { object_id, tag } = jdwp else {
                    return Ok("?".to_owned());
                };
                if *tag == tag::STRING {
                    return Ok(self
                        .resolve_string_value(*object_id)
                        .await
                        .unwrap_or_default());
                }
                if let Some(prim) = self.try_prim(value).await {
                    return Ok(prim.display());
                }
                match self.call_to_string(*object_id, thread_id).await {
                    Some(text) => text,
                    None => self.default_object_text(*object_id).await,
                }
            }
            EvalValue::Class { signature, .. } => {
                format!("class {}", jni_sig_to_display(signature))
            }
            EvalValue::Package(name) => return Err(format!("cannot resolve symbol '{name}'")),
        })
    }

    // ---- Result presentation ----

    async fn present_eval_value(
        &mut self,
        value: EvalValue,
        thread_id: u64,
    ) -> Result<super::values::Presented, String> {
        Ok(match value {
            EvalValue::Str(text) => super::values::Presented {
                text: format!("\"{text}\""),
                type_name: Some("java.lang.String".to_owned()),
                variables_reference: 0,
            },
            EvalValue::Jdwp(value) => self.present(&value, thread_id).await,
            EvalValue::Class { signature, .. } => super::values::Presented {
                text: format!("class {}", jni_sig_to_display(&signature)),
                type_name: Some("java.lang.Class".to_owned()),
                variables_reference: 0,
            },
            EvalValue::Package(name) => return Err(format!("cannot resolve symbol '{name}'")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_follows_java_promotion_and_overflow_rules() {
        let add = |a, b| apply_binary(BinaryOp::Add, a, b).unwrap();
        assert_eq!(add(Prim::Int(2), Prim::Int(3)), Prim::Int(5));
        assert_eq!(add(Prim::Int(i32::MAX), Prim::Int(1)), Prim::Int(i32::MIN));
        assert_eq!(add(Prim::Int(1), Prim::Long(2)), Prim::Long(3));
        assert_eq!(add(Prim::Int(1), Prim::Double(0.5)), Prim::Double(1.5));
        assert_eq!(add(Prim::Char(b'a' as u16), Prim::Int(1)), Prim::Int(98));
        assert_eq!(
            apply_binary(BinaryOp::Div, Prim::Int(7), Prim::Int(2)).unwrap(),
            Prim::Int(3)
        );
        assert_eq!(
            apply_binary(BinaryOp::Rem, Prim::Int(-7), Prim::Int(3)).unwrap(),
            Prim::Int(-1)
        );
        assert!(apply_binary(BinaryOp::Div, Prim::Int(1), Prim::Int(0)).is_err());
        assert_eq!(
            apply_binary(BinaryOp::Div, Prim::Double(1.0), Prim::Int(0)).unwrap(),
            Prim::Double(f64::INFINITY)
        );
    }

    #[test]
    fn comparisons_cover_mixed_numeric_types_and_booleans() {
        let cmp = |op, a, b| apply_binary(op, a, b).unwrap();
        assert_eq!(
            cmp(BinaryOp::Lt, Prim::Int(1), Prim::Long(2)),
            Prim::Bool(true)
        );
        assert_eq!(
            cmp(BinaryOp::Eq, Prim::Int(1), Prim::Double(1.0)),
            Prim::Bool(true)
        );
        assert_eq!(
            cmp(BinaryOp::Ne, Prim::Char(65), Prim::Int(65)),
            Prim::Bool(false)
        );
        assert_eq!(
            cmp(BinaryOp::Ge, Prim::Long(i64::MAX), Prim::Long(i64::MAX - 1)),
            Prim::Bool(true)
        );
        assert_eq!(
            cmp(BinaryOp::And, Prim::Bool(true), Prim::Bool(false)),
            Prim::Bool(false)
        );
        assert_eq!(
            cmp(BinaryOp::Eq, Prim::Bool(true), Prim::Bool(true)),
            Prim::Bool(true)
        );
        assert!(apply_binary(BinaryOp::Add, Prim::Bool(true), Prim::Int(1)).is_err());
        // NaN compares false except for !=.
        assert_eq!(
            cmp(BinaryOp::Eq, Prim::Double(f64::NAN), Prim::Double(f64::NAN)),
            Prim::Bool(false)
        );
    }

    #[test]
    fn integer_division_overflow_wraps_and_remainder_is_zero() {
        for (minimum, negative_one, zero) in [
            (Prim::Int(i32::MIN), Prim::Int(-1), Prim::Int(0)),
            (Prim::Long(i64::MIN), Prim::Long(-1), Prim::Long(0)),
        ] {
            assert_eq!(
                apply_binary(BinaryOp::Div, minimum, negative_one),
                Ok(minimum)
            );
            assert_eq!(apply_binary(BinaryOp::Rem, minimum, negative_one), Ok(zero));
            for op in [BinaryOp::Div, BinaryOp::Rem] {
                assert_eq!(apply_binary(op, minimum, zero), Err("/ by zero".to_owned()));
            }
        }
    }

    #[test]
    fn float_comparisons_apply_binary_numeric_promotion() {
        for integer in [Prim::Int(16_777_217), Prim::Long(16_777_217)] {
            let float = Prim::Float(16_777_216.0);
            for (op, expected) in [
                (BinaryOp::Eq, true),
                (BinaryOp::Ne, false),
                (BinaryOp::Lt, false),
                (BinaryOp::Le, true),
                (BinaryOp::Gt, false),
                (BinaryOp::Ge, true),
            ] {
                assert_eq!(apply_binary(op, integer, float), Ok(Prim::Bool(expected)));
                assert_eq!(apply_binary(op, float, integer), Ok(Prim::Bool(expected)));
            }
            // A double operand promotes to double instead, retaining this integer's precision.
            assert_eq!(
                apply_binary(BinaryOp::Eq, integer, Prim::Double(16_777_216.0)),
                Ok(Prim::Bool(false))
            );
        }
    }

    #[test]
    fn long_to_float_conversion_rounds_once() {
        // This value is just above a float midpoint. Rounding through f64 first loses the low
        // bit and rounds to the other neighboring float.
        let integer = Prim::Long(4_611_686_293_305_294_849);
        let expected = 4_611_686_568_183_201_792.0_f32;
        assert_eq!(
            apply_binary(BinaryOp::Add, integer, Prim::Float(0.0)),
            Ok(Prim::Float(expected))
        );
        assert!(
            matches!(convert_prim(integer, "F"), Some(JdwpValue::Float(value)) if value.to_bits() == expected.to_bits())
        );
        assert_eq!(
            apply_binary(BinaryOp::Eq, integer, Prim::Float(expected)),
            Ok(Prim::Bool(true))
        );
    }

    #[test]
    fn java_string_conversion_of_primitives() {
        assert_eq!(Prim::Double(2.0).display(), "2.0");
        assert_eq!(Prim::Double(2.5).display(), "2.5");
        assert_eq!(Prim::Float(1.0).display(), "1.0");
        assert_eq!(Prim::Char('x' as u16).display(), "x");
        assert_eq!(Prim::Long(7).display(), "7");
        assert_eq!(Prim::Bool(true).display(), "true");
    }

    #[test]
    fn method_signatures_split_into_parameters() {
        assert_eq!(method_parameters("()V"), Vec::<String>::new());
        assert_eq!(
            method_parameters("(I[JLjava/lang/String;[[Ljava/util/List;D)V"),
            ["I", "[J", "Ljava/lang/String;", "[[Ljava/util/List;", "D"]
        );
    }

    #[test]
    fn primitive_widening_matches_the_jls() {
        assert!(widens("I", "J"));
        assert!(widens("B", "D"));
        assert!(widens("C", "I"));
        assert!(!widens("J", "I"));
        assert!(!widens("I", "C"));
        assert!(!widens("Z", "I"));
        assert!(widens("I", "I"));
    }
}
