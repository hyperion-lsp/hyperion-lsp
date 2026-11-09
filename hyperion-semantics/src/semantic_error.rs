// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::symbol::Type;
use rowan::TextRange;

/// Represents a semantic error detected during type checking or symbol resolution
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticError {
    /// The source code range where the error occurred
    pub range: TextRange,
    /// The kind of semantic error
    pub kind: SemanticErrorKind,
}

/// Different categories of semantic errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticErrorKind {
    /// Type mismatch in variable declaration: `String foo = 1;`
    DeclarationTypeMismatch {
        declared_type: Type,
        actual_type: Type,
        variable_name: String,
    },

    /// Type mismatch in variable assignment: `foo = "Hey";` where `foo` is `int`
    AssignmentTypeMismatch {
        variable_type: Type,
        assigned_type: Type,
        variable_name: String,
    },

    /// Attempting to assign null to a primitive type: `int x = null;`
    NullToPrimitive {
        primitive_type: Type,
        variable_name: String,
    },

    /// Undefined symbol reference
    UndefinedSymbol {
        symbol_name: String,
        suggested_symbol: Option<String>,
    },

    /// Unresolved Kotlin identifier reference.
    UnresolvedReference { symbol_name: String },

    /// A Kotlin value whose type provably cannot be the declared one.
    KotlinTypeMismatch { message: String },

    /// A Kotlin call whose argument count provably matches its only candidate.
    KotlinArgumentCount { message: String },

    /// Assignment to an immutable Kotlin local/property or parameter.
    ValReassignment { variable_name: String },

    /// Method not found
    MethodNotFound {
        method_name: String,
        receiver_type: Option<Type>,
        /// Display forms of the argument types (`None` where unknown), so the
        /// message can spell the call as javac does: `method foo(int, String)`.
        argument_types: Option<Vec<Option<String>>>,
    },

    /// Wrong number of arguments in method call
    WrongArgumentCount {
        method_name: String,
        expected: usize,
        actual: usize,
    },

    /// Method argument type mismatch
    MethodArgumentTypeMismatch {
        method_name: String,
        param_index: usize,
        expected_type: Type,
        actual_type: Type,
    },

    /// No constructor of the instantiated class accepts the arguments.
    ConstructorNotApplicable {
        class_name: String,
        /// Argument type displays (`None` where unknown).
        arguments: Vec<Option<String>>,
        /// Parameter type displays of every constructor that was considered.
        candidates: Vec<Vec<String>>,
    },

    /// `new Shape()` where `Shape` is an interface or abstract class.
    AbstractInstantiation { type_name: String },

    /// A concrete class that leaves an inherited abstract method unimplemented.
    AbstractMethodNotImplemented {
        type_name: String,
        /// `count()` / `greet(String)`
        method: String,
        owner: String,
    },

    /// A checked exception raised where nothing catches or declares it.
    UnreportedException { exception: String },

    /// A switch expression (or pattern switch statement) that misses values.
    NonExhaustiveSwitch { is_expression: bool },

    /// `int x` declared twice in overlapping scopes.
    DuplicateVariable { name: String, location: String },

    /// An instance member (or `this`) used from a static context.
    StaticContextReference { what: String },

    /// Assignment to a `final` variable that cannot legally be assigned.
    FinalAssignment { name: String, is_parameter: bool },

    /// `return x;` in a void method or `return;` in a value method.
    ReturnValueMismatch { detail: &'static str },

    /// A loop or `if` condition that is provably not boolean.
    ConditionTypeMismatch { actual_type: Type },

    /// `for (x : expr)` where `expr` is neither an array nor an `Iterable`.
    ForEachNotApplicable { actual_type: Type },

    /// `List<int>`: a primitive where a type argument must be a reference type.
    PrimitiveTypeArgument { type_name: String },

    /// `receiver.name` where the receiver's type has no such field.
    FieldNotFound { name: String, receiver_type: String },

    /// A member that exists but is not accessible from the use site (JLS 6.6).
    InaccessibleMember {
        /// `method`/`variable`/`constructor`
        member_kind: &'static str,
        /// Rendered as it appears in the message: `hid()`, `hidden`, `Other(int)`.
        member: String,
        owner: String,
        /// `private`, `protected` or `package-private`
        access: &'static str,
    },

    /// A value-returning method whose end can be reached (JLS 8.4.7).
    MissingReturn,

    /// A statement that no execution can reach (JLS 14.22).
    UnreachableStatement,

    /// Return type mismatch
    ReturnTypeMismatch {
        method_name: String,
        expected_type: Type,
        actual_type: Type,
    },

    /// Non-exhaustive when expression (Kotlin)
    /// When expression on a sealed type is missing branches
    NonExhaustiveWhen {
        /// The sealed type being matched
        sealed_type: String,
        /// The subclasses that are not covered
        missing_branches: Vec<String>,
    },

    /// Nullable type mismatch (Kotlin)
    /// Attempting to use member access on a nullable type without safe call
    NullableTypeMismatch {
        /// The nullable type being accessed
        nullable_type: String,
        /// Whether non-nullable access is required
        non_nullable_required: bool,
        /// The expression being accessed
        expression: String,
    },

    /// Extension function not in scope (Kotlin)
    /// Extension function is called but not imported or in scope
    ExtensionFunctionNotInScope {
        /// Name of the extension function
        function_name: String,
        /// Receiver type
        receiver_type: String,
    },

    /// Pattern type doesn't match switch expression type (Java 21+)
    /// Attempting to match a pattern whose type is incompatible with the switch expression
    PatternTypeMismatch {
        /// The type of the switch expression
        switch_type: String,
        /// The type in the pattern
        pattern_type: String,
    },

    /// Record pattern has wrong number of components (Java 21+)
    /// Pattern doesn't match the record's component count
    PatternComponentCountMismatch {
        /// Name of the record type
        record_name: String,
        /// Expected number of components
        expected: usize,
        /// Actual number in the pattern
        actual: usize,
    },

    /// Pattern component name doesn't exist in record (Java 21+)
    /// Referenced a component that doesn't exist
    UnknownRecordComponent {
        /// Name of the record type
        record_name: String,
        /// The component name that doesn't exist
        component_name: String,
        /// List of valid component names
        available_components: Vec<String>,
    },

    /// Type mismatch in pattern component (Java 21+)
    /// Pattern component type doesn't match record component type
    PatternComponentTypeMismatch {
        /// Component name
        component_name: String,
        /// Expected type (from record declaration)
        expected_type: String,
        /// Actual type in pattern (if specified)
        actual_pattern_type: Option<String>,
    },
}

/// `incompatible types: X cannot be converted to Y`, or javac's more helpful
/// `possible lossy conversion from X to Y` when both are numeric primitives.
fn conversion_message(actual: &Type, target: &Type) -> String {
    use crate::symbol::PrimitiveType;
    if let (Type::Primitive(from), Type::Primitive(to)) = (actual, target) {
        if *from != PrimitiveType::Boolean && *to != PrimitiveType::Boolean {
            return format!("incompatible types: possible lossy conversion from {from} to {to}");
        }
    }
    format!("incompatible types: {actual} cannot be converted to {target}")
}

impl SemanticError {
    /// Create a new semantic error
    pub const fn new(range: TextRange, kind: SemanticErrorKind) -> Self {
        Self { range, kind }
    }

    /// Get a human-readable error message
    pub fn message(&self) -> String {
        match &self.kind {
            SemanticErrorKind::DeclarationTypeMismatch {
                declared_type,
                actual_type,
                variable_name: _,
            } => {
                conversion_message(actual_type, declared_type)
            }

            SemanticErrorKind::AssignmentTypeMismatch {
                variable_type,
                assigned_type,
                variable_name: _,
            } => {
                conversion_message(assigned_type, variable_type)
            }

            SemanticErrorKind::NullToPrimitive {
                primitive_type,
                variable_name: _,
            } => {
                format!("incompatible types: cannot assign null to primitive type {primitive_type}")
            }

            SemanticErrorKind::UndefinedSymbol {
                symbol_name,
                suggested_symbol,
            } => suggested_symbol.as_ref().map_or_else(
                || format!("cannot find symbol: {symbol_name}"),
                |suggested| format!("cannot find symbol: {symbol_name} (did you mean {suggested}?)"),
            ),

            SemanticErrorKind::UnresolvedReference { symbol_name } => {
                format!("unresolved reference: {symbol_name}")
            }

            SemanticErrorKind::KotlinTypeMismatch { message }
            | SemanticErrorKind::KotlinArgumentCount { message } => message.clone(),

            SemanticErrorKind::ValReassignment { variable_name: _ } => {
                "val cannot be reassigned".to_string()
            }

            SemanticErrorKind::MethodNotFound {
                method_name,
                receiver_type,
                argument_types,
            } => {
                let arguments = argument_types.as_ref().map_or_else(String::new, |types| {
                    types
                        .iter()
                        .map(|ty| ty.clone().unwrap_or_else(|| "?".to_owned()))
                        .collect::<Vec<_>>()
                        .join(", ")
                });
                receiver_type.as_ref().map_or_else(
                    || format!("cannot find symbol: method {method_name}({arguments})"),
                    |typ| {
                        let typ = match typ {
                            Type::Class(class) => class.raw_qualified_name(),
                            other => other.to_string(),
                        };
                        format!(
                            "cannot find symbol: method {method_name}({arguments}) in type {typ}"
                        )
                    },
                )
            }

            SemanticErrorKind::WrongArgumentCount {
                method_name,
                expected,
                actual,
            } => {
                format!(
                    "method {method_name}() expects {expected} arguments but {actual} were provided"
                )
            }

            SemanticErrorKind::MethodArgumentTypeMismatch {
                method_name,
                param_index,
                expected_type,
                actual_type,
            } => {
                format!(
                    "method {}() argument {}: {}",
                    method_name,
                    param_index + 1,
                    conversion_message(actual_type, expected_type)
                )
            }

            SemanticErrorKind::ConstructorNotApplicable {
                class_name,
                arguments,
                candidates,
            } => {
                let found = if arguments.is_empty() {
                    "no arguments".to_owned()
                } else {
                    arguments
                        .iter()
                        .map(|argument| argument.clone().unwrap_or_else(|| "?".to_owned()))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                match candidates.as_slice() {
                    [only] => {
                        let required = if only.is_empty() {
                            "no arguments".to_owned()
                        } else {
                            only.join(", ")
                        };
                        format!(
                            "constructor {class_name} in class {class_name} cannot be applied to given types (required: {required}; found: {found})"
                        )
                    }
                    _ => {
                        let signature = if arguments.is_empty() {
                            String::new()
                        } else {
                            found
                        };
                        format!("no suitable constructor found for {class_name}({signature})")
                    }
                }
            }

            SemanticErrorKind::AbstractInstantiation { type_name } => {
                format!("{type_name} is abstract; cannot be instantiated")
            }

            SemanticErrorKind::AbstractMethodNotImplemented {
                type_name,
                method,
                owner,
            } => format!(
                "{type_name} is not abstract and does not override abstract method {method} in {owner}"
            ),

            SemanticErrorKind::UnreportedException { exception } => format!(
                "unreported exception {exception}; must be caught or declared to be thrown"
            ),

            SemanticErrorKind::NonExhaustiveSwitch { is_expression } => format!(
                "the switch {} does not cover all possible input values",
                if *is_expression { "expression" } else { "statement" }
            ),

            SemanticErrorKind::DuplicateVariable { name, location } => {
                format!("variable {name} is already defined in {location}")
            }

            SemanticErrorKind::StaticContextReference { what } => {
                format!("non-static {what} cannot be referenced from a static context")
            }

            SemanticErrorKind::FinalAssignment { name, is_parameter } => {
                if *is_parameter {
                    format!("final parameter {name} may not be assigned")
                } else {
                    format!("cannot assign a value to final variable {name}")
                }
            }

            SemanticErrorKind::ReturnValueMismatch { detail } => {
                format!("incompatible types: {detail}")
            }

            SemanticErrorKind::ConditionTypeMismatch { actual_type } => {
                format!("incompatible types: {actual_type} cannot be converted to boolean")
            }

            SemanticErrorKind::ForEachNotApplicable { actual_type } => format!(
                "for-each not applicable to expression type (required: array or java.lang.Iterable; found: {actual_type})"
            ),

            SemanticErrorKind::PrimitiveTypeArgument { type_name } => {
                format!("unexpected type; required: reference; found: {type_name}")
            }

            SemanticErrorKind::FieldNotFound {
                name,
                receiver_type,
            } => format!("cannot find symbol: variable {name} in type {receiver_type}"),

            SemanticErrorKind::InaccessibleMember {
                member,
                owner,
                access,
                ..
            } => match *access {
                "package-private" => {
                    format!("{member} is not public in {owner}; cannot be accessed from outside package")
                }
                other => format!("{member} has {other} access in {owner}"),
            },

            SemanticErrorKind::MissingReturn => "missing return statement".to_string(),

            SemanticErrorKind::UnreachableStatement => "unreachable statement".to_string(),

            SemanticErrorKind::ReturnTypeMismatch {
                method_name,
                expected_type,
                actual_type,
            } => {
                let _ = method_name;
                conversion_message(actual_type, expected_type)
            }

            SemanticErrorKind::NonExhaustiveWhen {
                sealed_type,
                missing_branches,
            } => {
                let missing = missing_branches.join(", ");
                format!(
                    "'when' expression on sealed type {sealed_type} must be exhaustive. Missing branches: {missing}"
                )
            }

            SemanticErrorKind::NullableTypeMismatch {
                nullable_type,
                non_nullable_required,
                expression,
            } => {
                if *non_nullable_required {
                    format!(
                        "only safe (?.) or non-null asserted (!!.) calls are allowed on a nullable receiver of type {nullable_type}"
                    )
                } else {
                    format!("unsafe member access on nullable type {nullable_type}: {expression}")
                }
            }

            SemanticErrorKind::ExtensionFunctionNotInScope {
                function_name,
                receiver_type,
            } => {
                format!(
                    "unresolved reference: extension function '{function_name}' for type {receiver_type} is not in scope. Did you forget to import it?"
                )
            }

            SemanticErrorKind::PatternTypeMismatch {
                switch_type,
                pattern_type,
            } => {
                format!(
                    "pattern type '{pattern_type}' does not match switch expression type '{switch_type}'"
                )
            }

            SemanticErrorKind::PatternComponentCountMismatch {
                record_name,
                expected,
                actual,
            } => {
                format!(
                    "record '{}' expects {} component{} but pattern has {}",
                    record_name,
                    expected,
                    if *expected == 1 { "" } else { "s" },
                    actual
                )
            }

            SemanticErrorKind::UnknownRecordComponent {
                record_name,
                component_name,
                available_components,
            } => {
                let available = available_components.join(", ");
                format!(
                    "record '{record_name}' has no component named '{component_name}'. Available components: {available}"
                )
            }

            SemanticErrorKind::PatternComponentTypeMismatch {
                component_name,
                expected_type,
                actual_pattern_type,
            } => actual_pattern_type.as_ref().map_or_else(
                || format!(
                    "component '{component_name}' has type '{expected_type}' but pattern type is incompatible"
                ),
                |actual| format!(
                    "component '{component_name}' has type '{expected_type}' but pattern expects '{actual}'"
                ),
            ),
        }
    }

    /// Get a short description of the error kind for diagnostics
    pub const fn kind_name(&self) -> &'static str {
        match &self.kind {
            SemanticErrorKind::DeclarationTypeMismatch { .. } => "type-mismatch-declaration",
            SemanticErrorKind::AssignmentTypeMismatch { .. } => "type-mismatch-assignment",
            SemanticErrorKind::NullToPrimitive { .. } => "null-to-primitive",
            SemanticErrorKind::UndefinedSymbol { .. } => "undefined-symbol",
            SemanticErrorKind::UnresolvedReference { .. } => "unresolved-reference",
            SemanticErrorKind::KotlinTypeMismatch { .. } => "kotlin-type-mismatch",
            SemanticErrorKind::KotlinArgumentCount { .. } => "kotlin-argument-count",
            SemanticErrorKind::ValReassignment { .. } => "val-reassignment",
            SemanticErrorKind::MethodNotFound { .. } => "method-not-found",
            SemanticErrorKind::WrongArgumentCount { .. } => "wrong-argument-count",
            SemanticErrorKind::MethodArgumentTypeMismatch { .. } => "method-argument-type-mismatch",
            SemanticErrorKind::ConstructorNotApplicable { .. } => "constructor-not-applicable",
            SemanticErrorKind::AbstractInstantiation { .. } => "abstract-instantiation",
            SemanticErrorKind::AbstractMethodNotImplemented { .. } => {
                "abstract-method-not-implemented"
            }
            SemanticErrorKind::UnreportedException { .. } => "unreported-exception",
            SemanticErrorKind::NonExhaustiveSwitch { .. } => "non-exhaustive-switch",
            SemanticErrorKind::DuplicateVariable { .. } => "duplicate-variable",
            SemanticErrorKind::StaticContextReference { .. } => "static-context-reference",
            SemanticErrorKind::FinalAssignment { .. } => "final-assignment",
            SemanticErrorKind::ReturnValueMismatch { .. } => "return-value-mismatch",
            SemanticErrorKind::ConditionTypeMismatch { .. } => "condition-type-mismatch",
            SemanticErrorKind::ForEachNotApplicable { .. } => "foreach-not-applicable",
            SemanticErrorKind::PrimitiveTypeArgument { .. } => "primitive-type-argument",
            SemanticErrorKind::FieldNotFound { .. } => "field-not-found",
            SemanticErrorKind::InaccessibleMember { .. } => "inaccessible-member",
            SemanticErrorKind::MissingReturn => "missing-return",
            SemanticErrorKind::UnreachableStatement => "unreachable-statement",
            SemanticErrorKind::ReturnTypeMismatch { .. } => "return-type-mismatch",
            SemanticErrorKind::NonExhaustiveWhen { .. } => "non-exhaustive-when",
            SemanticErrorKind::NullableTypeMismatch { .. } => "nullable-type-mismatch",
            SemanticErrorKind::ExtensionFunctionNotInScope { .. } => {
                "extension-function-not-in-scope"
            }
            SemanticErrorKind::PatternTypeMismatch { .. } => "pattern-type-mismatch",
            SemanticErrorKind::PatternComponentCountMismatch { .. } => {
                "pattern-component-count-mismatch"
            }
            SemanticErrorKind::UnknownRecordComponent { .. } => "unknown-record-component",
            SemanticErrorKind::PatternComponentTypeMismatch { .. } => {
                "pattern-component-type-mismatch"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::{ClassType, PrimitiveType, Type};
    use rowan::{TextRange, TextSize};

    #[test]
    fn test_declaration_type_mismatch_message() {
        let error = SemanticError::new(
            TextRange::new(TextSize::from(0), TextSize::from(10)),
            SemanticErrorKind::DeclarationTypeMismatch {
                declared_type: Type::Class(ClassType::string()),
                actual_type: Type::Primitive(PrimitiveType::Int),
                variable_name: "foo".into(),
            },
        );

        assert_eq!(
            error.message(),
            "incompatible types: int cannot be converted to java.lang.String"
        );
    }

    #[test]
    fn test_assignment_type_mismatch_message() {
        let error = SemanticError::new(
            TextRange::new(TextSize::from(0), TextSize::from(10)),
            SemanticErrorKind::AssignmentTypeMismatch {
                variable_type: Type::Primitive(PrimitiveType::Int),
                assigned_type: Type::Class(ClassType::string()),
                variable_name: "foo".into(),
            },
        );

        assert_eq!(
            error.message(),
            "incompatible types: java.lang.String cannot be converted to int"
        );
    }

    #[test]
    fn test_null_to_primitive_message() {
        let error = SemanticError::new(
            TextRange::new(TextSize::from(0), TextSize::from(10)),
            SemanticErrorKind::NullToPrimitive {
                primitive_type: Type::Primitive(PrimitiveType::Int),
                variable_name: "x".into(),
            },
        );

        assert_eq!(
            error.message(),
            "incompatible types: cannot assign null to primitive type int"
        );
    }
}
