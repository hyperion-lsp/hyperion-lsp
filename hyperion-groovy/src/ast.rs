// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Common AST types for Gradle DSL parsing
//!
//! These types represent common patterns found in Gradle build files.

use std::collections::HashMap;
use std::fmt;

/// Represents a method call in Gradle DSL
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodCall {
    /// Method name (e.g., "implementation", "mavenCentral")
    pub name: String,
    /// Arguments to the method (simplified for MVP)
    pub args: Vec<MethodArgument>,
    /// Line number where method call appears
    pub line: usize,
    /// Column number where method call starts
    pub column: usize,
}

/// Argument to a method call
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MethodArgument {
    /// String literal: "com.example:lib:1.0"
    StringLiteral(String),
    /// GString: "${version}" - contains interpolation
    GString(String),
    /// Closure block: { ... }
    Closure,
    /// Map literal: [key: value]
    MapLiteral(HashMap<String, String>),
    /// Identifier/variable reference
    Identifier(String),
}

/// Represents a Gradle DSL block (closure)
#[derive(Clone, Debug)]
pub struct GradleBlock {
    /// Block name (e.g., "dependencies", "repositories", "tasks")
    pub name: String,
    /// Methods called within this block
    pub methods: Vec<MethodCall>,
    /// Nested blocks (for multi-level nesting)
    pub nested_blocks: Vec<GradleBlock>,
    /// Line range
    pub start_line: usize,
    pub end_line: usize,
}

/// Represents dependency specification
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// Group ID (Maven coordinates: group:artifact:version)
    pub group: String,
    /// Artifact ID
    pub artifact: String,
    /// Version
    pub version: String,
}

impl Dependency {
    /// Parse Maven coordinate string "group:artifact:version"
    pub fn from_string(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() >= 3 {
            Some(Dependency {
                group: parts[0].to_string(),
                artifact: parts[1].to_string(),
                version: parts[2].to_string(),
            })
        } else {
            None
        }
    }
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.group, self.artifact, self.version)
    }
}

/// Represents a repository configuration
#[derive(Clone, Debug)]
pub struct Repository {
    /// Repository type ("mavenCentral", "mavenLocal", "custom", etc.)
    pub repo_type: String,
    /// Repository URL (for custom repositories)
    pub url: Option<String>,
    /// Credentials (if any)
    pub credentials: Option<Credentials>,
}

/// Repository credentials
#[derive(Clone, Debug)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Groovy expression types
#[derive(Clone, Debug, PartialEq)]
pub enum Expression {
    // Literals
    IntLiteral(i64),
    LongLiteral(i64),
    FloatLiteral(f64),
    DoubleLiteral(f64),
    BooleanLiteral(bool),
    StringLiteral(String),
    NullLiteral,

    // Collections
    ListLiteral(Vec<Expression>),
    MapLiteral(Vec<(String, Expression)>),
    RangeLiteral {
        start: Box<Expression>,
        end: Box<Expression>,
        exclusive: bool,
    },

    // GString with interpolations
    GString(Vec<GStringPart>),

    // Variables and identifiers
    Identifier(String),
    Variable(String),

    // Property access (user.name, obj?.prop)
    PropertyAccess {
        object: Box<Expression>,
        property: String,
        safe_navigation: bool,
    },

    // Array/map access (arr[0], map["key"])
    IndexAccess {
        object: Box<Expression>,
        index: Box<Expression>,
    },

    // Method calls
    MethodCall {
        receiver: Option<Box<Expression>>,
        method_name: String,
        arguments: Vec<Expression>,
    },

    // Binary operations
    BinaryOp {
        operator: String,
        left: Box<Expression>,
        right: Box<Expression>,
    },

    // Unary operations
    UnaryOp {
        operator: String,
        operand: Box<Expression>,
    },

    // Ternary conditional
    Conditional {
        condition: Box<Expression>,
        then_expr: Box<Expression>,
        else_expr: Box<Expression>,
    },

    // Elvis operator (a ?: b)
    ElvisOp {
        primary: Box<Expression>,
        fallback: Box<Expression>,
    },

    // Closure expression
    Closure(Box<ClosureExpr>),

    // Parsed syntax that must remain visible until a faithful lowering exists.
    Unsupported(String),

    // Parenthesized expression
    Parenthesized(Box<Expression>),

    // New object creation: new ClassName(args)
    NewObject {
        class_name: String,
        arguments: Vec<Expression>,
    },
}

/// Parts of a GString
#[derive(Clone, Debug, PartialEq)]
pub enum GStringPart {
    Text(String),
    Interpolation(Box<Expression>),
}

/// Closure expression with parameters and body
#[derive(Clone, Debug, PartialEq)]
pub struct ClosureExpr {
    pub parameters: Vec<String>,
    pub body: Vec<Statement>,
    pub implicit_it: bool, // true if no parameters (uses implicit 'it')
}

/// Groovy statement types
#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    // Expression statement
    Expression(Expression),

    // Variable declaration
    VariableDecl {
        name: String,
        type_annotation: Option<String>,
        initializer: Option<Expression>,
    },

    // If/else statement
    If {
        condition: Expression,
        then_body: Vec<Statement>,
        else_body: Option<Vec<Statement>>,
    },

    // While loop
    While {
        condition: Expression,
        body: Vec<Statement>,
    },

    // For loop
    For {
        variable: String,
        iterable: Expression,
        body: Vec<Statement>,
    },

    // Traditional for loop: for (int i = 0; i < n; i++)
    TraditionalFor {
        init: Option<Box<Statement>>,
        condition: Option<Expression>,
        increment: Option<Expression>,
        body: Vec<Statement>,
    },

    // Switch statement
    Switch {
        expression: Expression,
        cases: Vec<SwitchCase>,
        default_case: Option<Vec<Statement>>,
    },

    // Try/catch/finally
    Try {
        body: Vec<Statement>,
        catch_blocks: Vec<CatchBlock>,
        finally_block: Option<Vec<Statement>>,
    },

    // Throw statement
    Throw(Expression),

    // Return statement
    Return(Option<Expression>),

    // Break statement
    Break,

    // Continue statement
    Continue,

    // Block
    Block(Vec<Statement>),

    // Method definition
    MethodDef {
        name: String,
        parameters: Vec<Parameter>,
        return_type: Option<String>,
        body: Vec<Statement>,
    },

    // Parsed statement whose semantics are intentionally not lowered yet.
    Unsupported(String),
}

/// Switch case
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchCase {
    pub pattern: Expression,
    pub body: Vec<Statement>,
}

/// Catch block
#[derive(Clone, Debug, PartialEq)]
pub struct CatchBlock {
    pub exception_type: String,
    pub variable_name: String,
    pub body: Vec<Statement>,
}

/// Method/closure parameter
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub type_annotation: Option<String>,
    pub default_value: Option<String>, // For simplicity, store as string
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_maven_dependency() {
        let dep = Dependency::from_string("com.google.guava:guava:31.1-jre");
        assert!(dep.is_some());
        let dep = dep.unwrap();
        assert_eq!(dep.group, "com.google.guava");
        assert_eq!(dep.artifact, "guava");
        assert_eq!(dep.version, "31.1-jre");
    }

    #[test]
    fn dependency_to_string() {
        let dep = Dependency {
            group: "junit".to_string(),
            artifact: "junit".to_string(),
            version: "4.13".to_string(),
        };
        assert_eq!(dep.to_string(), "junit:junit:4.13");
    }

    #[test]
    fn gradle_block_structure() {
        let block = GradleBlock {
            name: "dependencies".to_string(),
            methods: vec![MethodCall {
                name: "implementation".to_string(),
                args: vec![MethodArgument::StringLiteral(
                    "junit:junit:4.13".to_string(),
                )],
                line: 5,
                column: 4,
            }],
            nested_blocks: vec![],
            start_line: 4,
            end_line: 6,
        };
        assert_eq!(block.name.as_str(), "dependencies");
        assert_eq!(block.methods.len(), 1);
        assert_eq!(block.methods[0].name, "implementation");
    }

    #[test]
    fn expression_literals() {
        let _int_lit = Expression::IntLiteral(42);
        let _str_lit = Expression::StringLiteral("hello".to_string());
        let _bool_lit = Expression::BooleanLiteral(true);
        let _null_lit = Expression::NullLiteral;
    }

    #[test]
    fn expression_property_access() {
        let obj = Box::new(Expression::Identifier("user".to_string()));
        let prop_access = Expression::PropertyAccess {
            object: obj,
            property: "name".to_string(),
            safe_navigation: false,
        };
        assert_eq!(
            prop_access,
            Expression::PropertyAccess {
                object: Box::new(Expression::Identifier("user".to_string())),
                property: "name".to_string(),
                safe_navigation: false,
            }
        );
    }

    #[test]
    fn expression_method_call() {
        let call = Expression::MethodCall {
            receiver: Some(Box::new(Expression::Identifier("list".to_string()))),
            method_name: "size".to_string(),
            arguments: vec![],
        };
        match call {
            Expression::MethodCall { method_name, .. } => {
                assert_eq!(method_name, "size".to_string())
            }
            _ => panic!("Expected method call"),
        }
    }

    #[test]
    fn expression_gstring() {
        let gstring = Expression::GString(vec![
            GStringPart::Text("Hello ".to_string()),
            GStringPart::Interpolation(Box::new(Expression::Identifier("name".to_string()))),
        ]);
        match gstring {
            Expression::GString(parts) => assert_eq!(parts.len(), 2),
            _ => panic!("Expected GString"),
        }
    }

    #[test]
    fn statement_if_else() {
        let cond = Expression::BooleanLiteral(true);
        let then_body = vec![Statement::Expression(Expression::IntLiteral(1))];
        let else_body = Some(vec![Statement::Expression(Expression::IntLiteral(0))]);

        let if_stmt = Statement::If {
            condition: cond,
            then_body,
            else_body,
        };
        match if_stmt {
            Statement::If { then_body, .. } => assert_eq!(then_body.len(), 1),
            _ => panic!("Expected if statement"),
        }
    }

    #[test]
    fn statement_variable_decl() {
        let var_decl = Statement::VariableDecl {
            name: "x".to_string(),
            type_annotation: Some("int".to_string()),
            initializer: Some(Expression::IntLiteral(42)),
        };
        match var_decl {
            Statement::VariableDecl { name, .. } => assert_eq!(name, "x".to_string()),
            _ => panic!("Expected variable declaration"),
        }
    }

    #[test]
    fn closure_expression() {
        let closure = ClosureExpr {
            parameters: vec!["x".to_string()],
            body: vec![Statement::Expression(Expression::Identifier(
                "x".to_string(),
            ))],
            implicit_it: false,
        };
        assert_eq!(closure.parameters.len(), 1);
        assert_eq!(closure.parameters[0], "x");
    }
}
