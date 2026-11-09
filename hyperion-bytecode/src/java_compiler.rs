// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Shared JVM source model and compiler bookkeeping.
//!
//! Java CST conversion, the production Java backend, Kotlin's bounded evaluator,
//! and Groovy lowering share these types. Bytecode emission belongs to each
//! production backend rather than to a second generic compiler facade.

use std::collections::HashMap;
use std::fmt;

/// An internal inconsistency in operand-stack accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackDepthError {
    /// Lowering consumed more slots than it had produced.
    Underflow,
    /// The JVM stores `max_stack` in an unsigned 16-bit field.
    LimitExceeded { depth: i32 },
}

impl fmt::Display for StackDepthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Underflow => write!(f, "operand stack accounting underflowed"),
            Self::LimitExceeded { depth } => {
                write!(f, "operand stack depth {depth} exceeds the JVM limit")
            }
        }
    }
}

/// Tracks operand stack depth during compilation
#[derive(Debug, Clone)]
pub struct StackDepthTracker {
    /// Current stack depth
    current: i32,

    /// Maximum stack depth encountered
    max: i32,

    /// Sticky failure state: later pushes must not hide an earlier underflow.
    underflowed: bool,
}

impl Default for StackDepthTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl StackDepthTracker {
    /// Create a new stack depth tracker
    pub fn new() -> Self {
        Self {
            current: 0,
            max: 0,
            underflowed: false,
        }
    }

    /// Record pushing a value onto the stack (category 1: int, float, reference)
    pub fn push1(&mut self) {
        self.current += 1;
        self.max = self.max.max(self.current);
    }

    /// Record pushing a category 2 value (long, double)
    pub fn push2(&mut self) {
        self.current += 2;
        self.max = self.max.max(self.current);
    }

    /// Record popping a category 1 value
    pub fn pop1(&mut self) {
        self.pop(1);
    }

    /// Record popping a category 2 value
    pub fn pop2(&mut self) {
        self.pop(2);
    }

    /// Record popping n values from the stack
    pub fn pop(&mut self, n: i32) {
        if n < 0 || self.current < n {
            self.underflowed = true;
            self.current = 0;
        } else {
            self.current -= n;
        }
    }

    /// Get current stack depth
    pub fn current_depth(&self) -> i32 {
        self.current
    }

    /// Get the maximum stack depth, rejecting metadata that cannot be serialized.
    pub fn max_depth(&self) -> Result<u16, StackDepthError> {
        if self.underflowed {
            return Err(StackDepthError::Underflow);
        }
        u16::try_from(self.max).map_err(|_| StackDepthError::LimitExceeded { depth: self.max })
    }

    /// Reset for a new method
    pub fn reset(&mut self) {
        self.current = 0;
        self.max = 0;
        self.underflowed = false;
    }
}

/// Manages local variable slots and scoping
#[derive(Debug, Clone)]
pub struct LocalVariableTable {
    /// Slot -> variable name (for current scope)
    name_to_slot: HashMap<String, u16>,

    /// Next available slot
    next_slot: u16,

    /// Stack of scopes for block scoping
    scope_stack: Vec<ScopeInfo>,

    /// All variable entries for LocalVariableTable attribute
    entries: Vec<LocalVariableEntry>,
}

#[derive(Debug, Clone)]
struct ScopeInfo {
    /// Variables declared in this scope
    variables: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LocalVariableEntry {
    pub name: String,
    pub slot: u16,
    pub start_pc: u16,
    pub end_pc: u16,
}

impl Default for LocalVariableTable {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalVariableTable {
    /// Create a new local variable table
    pub fn new() -> Self {
        Self {
            name_to_slot: HashMap::new(),
            next_slot: 1, // 0 is reserved for 'this' in instance methods
            scope_stack: vec![ScopeInfo {
                variables: Vec::new(),
            }],
            entries: Vec::new(),
        }
    }

    /// Create for static method (slot 0 available)
    pub fn new_static() -> Self {
        Self {
            name_to_slot: HashMap::new(),
            next_slot: 0,
            scope_stack: vec![ScopeInfo {
                variables: Vec::new(),
            }],
            entries: Vec::new(),
        }
    }

    /// Allocate a slot for a new variable (category 1)
    pub fn allocate(&mut self, name: String, start_pc: u16) -> u16 {
        let slot = self.next_slot;
        self.next_slot += 1;

        self.name_to_slot.insert(name.clone(), slot);

        if let Some(scope) = self.scope_stack.last_mut() {
            scope.variables.push(name.clone());
        }

        self.entries.push(LocalVariableEntry {
            name,
            slot,
            start_pc,
            end_pc: 0, // Will be set when scope ends
        });

        slot
    }

    /// Allocate a slot for a category 2 variable (long, double - takes 2 slots)
    pub fn allocate_wide(&mut self, name: String, start_pc: u16) -> u16 {
        let slot = self.next_slot;
        self.next_slot += 2;

        self.name_to_slot.insert(name.clone(), slot);

        if let Some(scope) = self.scope_stack.last_mut() {
            scope.variables.push(name.clone());
        }

        self.entries.push(LocalVariableEntry {
            name,
            slot,
            start_pc,
            end_pc: 0,
        });

        slot
    }

    /// Look up a variable by name
    pub fn lookup(&self, name: &str) -> Option<u16> {
        self.name_to_slot.get(name).copied()
    }

    /// Get the next available local variable slot
    pub fn next_slot(&self) -> u16 {
        self.next_slot
    }

    /// Set the next available local variable slot (for temporary variables)
    pub fn set_next_slot(&mut self, slot: u16) {
        self.next_slot = slot;
    }

    /// Enter a new scope (block)
    pub fn enter_scope(&mut self) {
        self.scope_stack.push(ScopeInfo {
            variables: Vec::new(),
        });
    }

    /// Exit current scope
    pub fn exit_scope(&mut self, end_pc: u16) {
        if let Some(scope) = self.scope_stack.pop() {
            // Mark end PC for variables in this scope
            for name in &scope.variables {
                for entry in self.entries.iter_mut().rev() {
                    if &entry.name == name && entry.end_pc == 0 {
                        entry.end_pc = end_pc;
                        break;
                    }
                }
                self.name_to_slot.remove(name);
            }
        }
    }

    /// Get all variable entries for LocalVariableTable attribute
    pub fn entries(&self) -> &[LocalVariableEntry] {
        &self.entries
    }
}

/// Label for control flow targets
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Label(u32);

/// Allocates unique labels
pub struct LabelAllocator {
    next_id: u32,
}

impl Default for LabelAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl LabelAllocator {
    /// Create a new label allocator
    pub fn new() -> Self {
        Self { next_id: 0 }
    }

    /// Allocate a new unique label
    pub fn new_label(&mut self) -> Label {
        let label = Label(self.next_id);
        self.next_id += 1;
        label
    }
}

/// Java type representation for compilation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavaType {
    /// Primitive types
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Char,
    Void,

    /// Reference types
    Object(String), // Fully qualified name

    /// Array type
    Array {
        element_type: Box<JavaType>,
        dimensions: u8,
    },
}

impl JavaType {
    /// Get descriptor string for this type (JVM spec format)
    pub fn descriptor(&self) -> String {
        match self {
            JavaType::Byte => "B".to_string(),
            JavaType::Short => "S".to_string(),
            JavaType::Int => "I".to_string(),
            JavaType::Long => "J".to_string(),
            JavaType::Float => "F".to_string(),
            JavaType::Double => "D".to_string(),
            JavaType::Boolean => "Z".to_string(),
            JavaType::Char => "C".to_string(),
            JavaType::Void => "V".to_string(),
            JavaType::Object(name) => {
                format!("L{};", name.replace('.', "/"))
            }
            JavaType::Array {
                element_type,
                dimensions,
            } => {
                let elem_desc = element_type.descriptor();
                let brackets = "[".repeat(*dimensions as usize);
                format!("{}{}", brackets, elem_desc)
            }
        }
    }

    /// Check if this is a category 2 type (occupies 2 stack slots)
    pub fn is_category2(&self) -> bool {
        matches!(self, JavaType::Long | JavaType::Double)
    }

    /// Get width in stack slots
    pub fn stack_width(&self) -> u8 {
        match self {
            JavaType::Void => 0,
            _ if self.is_category2() => 2,
            _ => 1,
        }
    }

    /// Check if type is numeric
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            JavaType::Byte
                | JavaType::Short
                | JavaType::Int
                | JavaType::Long
                | JavaType::Float
                | JavaType::Double
                | JavaType::Char
        )
    }

    /// Check if types are assignable (from -> to)
    pub fn is_assignable_from(&self, other: &JavaType) -> bool {
        if self == other {
            return true;
        }

        match (self, other) {
            // Numeric type widening
            (JavaType::Long, JavaType::Int)
            | (JavaType::Long, JavaType::Short)
            | (JavaType::Long, JavaType::Byte)
            | (JavaType::Long, JavaType::Char) => true,

            (JavaType::Float, JavaType::Int)
            | (JavaType::Float, JavaType::Short)
            | (JavaType::Float, JavaType::Byte)
            | (JavaType::Float, JavaType::Char)
            | (JavaType::Float, JavaType::Long) => true,

            (JavaType::Double, JavaType::Int)
            | (JavaType::Double, JavaType::Short)
            | (JavaType::Double, JavaType::Byte)
            | (JavaType::Double, JavaType::Char)
            | (JavaType::Double, JavaType::Long)
            | (JavaType::Double, JavaType::Float) => true,

            (JavaType::Int, JavaType::Short)
            | (JavaType::Int, JavaType::Byte)
            | (JavaType::Int, JavaType::Char) => true,

            // Reference type assignability
            (JavaType::Object(target), JavaType::Object(_other)) => {
                // java.lang.Object is the supertype of all classes
                // Without full class hierarchy info, assume Object accepts anything
                target.ends_with("Object") || target.ends_with("java.lang.Object")
            }

            _ => false,
        }
    }
}

/// Compilation context for expressions
#[derive(Debug, Clone)]
pub struct ExpressionContext {
    /// Expected type (for type checking and coercion)
    pub expected_type: Option<JavaType>,

    /// Whether the expression value is needed
    pub value_used: bool,

    /// Whether this is an assignment target (lvalue)
    pub is_lvalue: bool,
}

impl ExpressionContext {
    /// Create a new expression context where value is used
    pub fn value() -> Self {
        Self {
            expected_type: None,
            value_used: true,
            is_lvalue: false,
        }
    }

    /// Create context where value is not needed (statement context)
    pub fn statement() -> Self {
        Self {
            expected_type: None,
            value_used: false,
            is_lvalue: false,
        }
    }

    /// Create context for an lvalue (assignment target)
    pub fn lvalue() -> Self {
        Self {
            expected_type: None,
            value_used: false,
            is_lvalue: true,
        }
    }
}

/// Represents a Java expression for compilation
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// Integer literal (int range)
    IntLiteral(i32),
    /// Character literal, held as its code point. Distinct from `IntLiteral` so
    /// the type is `char` (descriptor `C`) — e.g. `sb.append('a')` must append
    /// the character, and `println('a')` must print it, not its int code.
    CharLiteral(i32),
    /// Long literal
    LongLiteral(i64),
    /// Float literal
    FloatLiteral(f32),
    /// Double literal
    DoubleLiteral(f64),
    /// String literal
    StringLiteral(String),
    /// Boolean literal
    BooleanLiteral(bool),
    /// Null literal
    NullLiteral,
    /// Variable/local reference
    NameExpr(String),
    /// Binary operation (e.g., a + b)
    BinaryExpr {
        left: Box<Expression>,
        op: BinaryOp,
        right: Box<Expression>,
    },
    /// Unary operation (e.g., -x, !x)
    UnaryExpr {
        op: UnaryOp,
        operand: Box<Expression>,
    },
    /// Method call
    MethodCall {
        receiver: Option<Box<Expression>>,
        method: String,
        args: Vec<Expression>,
    },
    /// Object creation
    NewExpr {
        class: String,
        args: Vec<Expression>,
    },
    /// Array creation, e.g. `new int[n]`
    NewArray {
        element_type: JavaType,
        length: Box<Expression>,
    },
    /// Multi-dimensional array creation, e.g. `new int[a][b]`. `element_type` is
    /// the base (non-array) element; `lengths` gives one size per dimension.
    NewMultiArray {
        element_type: JavaType,
        lengths: Vec<Expression>,
    },
    /// Type cast
    Cast {
        target_type: JavaType,
        expr: Box<Expression>,
    },
    /// `value instanceof class`. `class` is the internal name of a class, or the descriptor of an
    /// array type. The result is a boolean.
    InstanceOf {
        value: Box<Expression>,
        class: String,
    },
    /// This reference
    This,
    /// Field access (e.g., obj.field or MyClass.staticField)
    FieldAccess {
        receiver: Option<Box<Expression>>,
        field: String,
        class: String,
    },
    /// Array access (e.g., arr[index])
    ArrayAccess {
        array: Box<Expression>,
        index: Box<Expression>,
    },
    /// Assignment (e.g., x = value)
    Assignment {
        target: Box<Expression>,
        value: Box<Expression>,
    },
    /// Lambda expression (e.g., `x -> x + 1` or `x -> { return x + 1; }`).
    Lambda {
        params: Vec<String>,
        body: Box<LambdaBody>,
        target: Option<FunctionalTarget>,
    },
    /// Method/constructor reference, target-typed by its surrounding SAM.
    MethodReference {
        qualifier: Box<Expression>,
        method: String,
        is_constructor: bool,
        target: Option<FunctionalTarget>,
    },
    /// Conditional / ternary expression `cond ? then : els`.
    Ternary {
        condition: Box<Expression>,
        then_expr: Box<Expression>,
        else_expr: Box<Expression>,
    },
    /// Array initializer `{ e0, e1, ... }`. `element_type` is the declared array
    /// element type when known (e.g. from `int[] a = {…}`); otherwise `None`
    /// and the backend infers it from the elements.
    ArrayInit {
        element_type: Option<JavaType>,
        elements: Vec<Expression>,
    },
}

/// The two body forms permitted by the Java grammar for a lambda expression.
///
/// Keeping blocks as statements preserves declarations, control flow, and
/// side effects instead of lossy conversion to a single expression.
#[derive(Debug, Clone, PartialEq)]
pub enum LambdaBody {
    Expression(Box<Expression>),
    Block(Vec<Statement>),
}

/// Target functional-interface information retained for a poly expression.
///
/// Java lambda parameters can omit their types because the surrounding SAM
/// supplies them. The JVM descriptor remains erased, while these source-level
/// types drive unboxing, overload selection, and method-reference lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionalTarget {
    pub interface: String,
    pub sam_method: String,
    pub parameter_types: Vec<JavaType>,
    pub return_type: JavaType,
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    LeftShift,
    RightShift,
    UnsignedRightShift,
    LogicalAnd,
    LogicalOr,
    Equal,
    NotEqual,
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,     // -x for numeric, unary minus
    LogicalNot, // !x
    BitwiseNot, // ~x
}

/// Represents a Java statement for compilation
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// Expression statement (e.g., method call, assignment)
    Expression(Expression),
    /// Variable declaration with optional initializer
    VariableDeclaration {
        name: String,
        var_type: JavaType,
        /// Source generic arguments retained for expression typing while the
        /// local's JVM descriptor remains erased.
        generic_arguments: Vec<JavaType>,
        initializer: Option<Expression>,
    },
    /// If statement
    If {
        condition: Expression,
        then_stmt: Box<Statement>,
        else_stmt: Option<Box<Statement>>,
    },
    /// While loop
    While {
        condition: Expression,
        body: Box<Statement>,
    },
    /// For loop
    For {
        init: Option<Box<Statement>>,
        condition: Option<Expression>,
        update: Option<Expression>,
        body: Box<Statement>,
    },
    /// Block of statements
    Block(Vec<Statement>),
    /// A scope-transparent group of statements compiled in the *enclosing*
    /// scope (unlike `Block`, which opens a new one). Used for a single
    /// multi-declarator local declaration, e.g. `int a = 1, b = 2;`, so each
    /// declared variable lives in the same scope the declaration appears in.
    Multi(Vec<Statement>),
    /// Return statement
    Return(Option<Expression>),
    /// Try-catch statement
    Try {
        body: Box<Statement>,
        catches: Vec<CatchClause>,
        finally_block: Option<Box<Statement>>,
    },
    /// Throw statement
    Throw(Expression),
    /// `do { body } while (condition);` — body runs before the first test.
    DoWhile {
        condition: Expression,
        body: Box<Statement>,
    },
    /// `break;` (optional target label for labelled loops).
    Break(Option<String>),
    /// `continue;` (optional target label for labelled loops).
    Continue(Option<String>),
    /// `switch (scrutinee) { case … }` (traditional colon form, int/char keys).
    Switch {
        scrutinee: Expression,
        cases: Vec<SwitchCase>,
    },
    /// Enhanced-for `for (T x : iterable) body`. Kept as a node (rather than
    /// desugared in the converter) because the lowering depends on the
    /// iterable's *type*, which only the backend resolves: an array becomes an
    /// index loop, a Collection/Iterable an iterator loop.
    ForEach {
        elem_type: JavaType,
        var_name: String,
        iterable: Expression,
        body: Box<Statement>,
    },
    /// An explicit constructor invocation — `this(args)` or `super(args)` — as
    /// the first statement of a constructor body. `is_super` selects the target
    /// (`super` → the superclass `<init>`, otherwise this class's `<init>`).
    ExplicitConstructorCall {
        is_super: bool,
        args: Vec<Expression>,
    },
}

/// One `case`/`default` arm of a traditional `switch`. Statements fall through
/// to the next arm unless terminated by `break`.
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    /// Constant key values for this arm (empty ⇒ `default`).
    pub values: Vec<i32>,
    /// Enum constant names used as labels. The backend turns them into ordinals against the
    /// scrutinee's enum type.
    pub names: Vec<String>,
    /// Whether this is the `default` arm.
    pub is_default: bool,
    /// Arm body statements.
    pub body: Vec<Statement>,
}

/// A formal parameter of a method.
#[derive(Debug, Clone)]
pub struct Param {
    /// Parameter name (bound as a local variable in the method body)
    pub name: String,
    /// Declared type
    pub ty: JavaType,
}

/// A method declaration within a class.
#[derive(Debug, Clone)]
pub struct MethodDecl {
    /// Method name (`<init>` for constructors)
    pub name: String,
    /// Formal parameters in declaration order
    pub params: Vec<Param>,
    /// Return type (`Void` for `void` methods and constructors)
    pub return_type: JavaType,
    /// Whether the method is `static`
    pub is_static: bool,
    /// Method body statements (empty for abstract/native methods)
    pub body: Vec<Statement>,
    /// Declared with no body: an abstract method, or an interface method without `default`.
    pub is_abstract: bool,
}

impl MethodDecl {
    /// Build the JVM method descriptor string, e.g. `(II)I`.
    pub fn descriptor(&self) -> String {
        let mut s = String::from("(");
        for p in &self.params {
            s.push_str(&p.ty.descriptor());
        }
        s.push(')');
        s.push_str(&self.return_type.descriptor());
        s
    }
}

/// A field declaration within a class.
#[derive(Debug, Clone)]
pub struct FieldDecl {
    /// Field name
    pub name: String,
    /// Declared type
    pub ty: JavaType,
    /// Whether the field is `static`
    pub is_static: bool,
}

/// A class declaration — the top-level compilation unit for the Java backend.
#[derive(Debug, Clone)]
pub struct ClassDecl {
    /// Internal class name (slash-separated), e.g. `com/example/Main`
    pub name: String,
    /// Internal name of the superclass
    pub super_name: String,
    /// Declared fields
    pub fields: Vec<FieldDecl>,
    /// Declared methods (excluding the synthesized default `<init>`)
    pub methods: Vec<MethodDecl>,
    /// Static fields of the classes that enclose this one, each with its owner's internal name.
    /// A nested class names them by simple name, as Java does.
    pub enclosing_static_fields: Vec<(String, FieldDecl)>,
    /// Assignments of the static field initialisers and the static blocks, in source order.
    /// They form the class's `<clinit>`.
    pub static_init: Vec<Statement>,
    /// Assignments of the instance field initialisers and the instance blocks, in source order.
    /// A constructor that does not begin with `this(...)` runs them after its superclass call.
    pub instance_init: Vec<Statement>,
    /// Whether this is an interface: a `.class` with ACC_INTERFACE, whose methods are abstract or default.
    pub is_interface: bool,
    /// Internal names of the interfaces this class implements (or, for an interface, extends).
    pub interfaces: Vec<String>,
    /// Whether this is an annotation type: an interface with ACC_ANNOTATION.
    pub is_annotation: bool,
    /// Whether the class is declared `abstract`. A class with an abstract method, such as
    /// an enum whose constants implement it, is abstract too: see `ClassDecl::is_abstract`.
    pub declared_abstract: bool,
    /// The types named as type arguments in the `extends` and `implements` clauses, erased as the
    /// signatures are: `A` for `Comparable<A>`. A supertype method whose parameter erases to `Object`
    /// is overridden at the declared type when that type is one of these.
    pub supertype_arguments: Vec<JavaType>,
}

impl ClassDecl {
    /// Whether the class file has ACC_ABSTRACT, as javac sets it: for an interface, a class
    /// declared `abstract`, and a class with an abstract method.
    pub fn is_abstract(&self) -> bool {
        self.is_interface || self.declared_abstract || self.methods.iter().any(|m| m.is_abstract)
    }
}

/// Catch clause in try-catch statement
#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    /// Exception type (e.g., "java/lang/Exception")
    pub exception_type: String,
    /// Variable name to bind caught exception
    pub variable: String,
    /// Catch body
    pub body: Statement,
}

/// Context for local variables during compilation
#[derive(Debug, Clone)]
pub struct VariableContext {
    /// Maps variable name to (slot, type)
    variables: std::collections::HashMap<String, (u16, JavaType)>,
    /// Stack of scopes for block scoping
    scope_stack: Vec<std::collections::HashSet<String>>,
}

impl VariableContext {
    /// Create a new variable context
    pub fn new() -> Self {
        Self {
            variables: std::collections::HashMap::new(),
            scope_stack: vec![std::collections::HashSet::new()],
        }
    }

    /// Register a local variable
    pub fn add_variable(&mut self, name: String, slot: u16, ty: JavaType) {
        self.variables.insert(name.clone(), (slot, ty));
        // Track variable in current scope
        if let Some(scope) = self.scope_stack.last_mut() {
            scope.insert(name);
        }
    }

    /// Look up a variable
    pub fn lookup(&self, name: &str) -> Option<(u16, JavaType)> {
        self.variables.get(name).cloned()
    }

    /// Enter a new scope
    pub fn push_scope(&mut self) {
        self.scope_stack.push(std::collections::HashSet::new());
    }

    /// Exit current scope
    pub fn pop_scope(&mut self) {
        if let Some(scope) = self.scope_stack.pop() {
            // Remove variables from this scope
            for var_name in scope {
                self.variables.remove(&var_name);
            }
        }
    }
}

impl Default for VariableContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_depth_tracks_categories_and_reset() {
        let mut tracker = StackDepthTracker::new();
        tracker.push1();
        tracker.push2();
        tracker.pop1();
        assert_eq!(tracker.current_depth(), 2);
        assert_eq!(tracker.max_depth(), Ok(3));

        tracker.reset();
        assert_eq!(tracker.current_depth(), 0);
        assert_eq!(tracker.max_depth(), Ok(0));
    }

    #[test]
    fn stack_depth_underflow_is_sticky_and_never_panics() {
        let mut tracker = StackDepthTracker::new();
        tracker.pop1();
        tracker.push1();

        assert_eq!(tracker.current_depth(), 1);
        assert_eq!(tracker.max_depth(), Err(StackDepthError::Underflow));

        tracker.reset();
        assert_eq!(tracker.max_depth(), Ok(0));
    }

    #[test]
    fn stack_depth_rejects_values_that_do_not_fit_class_metadata() {
        let mut tracker = StackDepthTracker::new();
        for _ in 0..=u16::MAX {
            tracker.push1();
        }

        assert_eq!(
            tracker.max_depth(),
            Err(StackDepthError::LimitExceeded { depth: 65_536 })
        );
    }

    #[test]
    fn local_slots_respect_width_and_scope() {
        let mut table = LocalVariableTable::new();
        assert_eq!(table.allocate("first".to_string(), 0), 1);
        table.enter_scope();
        assert_eq!(table.allocate_wide("wide".to_string(), 3), 2);
        assert_eq!(table.next_slot(), 4);

        table.exit_scope(9);
        assert_eq!(table.lookup("first"), Some(1));
        assert_eq!(table.lookup("wide"), None);
        let wide = table
            .entries()
            .iter()
            .find(|entry| entry.name == "wide")
            .unwrap();
        assert_eq!((wide.start_pc, wide.end_pc), (3, 9));
    }

    #[test]
    fn java_types_preserve_jvm_width_and_assignability() {
        assert_eq!(JavaType::Int.descriptor(), "I");
        assert_eq!(
            JavaType::Array {
                element_type: Box::new(JavaType::Object("java.lang.String".to_string())),
                dimensions: 2,
            }
            .descriptor(),
            "[[Ljava/lang/String;"
        );
        assert_eq!(JavaType::Long.stack_width(), 2);
        assert!(JavaType::Long.is_assignable_from(&JavaType::Int));
        assert!(!JavaType::Int.is_assignable_from(&JavaType::Long));
    }

    #[test]
    fn label_allocator_returns_distinct_labels() {
        let mut allocator = LabelAllocator::new();
        assert_ne!(allocator.new_label(), allocator.new_label());
    }

    #[test]
    fn variable_context_removes_inner_scope_bindings() {
        let mut context = VariableContext::new();
        context.add_variable("outer".to_string(), 1, JavaType::Int);
        context.push_scope();
        context.add_variable("inner".to_string(), 2, JavaType::Long);
        assert_eq!(context.lookup("inner"), Some((2, JavaType::Long)));

        context.pop_scope();
        assert_eq!(context.lookup("inner"), None);
        assert_eq!(context.lookup("outer"), Some((1, JavaType::Int)));
    }
}
