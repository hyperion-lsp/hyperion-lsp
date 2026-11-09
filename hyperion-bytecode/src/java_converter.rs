// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java CST → AST Converter
//!
//! Converts hyperion_parser's Concrete Syntax Tree (CST) into the simplified
//! AST types used by the bytecode compiler.

use crate::java_compiler::{
    BinaryOp, CatchClause, ClassDecl, Expression, FieldDecl, FunctionalTarget, JavaType,
    LambdaBody, MethodDecl, Param, Statement, UnaryOp,
};
use crate::producer::{BytecodeError, BytecodeResult};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use std::collections::HashSet;

/// Helper to create a parse error
fn parse_error(message: impl Into<String>) -> BytecodeError {
    BytecodeError::ParseError {
        file: String::from("<unknown>"),
        line: 0,
        column: 0,
        message: message.into(),
    }
}

/// Per-compilation-unit context used to resolve a simple class name to its
/// internal (`pkg/Name`) form: the file's `import`s, its wildcard-imported
/// packages, and the set of class names *declared in the unit*. Stored in a
/// thread-local so `qualify_class_name` (called deep in `convert_type`) can see
/// it without threading the context through every conversion signature. Each
/// unit overwrites it at the start of conversion; conversion is synchronous per
/// thread, so there is no cross-unit bleed.
#[derive(Default)]
struct UnitCtx {
    /// Explicit single-type imports: simple name → internal FQN.
    imports: std::collections::HashMap<String, String>,
    /// Wildcard-imported packages in internal form (e.g. `java/util`).
    wildcards: Vec<String>,
    /// Simple names of types declared in this unit (never qualified to stdlib).
    unit_classes: HashSet<String>,
}

thread_local! {
    static UNIT_CTX: std::cell::RefCell<UnitCtx> = std::cell::RefCell::new(UnitCtx::default());
}

/// Simple names of `java.util` types Hyperion can model (heap-backed natives or
/// stubs). A wildcard `import java.util.*` only qualifies names in this set, so
/// a user class with an unrelated name is never mis-qualified.
const JAVA_UTIL_TYPES: &[&str] = &[
    "ArrayList",
    "LinkedList",
    "HashMap",
    "LinkedHashMap",
    "TreeMap",
    "HashSet",
    "LinkedHashSet",
    "TreeSet",
    "List",
    "Map",
    "Set",
    "Collection",
    "Queue",
    "Deque",
    "ArrayDeque",
    "Stack",
    "Vector",
    "PriorityQueue",
    "Iterator",
    "Optional",
    "Arrays",
    "Collections",
    "Objects",
    "Comparator",
    "Random",
];

/// Populate the thread-local [`UnitCtx`] from a compilation-unit root: scan its
/// `import` declarations and the names of all types it declares.
fn install_unit_ctx(root: &SyntaxNode) {
    let mut ctx = UnitCtx::default();
    for child in root.children() {
        if child.kind() == SyntaxKind::IMPORT_DECL {
            // The QUALIFIED_NAME holds the dotted path; a STAR sibling marks
            // a wildcard (`import java.util.*`). `import static …` members are
            // not type imports — skip them (they carry a STATIC_KW).
            let is_static = child
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .any(|t| t.kind() == SyntaxKind::STATIC_KW);
            if is_static {
                continue;
            }
            let wildcard = child
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .any(|t| t.kind() == SyntaxKind::STAR);
            let Some(qn) = child
                .children()
                .find(|c| c.kind() == SyntaxKind::QUALIFIED_NAME)
            else {
                continue;
            };
            let segs: Vec<String> = qn
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .map(|t| t.text().to_string())
                .collect();
            if wildcard {
                if !segs.is_empty() {
                    ctx.wildcards.push(segs.join("/"));
                }
            } else if let Some((simple, pkg)) = segs.split_last() {
                if !pkg.is_empty() {
                    ctx.imports
                        .insert(simple.clone(), format!("{}/{simple}", pkg.join("/")));
                }
            }
        }
    }
    for decl in root.descendants() {
        if matches!(
            decl.kind(),
            SyntaxKind::CLASS_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
        ) {
            if let Some(name) = decl
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT)
            {
                ctx.unit_classes.insert(name.text().to_string());
            }
        }
    }
    UNIT_CTX.with(|c| *c.borrow_mut() = ctx);
}

/// Convert a source file CST node to statements
pub fn convert_source_file(node: SyntaxNode) -> BytecodeResult<Vec<Statement>> {
    let mut statements = Vec::new();

    // Iterate through children looking for class declarations
    for child in node.children() {
        if child.kind() == SyntaxKind::CLASS_DECL {
            // Extract methods from class
            extract_class_methods(child, &mut statements)?;
        }
    }

    Ok(statements)
}

fn extract_class_methods(
    class_node: SyntaxNode,
    statements: &mut Vec<Statement>,
) -> BytecodeResult<()> {
    for child in class_node.children() {
        if child.kind() == SyntaxKind::METHOD_DECL {
            // Find method body
            for method_child in child.children() {
                if method_child.kind() == SyntaxKind::BLOCK_STMT {
                    let block_statements = convert_block(method_child)?;
                    statements.extend(block_statements);
                }
            }
        }
    }
    Ok(())
}

fn convert_block(node: SyntaxNode) -> BytecodeResult<Vec<Statement>> {
    let mut statements = Vec::new();

    for child in node.children() {
        // Empty statements (`;`) carry no code.
        if child.kind() == SyntaxKind::EMPTY_STMT {
            continue;
        }
        // Propagate conversion errors rather than silently dropping statements —
        // a dropped statement produces a program that runs but computes the wrong
        // answer, which is worse than a clear compilation error.
        statements.push(convert_statement(child)?);
    }

    Ok(statements)
}

fn convert_statement(node: SyntaxNode) -> BytecodeResult<Statement> {
    match node.kind() {
        SyntaxKind::EXPR_STMT => {
            let children: Vec<_> = node
                .children()
                .filter(|c| is_expression_node(c.kind()))
                .collect();
            Ok(Statement::Expression(convert_expr_nodes(&children)?))
        }
        SyntaxKind::LOCAL_VAR_DECL => convert_var_decl(node),
        SyntaxKind::RETURN_STMT => {
            let exprs: Vec<_> = node
                .children()
                .filter(|c| is_expression_node(c.kind()))
                .collect();
            if exprs.is_empty() {
                Ok(Statement::Return(None))
            } else {
                Ok(Statement::Return(Some(convert_expr_nodes(&exprs)?)))
            }
        }
        SyntaxKind::BLOCK_STMT => Ok(Statement::Block(convert_block(node)?)),
        SyntaxKind::IF_STMT => convert_if(node),
        SyntaxKind::WHILE_STMT => convert_while(node),
        SyntaxKind::FOR_STMT => convert_for(node),
        SyntaxKind::FOREACH_STMT => convert_foreach(node),
        // Java assertions are disabled unless `-ea` is set; model the default
        // (assertions stripped) by compiling `assert ...;` to nothing.
        SyntaxKind::ASSERT_STMT => Ok(Statement::Block(Vec::new())),
        SyntaxKind::THROW_STMT => {
            let exprs: Vec<_> = node
                .children()
                .filter(|c| is_expression_node(c.kind()))
                .collect();
            Ok(Statement::Throw(convert_expr_nodes(&exprs)?))
        }
        SyntaxKind::DO_WHILE_STMT => convert_do_while(node),
        SyntaxKind::BREAK_STMT => Ok(Statement::Break(break_continue_label(&node))),
        SyntaxKind::CONTINUE_STMT => Ok(Statement::Continue(break_continue_label(&node))),
        SyntaxKind::TRY_STMT => convert_try(node),
        SyntaxKind::SWITCH_STMT => convert_switch(node),
        SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION => {
            // `this(args)` or `super(args)`. The leading keyword token selects
            // the target; the ARG_LIST holds the (comma-split) arguments.
            let is_super = node
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .any(|t| t.kind() == SyntaxKind::SUPER_KW);
            let mut args = Vec::new();
            if let Some(arg_list) = node.children().find(|c| c.kind() == SyntaxKind::ARG_LIST) {
                for seg in split_arg_list(&arg_list) {
                    args.push(convert_expr_nodes(&seg)?);
                }
            }
            Ok(Statement::ExplicitConstructorCall { is_super, args })
        }
        _ => Err(parse_error(format!(
            "Unsupported statement: {:?}",
            node.kind()
        ))),
    }
}

/// True if the node kind is a statement we can convert.
fn is_statement_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::EXPR_STMT
            | SyntaxKind::LOCAL_VAR_DECL
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::BLOCK_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOREACH_STMT
            | SyntaxKind::THROW_STMT
            | SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION
            | SyntaxKind::DO_WHILE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::SWITCH_STMT
    )
}

/// Convert a traditional `switch` (colon arms, int/char constant labels).
/// Arrow-form switches and switch *expressions* are not handled here.
fn convert_switch(node: SyntaxNode) -> BytecodeResult<Statement> {
    // The scrutinee is the expression child(ren) before the first CASE_CLAUSE.
    let scrut_nodes: Vec<_> = node
        .children()
        .take_while(|c| c.kind() != SyntaxKind::CASE_CLAUSE)
        .filter(|c| is_expression_node(c.kind()))
        .collect();
    if scrut_nodes.is_empty() {
        return Err(parse_error("switch without scrutinee"));
    }
    let scrutinee = convert_expr_nodes(&scrut_nodes)?;

    let mut cases = Vec::new();
    for cc in node
        .children()
        .filter(|c| c.kind() == SyntaxKind::CASE_CLAUSE)
    {
        let is_default = cc
            .children_with_tokens()
            .any(|e| e.kind() == SyntaxKind::DEFAULT_KW);
        let mut values = Vec::new();
        let mut names = Vec::new();
        let mut body = Vec::new();
        for ch in cc.children() {
            if is_statement_node(ch.kind()) {
                body.push(convert_statement(ch)?);
            } else if !is_default && is_expression_node(ch.kind()) {
                // A traditional case label must be a constant int/char.
                match convert_expression(ch)? {
                    Expression::IntLiteral(v) | Expression::CharLiteral(v) => values.push(v),
                    // An enum constant label. The backend resolves the name against the enum.
                    Expression::NameExpr(name) => names.push(name),
                    _ => {
                        return Err(parse_error(
                            "unsupported switch case label (non-constant-int)",
                        ))
                    }
                }
            }
        }
        cases.push(crate::java_compiler::SwitchCase {
            values,
            names,
            is_default,
            body,
        });
    }

    Ok(Statement::Switch { scrutinee, cases })
}

/// Convert `try { } catch (E e) { } finally { }`. A try-with-resources is desugared as in
/// JLS 14.20.3.1, so its resources are declared and closed. A resource that names an
/// existing variable (Java 9) is refused, rather than dropped.
fn convert_try(node: SyntaxNode) -> BytecodeResult<Statement> {
    // The try body is the first direct BLOCK_STMT; catch/finally bodies are
    // nested inside CATCH_CLAUSE / FINALLY_CLAUSE.
    let body_node = node
        .children()
        .find(|c| c.kind() == SyntaxKind::BLOCK_STMT)
        .ok_or_else(|| parse_error("try without body"))?;
    let resources = resource_declarations(&node)?;
    let mut body = convert_statement(body_node)?;
    // The first resource is the outermost, so it is closed last
    for resource in resources.into_iter().rev() {
        body = close_resource(resource, body);
    }

    let mut catches = Vec::new();
    for cc in node
        .children()
        .filter(|c| c.kind() == SyntaxKind::CATCH_CLAUSE)
    {
        let variable = cc
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|t| t.kind() == SyntaxKind::IDENT)
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "$e".to_string());
        let cbody = cc
            .children()
            .find(|c| c.kind() == SyntaxKind::BLOCK_STMT)
            .map(convert_statement)
            .transpose()?
            .ok_or_else(|| parse_error("catch without body"))?;
        // Multi-catch `catch (A | B e)` yields one handler per type (sharing the
        // bound variable and body), which is semantically equivalent.
        let types: Vec<_> = cc.children().filter(|c| is_type_node(c.kind())).collect();
        if types.is_empty() {
            catches.push(CatchClause {
                exception_type: "java/lang/Throwable".to_string(),
                variable,
                body: cbody,
            });
        } else {
            for t in types {
                let exception_type = match convert_type(t)? {
                    JavaType::Object(n) => n,
                    _ => "java/lang/Throwable".to_string(),
                };
                catches.push(CatchClause {
                    exception_type,
                    variable: variable.clone(),
                    body: cbody.clone(),
                });
            }
        }
    }

    let finally_block = node
        .children()
        .find(|c| c.kind() == SyntaxKind::FINALLY_CLAUSE)
        .and_then(|f| f.children().find(|c| c.kind() == SyntaxKind::BLOCK_STMT))
        .map(convert_statement)
        .transpose()?
        .map(Box::new);

    // A try-with-resources without catch or finally is only its resource block
    if catches.is_empty() && finally_block.is_none() {
        return Ok(body);
    }
    Ok(Statement::Try {
        body: Box::new(body),
        catches,
        finally_block,
    })
}

/// A resource of a try-with-resources: its variable's name and its declaration.
struct Resource {
    name: String,
    declaration: Statement,
}

/// The resources declared by a try-with-resources, in source order.
fn resource_declarations(node: &SyntaxNode) -> BytecodeResult<Vec<Resource>> {
    let Some(spec) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::RESOURCE_SPEC)
    else {
        return Ok(Vec::new());
    };
    let mut resources = Vec::new();
    for resource in spec.children() {
        if resource.kind() != SyntaxKind::LOCAL_VAR_DECL {
            return Err(parse_error(
                "try-with-resources on an existing variable is not supported",
            ));
        }
        let declaration = convert_var_decl(resource)?;
        let Statement::VariableDeclaration { name, .. } = &declaration else {
            return Err(parse_error(
                "a try-with-resources resource must declare one variable",
            ));
        };
        let name = name.clone();
        resources.push(Resource { name, declaration });
    }
    Ok(resources)
}

/// Desugars one resource around `body`, as JLS 14.20.3.1 does. The resource is declared,
/// and the body runs in a try whose finally closes it, unless it is null. When the body
/// threw, a failing close() is recorded as suppressed on that exception, and the body's
/// exception is the one that propagates.
fn close_resource(resource: Resource, body: Statement) -> Statement {
    let name = |n: &str| Expression::NameExpr(n.to_string());
    let call = |receiver: &str, method: &str, args: Vec<Expression>| Expression::MethodCall {
        receiver: Some(Box::new(name(receiver))),
        method: method.to_string(),
        args,
    };
    let not_null = |n: &str| Expression::BinaryExpr {
        left: Box::new(name(n)),
        op: BinaryOp::NotEqual,
        right: Box::new(Expression::NullLiteral),
    };
    let throwable_catch = |variable: &str, body: Statement| CatchClause {
        exception_type: "java/lang/Throwable".to_string(),
        variable: variable.to_string(),
        body,
    };
    let resource_name = resource.name;
    let primary = format!("$primary${resource_name}");
    let thrown = format!("$thrown${resource_name}");
    let suppressed = format!("$suppressed${resource_name}");
    let close = || Statement::Expression(call(&resource_name, "close", Vec::new()));

    // close() after a body that threw: its failure is suppressed on that exception
    let close_after_failure = Statement::Try {
        body: Box::new(close()),
        catches: vec![throwable_catch(
            &suppressed,
            Statement::Expression(call(&primary, "addSuppressed", vec![name(&suppressed)])),
        )],
        finally_block: None,
    };
    let close_in_finally = Statement::If {
        condition: not_null(&resource_name),
        then_stmt: Box::new(Statement::If {
            condition: not_null(&primary),
            then_stmt: Box::new(close_after_failure),
            else_stmt: Some(Box::new(close())),
        }),
        else_stmt: None,
    };
    Statement::Block(vec![
        resource.declaration,
        Statement::VariableDeclaration {
            name: primary.clone(),
            var_type: JavaType::Object("java/lang/Throwable".to_string()),
            generic_arguments: Vec::new(),
            initializer: Some(Expression::NullLiteral),
        },
        Statement::Try {
            body: Box::new(body),
            catches: vec![throwable_catch(
                &thrown,
                Statement::Block(vec![
                    Statement::Expression(Expression::Assignment {
                        target: Box::new(name(&primary)),
                        value: Box::new(name(&thrown)),
                    }),
                    Statement::Throw(name(&thrown)),
                ]),
            )],
            finally_block: Some(Box::new(close_in_finally)),
        },
    ])
}

/// The optional target label of a `break`/`continue` (`break outer;`).
fn break_continue_label(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
}

/// Convert `do <body> while (<cond>);`.
fn convert_do_while(node: SyntaxNode) -> BytecodeResult<Statement> {
    let body = node
        .children()
        .find(|c| is_statement_node(c.kind()))
        .map(convert_statement)
        .transpose()?
        .ok_or_else(|| parse_error("do-while without body"))?;
    let cond_nodes: Vec<_> = node
        .children()
        .filter(|c| is_expression_node(c.kind()))
        .collect();
    if cond_nodes.is_empty() {
        return Err(parse_error("do-while without condition"));
    }
    Ok(Statement::DoWhile {
        condition: convert_expr_nodes(&cond_nodes)?,
        body: Box::new(body),
    })
}

/// Convert an enhanced-for `for (T x : iterable) body` into a `ForEach` node.
/// The lowering (array index loop vs. iterator loop) is deferred to the backend,
/// which can resolve the iterable's type.
fn convert_foreach(node: SyntaxNode) -> BytecodeResult<Statement> {
    let elem_type = node
        .children()
        .find(|c| is_type_node(c.kind()))
        .map(convert_type)
        .transpose()?
        .unwrap_or(JavaType::Int);
    let var_name = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .ok_or_else(|| parse_error("for-each without loop variable"))?;
    let iterable = node
        .children()
        .find(|c| is_expression_node(c.kind()))
        .map(convert_expression)
        .transpose()?
        .ok_or_else(|| parse_error("for-each without iterable"))?;
    let body = node
        .children()
        .find(|c| is_statement_node(c.kind()))
        .map(convert_statement)
        .transpose()?
        .ok_or_else(|| parse_error("for-each without body"))?;

    // The lowering (array index loop vs. collection iterator loop) depends on
    // the iterable's type, which only the backend resolves — emit a node.
    Ok(Statement::ForEach {
        elem_type,
        var_name,
        iterable,
        body: Box::new(body),
    })
}

fn convert_if(node: SyntaxNode) -> BytecodeResult<Statement> {
    // The condition is the leading run of expression siblings (which may form an
    // operator chain); the branches are the statement children that follow.
    let mut cond_nodes: Vec<SyntaxNode> = Vec::new();
    let mut branches = Vec::new();
    for child in node.children() {
        if branches.is_empty() && is_expression_node(child.kind()) {
            cond_nodes.push(child);
        } else if is_statement_node(child.kind()) {
            branches.push(convert_statement(child)?);
        }
    }
    let condition = if cond_nodes.is_empty() {
        None
    } else {
        Some(convert_expr_nodes(&cond_nodes)?)
    };
    let condition = condition.ok_or_else(|| parse_error("if without condition"))?;
    let then_stmt = Box::new(
        branches
            .first()
            .cloned()
            .ok_or_else(|| parse_error("if without then-branch"))?,
    );
    let else_stmt = branches.get(1).cloned().map(Box::new);
    Ok(Statement::If {
        condition,
        then_stmt,
        else_stmt,
    })
}

fn convert_while(node: SyntaxNode) -> BytecodeResult<Statement> {
    let mut cond_nodes: Vec<SyntaxNode> = Vec::new();
    let mut body = None;
    for child in node.children() {
        if body.is_none() && is_expression_node(child.kind()) {
            cond_nodes.push(child);
        } else if body.is_none() && is_statement_node(child.kind()) {
            body = Some(Box::new(convert_statement(child)?));
        }
    }
    let condition = if cond_nodes.is_empty() {
        return Err(parse_error("while without condition"));
    } else {
        convert_expr_nodes(&cond_nodes)?
    };
    Ok(Statement::While {
        condition,
        body: body.ok_or_else(|| parse_error("while without body"))?,
    })
}

/// Convert a C-style `for (init; cond; update) body`.
///
/// The CST lays the clauses out positionally, delimited by `;` tokens and the
/// closing `)`. We walk children tracking how many semicolons we have passed and
/// whether we have crossed the `)` to know which clause each node belongs to.
fn convert_for(node: SyntaxNode) -> BytecodeResult<Statement> {
    let mut init: Option<Box<Statement>> = None;
    let mut condition: Option<Expression> = None;
    let mut update: Option<Expression> = None;
    let mut body: Option<Box<Statement>> = None;

    // Expression nodes collected per clause (assignments span two sibling nodes).
    let mut init_nodes: Vec<SyntaxNode> = Vec::new();
    let mut cond_nodes: Vec<SyntaxNode> = Vec::new();
    let mut update_nodes: Vec<SyntaxNode> = Vec::new();

    let mut semis = 0u8;
    let mut rparen_seen = false;

    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::SEMICOLON => semis += 1,
                SyntaxKind::R_PAREN => rparen_seen = true,
                _ => {}
            },
            NodeOrToken::Node(child) => {
                if rparen_seen {
                    if body.is_none() && is_statement_node(child.kind()) {
                        body = Some(Box::new(convert_statement(child)?));
                    }
                } else {
                    match semis {
                        0 => {
                            if child.kind() == SyntaxKind::LOCAL_VAR_DECL {
                                init = Some(Box::new(convert_var_decl(child)?));
                            } else if is_expression_node(child.kind()) {
                                init_nodes.push(child);
                            }
                        }
                        1 if is_expression_node(child.kind()) => cond_nodes.push(child),
                        _ if is_expression_node(child.kind()) => update_nodes.push(child),
                        _ => {}
                    }
                }
            }
        }
    }

    if init.is_none() && !init_nodes.is_empty() {
        init = Some(Box::new(Statement::Expression(convert_expr_nodes(
            &init_nodes,
        )?)));
    }
    if !cond_nodes.is_empty() {
        condition = Some(convert_expr_nodes(&cond_nodes)?);
    }
    if !update_nodes.is_empty() {
        update = Some(convert_expr_nodes(&update_nodes)?);
    }

    Ok(Statement::For {
        init,
        condition,
        update,
        body: body.ok_or_else(|| parse_error("for without body"))?,
    })
}

/// One declarator accumulated while walking a (possibly multi-variable)
/// `LOCAL_VAR_DECL`: `name`, trailing per-name array dims (`int a[]`), and the
/// initializer expression node(s) that follow its `=`.
struct Declarator {
    name: String,
    trailing_dims: u8,
    init_nodes: Vec<SyntaxNode>,
}

fn convert_var_decl(node: SyntaxNode) -> BytecodeResult<Statement> {
    // The parser emits a *flat* declaration: leading type node(s), then a
    // comma-separated declarator list interleaving IDENT tokens, optional
    // `[]` dims, and initializer expression nodes. A single initializer may
    // itself span several sibling expression nodes (operator-chain
    // continuations), so initializers are grouped per declarator by splitting
    // on the top-level commas rather than assuming one expr per name.
    let mut base_type: Option<JavaType> = None;
    let mut base_type_node: Option<SyntaxNode> = None;
    let mut base_array_dims: u8 = 0;
    let mut declarators: Vec<Declarator> = Vec::new();
    let mut seen_name = false;

    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Node(child) => match child.kind() {
                // Type nodes appear before the first declarator name.
                SyntaxKind::ARRAY_TYPE if !seen_name => {
                    if child.children().any(|c| is_type_node(c.kind())) {
                        base_type_node = Some(child.clone());
                        base_type = Some(convert_type(child)?);
                    } else {
                        base_array_dims += 1;
                    }
                }
                k if is_type_node(k) && !seen_name && base_type.is_none() => {
                    base_type_node = Some(child.clone());
                    base_type = Some(convert_type(child)?);
                }
                // Expression nodes belong to the current declarator's initializer.
                k if is_expression_node(k) => {
                    if let Some(d) = declarators.last_mut() {
                        d.init_nodes.push(child);
                    }
                }
                _ => {}
            },
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::IDENT => {
                    seen_name = true;
                    declarators.push(Declarator {
                        name: token.text().to_string(),
                        trailing_dims: 0,
                        init_nodes: Vec::new(),
                    });
                }
                // `int a[]` — a per-declarator extra dimension.
                SyntaxKind::L_BRACKET => {
                    if let Some(d) = declarators.last_mut() {
                        d.trailing_dims += 1;
                    }
                }
                _ => {}
            },
        }
    }

    if declarators.is_empty() {
        return Err(parse_error("Variable declaration missing name"));
    }

    let functional_target = base_type_node
        .as_ref()
        .and_then(functional_target_from_type);
    let generic_arguments = base_type_node
        .as_ref()
        .map(type_arguments_from_type)
        .unwrap_or_default();

    let stmts = declarators
        .into_iter()
        .map(|d| {
            let var_type = compose_var_type(base_type.clone(), base_array_dims + d.trailing_dims);
            let mut initializer = if d.init_nodes.is_empty() {
                None
            } else {
                Some(convert_expr_nodes(&d.init_nodes)?)
            };
            if let (Some(expr), Some(target)) = (initializer.as_mut(), &functional_target) {
                attach_functional_target(expr, target);
            }
            // For `T[] a = {…}`, give the array initializer the declared element
            // type so element literals coerce/store at the right width.
            if let (
                Some(Expression::ArrayInit { element_type, .. }),
                JavaType::Array {
                    element_type: decl_elem,
                    dimensions,
                },
            ) = (initializer.as_mut(), &var_type)
            {
                *element_type = Some(if *dimensions > 1 {
                    JavaType::Array {
                        element_type: decl_elem.clone(),
                        dimensions: dimensions - 1,
                    }
                } else {
                    (**decl_elem).clone()
                });
            }
            Ok(Statement::VariableDeclaration {
                name: d.name,
                var_type,
                generic_arguments: generic_arguments.clone(),
                initializer,
            })
        })
        .collect::<BytecodeResult<Vec<_>>>()?;

    // A single declarator is the common case; only group when there are several
    // so they share the enclosing scope without an extra block.
    Ok(if stmts.len() == 1 {
        stmts.into_iter().next().unwrap()
    } else {
        Statement::Multi(stmts)
    })
}

fn type_arguments_from_type(node: &SyntaxNode) -> Vec<JavaType> {
    node.descendants()
        .find(|descendant| descendant.kind() == SyntaxKind::TYPE_ARGS)
        .into_iter()
        .flat_map(|arguments| {
            arguments
                .children()
                .filter(|child| is_type_node(child.kind()))
        })
        .filter_map(|child| convert_type(child).ok())
        .collect()
}

fn attach_functional_target(expr: &mut Expression, functional_target: &FunctionalTarget) {
    match expr {
        Expression::Lambda { target, .. } | Expression::MethodReference { target, .. } => {
            *target = Some(functional_target.clone());
        }
        _ => {}
    }
}

/// Recover the target SAM signature from a declared standard functional type.
/// Generic arguments are source-level types; the generated invocation and
/// `doCall` descriptors intentionally remain erased to Object.
fn functional_target_from_type(node: &SyntaxNode) -> Option<FunctionalTarget> {
    let JavaType::Object(interface) = convert_type(node.clone()).ok()? else {
        return None;
    };
    let args_node = node
        .descendants()
        .find(|descendant| descendant.kind() == SyntaxKind::TYPE_ARGS);
    let type_args: Vec<JavaType> = args_node
        .into_iter()
        .flat_map(|args| args.children().filter(|child| is_type_node(child.kind())))
        .filter_map(|child| convert_type(child).ok())
        .collect();
    let arg = |index: usize| type_args.get(index).cloned();
    let (sam_method, parameter_types, return_type) = match interface.as_str() {
        "java/util/function/Function" => ("apply", vec![arg(0)?], arg(1)?),
        "java/util/function/BiFunction" => ("apply", vec![arg(0)?, arg(1)?], arg(2)?),
        "java/util/function/UnaryOperator" => ("apply", vec![arg(0)?], arg(0)?),
        "java/util/function/BinaryOperator" => ("apply", vec![arg(0)?, arg(0)?], arg(0)?),
        "java/util/function/Predicate" => ("test", vec![arg(0)?], JavaType::Boolean),
        "java/util/function/BiPredicate" => ("test", vec![arg(0)?, arg(1)?], JavaType::Boolean),
        "java/util/function/Consumer" => ("accept", vec![arg(0)?], JavaType::Void),
        "java/util/function/BiConsumer" => ("accept", vec![arg(0)?, arg(1)?], JavaType::Void),
        "java/util/function/Supplier" => ("get", Vec::new(), arg(0)?),
        "java/util/Comparator" => ("compare", vec![arg(0)?, arg(0)?], JavaType::Int),
        "java/util/concurrent/Callable" => ("call", Vec::new(), arg(0)?),
        "java/lang/Runnable" => ("run", Vec::new(), JavaType::Void),
        _ => return None,
    };

    Some(FunctionalTarget {
        interface,
        sam_method: sam_method.to_string(),
        parameter_types,
        return_type,
    })
}

/// Combine a base type with trailing `[]` dimensions into the declared type.
fn compose_var_type(base: Option<JavaType>, dims: u8) -> JavaType {
    match (base, dims) {
        (
            Some(JavaType::Array {
                element_type,
                dimensions,
            }),
            extra,
        ) => JavaType::Array {
            element_type,
            dimensions: dimensions + extra,
        },
        (Some(base), 0) => base,
        (Some(base), dims) => JavaType::Array {
            element_type: Box::new(base),
            dimensions: dims,
        },
        (None, dims) if dims > 0 => JavaType::Array {
            element_type: Box::new(JavaType::Int),
            dimensions: dims,
        },
        (None, _) => JavaType::Int,
    }
}

/// Fold a sequence of sibling expression nodes into one expression.
///
/// The parser uses a *flat* representation for several constructs:
/// - Assignment `lhs = rhs`: a target node followed by an `ASSIGNMENT_EXPR`
///   node holding the operator and right-hand side.
/// - Left-associative operator chains `a + b + c`: a complete `BINARY_EXPR`
///   followed by *continuation* `BINARY_EXPR` siblings, each carrying just an
///   operator and a right operand.
///
/// We fold left-to-right, threading the accumulated expression as the left
/// operand of each continuation.
fn convert_expr_nodes(nodes: &[SyntaxNode]) -> BytecodeResult<Expression> {
    let mut acc: Option<Expression> = None;
    for node in nodes.iter().filter(|n| is_expression_node(n.kind())) {
        match node.kind() {
            SyntaxKind::ASSIGNMENT_EXPR => {
                let target = acc
                    .take()
                    .ok_or_else(|| parse_error("assignment without target"))?;
                let rhs_node = node
                    .children()
                    .filter(|c| is_expression_node(c.kind()))
                    .collect::<Vec<_>>();
                let rhs = convert_expr_nodes(&rhs_node)?;
                let value = match node
                    .children_with_tokens()
                    .find_map(|e| compound_assign_op(e.kind()))
                {
                    Some(op) => Expression::BinaryExpr {
                        left: Box::new(target.clone()),
                        op,
                        right: Box::new(rhs),
                    },
                    None => rhs,
                };
                acc = Some(Expression::Assignment {
                    target: Box::new(target),
                    value: Box::new(value),
                });
            }
            SyntaxKind::BINARY_EXPR => {
                let operands: Vec<_> = node
                    .children()
                    .filter(|c| is_expression_node(c.kind()))
                    .collect();
                if let Some(left) = acc.take() {
                    // Continuation: `<acc> <op> <operand>`.
                    let op = node
                        .children_with_tokens()
                        .find_map(|e| token_to_binary_op(e.kind()))
                        .ok_or_else(|| parse_error("binary continuation without operator"))?;
                    let right = convert_expr_nodes(&operands)?;
                    acc = Some(Expression::BinaryExpr {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    });
                } else {
                    acc = Some(convert_binary_expr(node.clone())?);
                }
            }
            _ => {
                acc = Some(convert_expression(node.clone())?);
            }
        }
    }
    acc.ok_or_else(|| parse_error("empty expression"))
}

/// Convert a `LAMBDA_EXPR` (`x -> expr`, `(a, b) -> expr`, `() -> expr`) to
/// `Expression::Lambda`. Parameters are `PARAM` nodes (typed or bare); the body
/// is either the expression after the arrow or the complete statement block.
fn convert_lambda(node: SyntaxNode) -> BytecodeResult<Expression> {
    let empty = HashSet::new();
    let mut params = Vec::new();
    let mut block_body: Option<Vec<Statement>> = None;
    // The body may be a flat operator chain spread across sibling expression
    // nodes (`x -> x + a + b` parses as `[BINARY(x+a), BINARY(+b)]`); collect them
    // all and fold left-associatively with `convert_expr_nodes`, exactly as a
    // normal expression context does.
    let mut body_nodes: Vec<SyntaxNode> = Vec::new();

    for child in node.children() {
        match child.kind() {
            SyntaxKind::PARAM => params.push(convert_param(&child, &empty)?.name),
            SyntaxKind::PARAM_LIST => {
                for p in child.children().filter(|c| c.kind() == SyntaxKind::PARAM) {
                    params.push(convert_param(&p, &empty)?.name);
                }
            }
            SyntaxKind::BLOCK_STMT => {
                block_body = Some(convert_block(child)?);
            }
            k if is_expression_node(k) => body_nodes.push(child),
            _ => {}
        }
    }

    let body = if !body_nodes.is_empty() {
        LambdaBody::Expression(Box::new(convert_expr_nodes(&body_nodes)?))
    } else {
        LambdaBody::Block(block_body.ok_or_else(|| parse_error("lambda without a body"))?)
    };
    Ok(Expression::Lambda {
        params,
        body: Box::new(body),
        target: None,
    })
}

fn convert_expression(node: SyntaxNode) -> BytecodeResult<Expression> {
    match node.kind() {
        SyntaxKind::LITERAL_EXPR => convert_literal(&node),
        SyntaxKind::INT_LITERAL
        | SyntaxKind::LONG_LITERAL
        | SyntaxKind::FLOAT_LITERAL
        | SyntaxKind::DOUBLE_LITERAL
        | SyntaxKind::STRING_LITERAL
        | SyntaxKind::BOOLEAN_LITERAL
        | SyntaxKind::CHAR_LITERAL
        | SyntaxKind::NULL_LITERAL => convert_literal_token(node.kind(), &node.text().to_string()),
        SyntaxKind::NAME_EXPR => {
            let name = node
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT)
                .map(|t| t.text().to_string())
                .unwrap_or_else(|| node.text().to_string());
            Ok(Expression::NameExpr(name))
        }
        SyntaxKind::IDENT => Ok(Expression::NameExpr(node.text().to_string())),
        SyntaxKind::METHOD_REF_EXPR | SyntaxKind::CONSTRUCTOR_REF_EXPR => {
            convert_method_reference(node)
        }
        SyntaxKind::PAREN_EXPR => {
            // The parenthesised expression may be a left-associative chain
            // represented as sibling nodes (e.g. `(a + b + c)`); fold them all
            // rather than taking only the first operand.
            let inner: Vec<_> = node
                .children()
                .filter(|c| is_expression_node(c.kind()))
                .collect();
            if inner.is_empty() {
                return Err(parse_error("empty parenthesized expression"));
            }
            convert_expr_nodes(&inner)
        }
        SyntaxKind::INSTANCEOF_EXPR => {
            let operand: Vec<_> = node
                .children()
                .filter(|c| is_expression_node(c.kind()))
                .collect();
            let type_node = node
                .children()
                .find(|c| is_type_node(c.kind()))
                .ok_or_else(|| parse_error("instanceof without a type"))?;
            let class = match convert_type(type_node)? {
                JavaType::Object(name) => name,
                other => other.descriptor(),
            };
            Ok(Expression::InstanceOf {
                value: Box::new(convert_expr_nodes(&operand)?),
                class,
            })
        }
        SyntaxKind::PATTERN_INSTANCEOF_EXPR => Err(BytecodeError::UnsupportedFeature {
            feature: "instanceof with a pattern binding".to_string(),
        }),
        SyntaxKind::CLASS_LITERAL_EXPR => Err(BytecodeError::UnsupportedFeature {
            feature: "class literals".to_string(),
        }),
        SyntaxKind::SUPER_EXPR => Err(BytecodeError::UnsupportedFeature {
            feature: "super as an expression".to_string(),
        }),
        SyntaxKind::SWITCH_EXPR => Err(BytecodeError::UnsupportedFeature {
            feature: "switch expressions".to_string(),
        }),
        SyntaxKind::BINARY_EXPR => convert_binary_expr(node),
        SyntaxKind::UNARY_EXPR => convert_unary_expr(node),
        SyntaxKind::METHOD_CALL_EXPR => convert_method_call(node),
        SyntaxKind::ASSIGNMENT_EXPR => {
            // Self-contained assignment (target nested inside).
            convert_expr_nodes(&[node])
        }
        SyntaxKind::TERNARY_EXPR => convert_ternary_expr(node),
        SyntaxKind::LAMBDA_EXPR => convert_lambda(node),
        SyntaxKind::ARRAY_INIT_EXPR => convert_array_init(node),
        SyntaxKind::FIELD_ACCESS_EXPR => convert_field_access(node),
        SyntaxKind::THIS_EXPR => Ok(Expression::This),
        SyntaxKind::NEW_EXPR => convert_new_expr(node),
        SyntaxKind::ARRAY_ACCESS_EXPR => convert_array_access(node),
        SyntaxKind::CAST_EXPR => {
            let target_type = node
                .children()
                .find(|c| is_type_node(c.kind()))
                .map(convert_type)
                .transpose()?
                .ok_or_else(|| parse_error("cast without target type"))?;
            let inner = node
                .children()
                .find(|c| is_expression_node(c.kind()))
                .ok_or_else(|| parse_error("cast without operand"))?;
            Ok(Expression::Cast {
                target_type,
                expr: Box::new(convert_expression(inner)?),
            })
        }
        _ => Err(parse_error(format!(
            "Unsupported expression: {:?}",
            node.kind()
        ))),
    }
}

fn convert_method_reference(node: SyntaxNode) -> BytecodeResult<Expression> {
    let is_constructor = node.kind() == SyntaxKind::CONSTRUCTOR_REF_EXPR;
    let qualifier = node
        .children()
        .find(|child| is_expression_node(child.kind()))
        .ok_or_else(|| parse_error("method reference without qualifier"))?;
    let method = if is_constructor {
        "<init>".to_string()
    } else {
        node.children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .filter(|token| token.kind() == SyntaxKind::IDENT)
            .last()
            .map(|token| token.text().to_string())
            .ok_or_else(|| parse_error("method reference without method name"))?
    };
    Ok(Expression::MethodReference {
        qualifier: Box::new(convert_expression(qualifier)?),
        method,
        is_constructor,
        target: None,
    })
}

/// Convert a `new` expression: object creation `new C(args)` or array creation
/// `new T[len]`. Array creation is distinguished by the presence of `[`.
fn convert_new_expr(node: SyntaxNode) -> BytecodeResult<Expression> {
    let is_array = node
        .children_with_tokens()
        .any(|e| e.kind() == SyntaxKind::L_BRACKET);

    let type_node = node
        .children()
        .find(|c| is_type_node(c.kind()))
        .ok_or_else(|| parse_error("new expression without type"))?;

    if is_array {
        let element_type = convert_type(type_node)?;
        // One length expression per specified dimension: `new int[a][b]` has two.
        let lengths: Vec<Expression> = node
            .children()
            .filter(|c| is_expression_node(c.kind()))
            .map(convert_expression)
            .collect::<BytecodeResult<_>>()?;
        return Ok(match lengths.len() {
            0 => Expression::NewArray {
                element_type,
                length: Box::new(Expression::IntLiteral(0)),
            },
            1 => Expression::NewArray {
                element_type,
                length: Box::new(lengths.into_iter().next().unwrap()),
            },
            _ => Expression::NewMultiArray {
                element_type,
                lengths,
            },
        });
    }

    // Object creation.
    let class = match convert_type(type_node)? {
        JavaType::Object(name) => name,
        other => other.descriptor(),
    };
    let mut args = Vec::new();
    if let Some(arg_list) = node.children().find(|c| c.kind() == SyntaxKind::ARG_LIST) {
        for seg in split_arg_list(&arg_list) {
            args.push(convert_expr_nodes(&seg)?);
        }
    }
    Ok(Expression::NewExpr { class, args })
}

fn convert_array_access(node: SyntaxNode) -> BytecodeResult<Expression> {
    // The `[` separates the array expression from the (possibly chained) index.
    let mut array_nodes: Vec<SyntaxNode> = Vec::new();
    let mut index_nodes: Vec<SyntaxNode> = Vec::new();
    let mut after_bracket = false;
    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::L_BRACKET => after_bracket = true,
            NodeOrToken::Node(n) if is_expression_node(n.kind()) => {
                if after_bracket {
                    index_nodes.push(n);
                } else {
                    array_nodes.push(n);
                }
            }
            _ => {}
        }
    }
    if array_nodes.is_empty() {
        return Err(parse_error("array access without array"));
    }
    if index_nodes.is_empty() {
        return Err(parse_error("array access without index"));
    }
    Ok(Expression::ArrayAccess {
        array: Box::new(convert_expr_nodes(&array_nodes)?),
        index: Box::new(convert_expr_nodes(&index_nodes)?),
    })
}

/// Split an ARG_LIST into per-argument node groups, breaking on commas.
/// Each group is the sibling sequence forming one argument expression.
fn split_arg_list(arg_list: &SyntaxNode) -> Vec<Vec<SyntaxNode>> {
    let mut segments: Vec<Vec<SyntaxNode>> = Vec::new();
    let mut current: Vec<SyntaxNode> = Vec::new();
    for elem in arg_list.children_with_tokens() {
        match elem {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMA && !current.is_empty() => {
                segments.push(std::mem::take(&mut current));
            }
            NodeOrToken::Node(n) if is_expression_node(n.kind()) => current.push(n),
            _ => {}
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

/// Convert a `LITERAL_EXPR` node by inspecting its single literal token.
fn convert_literal(node: &SyntaxNode) -> BytecodeResult<Expression> {
    let tok = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| {
            matches!(
                t.kind(),
                SyntaxKind::INT_LITERAL
                    | SyntaxKind::LONG_LITERAL
                    | SyntaxKind::FLOAT_LITERAL
                    | SyntaxKind::DOUBLE_LITERAL
                    | SyntaxKind::STRING_LITERAL
                    | SyntaxKind::BOOLEAN_LITERAL
                    | SyntaxKind::CHAR_LITERAL
                    | SyntaxKind::NULL_LITERAL
            )
        })
        .ok_or_else(|| parse_error("literal expression without a literal token"))?;
    convert_literal_token(tok.kind(), tok.text())
}

fn convert_literal_token(kind: SyntaxKind, text: &str) -> BytecodeResult<Expression> {
    match kind {
        SyntaxKind::INT_LITERAL => {
            let t = text.replace('_', "");
            // A literal that does not fit i32 is a long (Java requires `L`, but be lenient).
            if let Ok(v) = parse_int_literal(&t) {
                Ok(Expression::IntLiteral(v))
            } else {
                let cleaned = t.trim_end_matches(['l', 'L']);
                parse_long_literal(cleaned).map(Expression::LongLiteral)
            }
        }
        SyntaxKind::LONG_LITERAL => {
            let t = text.replace('_', "");
            let cleaned = t.trim_end_matches(['l', 'L']);
            parse_long_literal(cleaned).map(Expression::LongLiteral)
        }
        SyntaxKind::FLOAT_LITERAL => {
            let t = text.trim_end_matches(['f', 'F']).replace('_', "");
            t.parse::<f32>()
                .map(Expression::FloatLiteral)
                .map_err(|e| parse_error(format!("Invalid float: {e}")))
        }
        SyntaxKind::DOUBLE_LITERAL => {
            let t = text.trim_end_matches(['d', 'D']).replace('_', "");
            t.parse::<f64>()
                .map(Expression::DoubleLiteral)
                .map_err(|e| parse_error(format!("Invalid double: {e}")))
        }
        SyntaxKind::STRING_LITERAL => {
            let content = if text.len() >= 2 {
                text[1..text.len() - 1].to_string()
            } else {
                text.to_string()
            };
            Ok(Expression::StringLiteral(unescape(&content)))
        }
        SyntaxKind::BOOLEAN_LITERAL => Ok(Expression::BooleanLiteral(text == "true")),
        SyntaxKind::CHAR_LITERAL => {
            let inner = text.trim_matches('\'');
            let c = unescape(inner).chars().next().unwrap_or('\0');
            Ok(Expression::CharLiteral(c as i32))
        }
        SyntaxKind::NULL_LITERAL => Ok(Expression::NullLiteral),
        _ => Err(parse_error(format!("Unsupported literal: {kind:?}"))),
    }
}

fn parse_int_literal(s: &str) -> Result<i32, std::num::ParseIntError> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i32::from_str_radix(hex, 16)
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        i32::from_str_radix(bin, 2)
    } else {
        s.parse::<i32>()
    }
}

fn parse_long_literal(s: &str) -> BytecodeResult<i64> {
    let r = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16)
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        i64::from_str_radix(bin, 2)
    } else {
        s.parse::<i64>()
    };
    r.map_err(|e| parse_error(format!("Invalid long: {e}")))
}

/// Minimal escape-sequence handling for string/char literals.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some('0') => out.push('\0'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Convert `cond ? then : els`. The condition and each branch may themselves
/// be flat operator-chains (sibling nodes), so expression children are
/// partitioned by the `?` and `:` delimiter tokens and each group folded.
fn convert_ternary_expr(node: SyntaxNode) -> BytecodeResult<Expression> {
    let (mut cond, mut then_nodes, mut else_nodes) = (Vec::new(), Vec::new(), Vec::new());
    let mut seen_question = false;
    let mut seen_colon = false;
    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::QUESTION => seen_question = true,
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::COLON && seen_question => {
                seen_colon = true;
            }
            NodeOrToken::Node(n) if is_expression_node(n.kind()) => {
                if !seen_question {
                    cond.push(n);
                } else if !seen_colon {
                    then_nodes.push(n);
                } else {
                    else_nodes.push(n);
                }
            }
            _ => {}
        }
    }
    if cond.is_empty() || then_nodes.is_empty() || else_nodes.is_empty() {
        return Err(parse_error("malformed ternary expression"));
    }
    Ok(Expression::Ternary {
        condition: Box::new(convert_expr_nodes(&cond)?),
        then_expr: Box::new(convert_expr_nodes(&then_nodes)?),
        else_expr: Box::new(convert_expr_nodes(&else_nodes)?),
    })
}

/// Convert an array initializer `{ e0, e1, ... }`. Elements are comma-separated
/// (each possibly a flat operator-chain), so split on top-level commas and fold
/// each group. The element type is left `None` here and resolved later from the
/// declared array type (or inferred by the backend).
fn convert_array_init(node: SyntaxNode) -> BytecodeResult<Expression> {
    let mut elements = Vec::new();
    let mut group: Vec<SyntaxNode> = Vec::new();
    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMA && !group.is_empty() => {
                elements.push(convert_expr_nodes(&group)?);
                group.clear();
            }
            NodeOrToken::Node(n)
                if is_expression_node(n.kind()) || n.kind() == SyntaxKind::ARRAY_INIT_EXPR =>
            {
                group.push(n);
            }
            _ => {}
        }
    }
    if !group.is_empty() {
        elements.push(convert_expr_nodes(&group)?);
    }
    Ok(Expression::ArrayInit {
        element_type: None,
        elements,
    })
}

fn convert_field_access(node: SyntaxNode) -> BytecodeResult<Expression> {
    // Receiver is the first expression child; the field is the trailing IDENT.
    let receiver = node
        .children()
        .find(|c| is_expression_node(c.kind()))
        .map(|c| convert_expression(c).map(Box::new))
        .transpose()?;
    let field = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .last()
        .map(|t| t.text().to_string())
        .ok_or_else(|| parse_error("field access without field name"))?;
    // Record the textual receiver name (used to recognise `System.out` etc.).
    let class = receiver
        .as_ref()
        .map(|r| match &**r {
            Expression::NameExpr(n) => n.clone(),
            _ => String::new(),
        })
        .unwrap_or_default();
    Ok(Expression::FieldAccess {
        receiver,
        field,
        class,
    })
}

fn convert_binary_expr(node: SyntaxNode) -> BytecodeResult<Expression> {
    // A `BINARY_EXPR` has exactly one direct operator token; the operands on
    // each side may be flat operator-chains (left-associative continuations),
    // e.g. `1 + 2 + 3 == t` parses the `==` node as
    // `[BINARY(1+2), BINARY(+3), ==, t]`. Split at the operator token into a
    // left group and a right group and fold each with the chain-aware
    // `convert_expr_nodes` (rather than assuming a single node per side).
    let mut left_group: Vec<SyntaxNode> = Vec::new();
    let mut right_group: Vec<SyntaxNode> = Vec::new();
    let mut op = None;

    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Node(child) if is_expression_node(child.kind()) => {
                if op.is_none() {
                    left_group.push(child);
                } else {
                    right_group.push(child);
                }
            }
            NodeOrToken::Token(token) if op.is_none() => {
                if let Some(parsed) = token_to_binary_op(token.kind()) {
                    op = Some(parsed);
                }
            }
            _ => {}
        }
    }

    let op = op.ok_or_else(|| parse_error("binary expression without operator"))?;
    if left_group.is_empty() || right_group.is_empty() {
        return Err(parse_error("Missing right operand"));
    }
    let left = Some(convert_expr_nodes(&left_group)?);
    let right = Some(convert_expr_nodes(&right_group)?);

    Ok(Expression::BinaryExpr {
        left: Box::new(left.ok_or_else(|| parse_error("Missing left operand"))?),
        op,
        right: Box::new(right.ok_or_else(|| parse_error("Missing right operand"))?),
    })
}

fn convert_unary_expr(node: SyntaxNode) -> BytecodeResult<Expression> {
    let operand_node = node
        .children()
        .find(|c| is_expression_node(c.kind()))
        .ok_or_else(|| parse_error("unary expression without operand"))?;
    let operand = convert_expression(operand_node)?;

    // Increment / decrement (`x++`, `++x`, `x--`, `--x`) desugar to an
    // assignment. In value position this yields the *new* value; callers that
    // use them only for effect (loop updates, expression statements) are
    // unaffected by the pre/post distinction.
    let incdec = node.children_with_tokens().find_map(|e| match e.kind() {
        SyntaxKind::PLUS_PLUS => Some(BinaryOp::Add),
        SyntaxKind::MINUS_MINUS => Some(BinaryOp::Subtract),
        _ => None,
    });
    if let Some(op) = incdec {
        return Ok(Expression::Assignment {
            target: Box::new(operand.clone()),
            value: Box::new(Expression::BinaryExpr {
                left: Box::new(operand),
                op,
                right: Box::new(Expression::IntLiteral(1)),
            }),
        });
    }

    let op = node
        .children_with_tokens()
        .find_map(|e| token_to_unary_op(e.kind()))
        .ok_or_else(|| parse_error("Missing unary operator"))?;
    Ok(Expression::UnaryExpr {
        op,
        operand: Box::new(operand),
    })
}

/// Map a compound-assignment operator token to its underlying binary op.
fn compound_assign_op(kind: SyntaxKind) -> Option<BinaryOp> {
    match kind {
        SyntaxKind::PLUS_EQ => Some(BinaryOp::Add),
        SyntaxKind::MINUS_EQ => Some(BinaryOp::Subtract),
        SyntaxKind::STAR_EQ => Some(BinaryOp::Multiply),
        SyntaxKind::SLASH_EQ => Some(BinaryOp::Divide),
        SyntaxKind::PERCENT_EQ => Some(BinaryOp::Modulo),
        SyntaxKind::AMP_EQ => Some(BinaryOp::BitwiseAnd),
        SyntaxKind::PIPE_EQ => Some(BinaryOp::BitwiseOr),
        SyntaxKind::CARET_EQ => Some(BinaryOp::BitwiseXor),
        SyntaxKind::SHL_EQ => Some(BinaryOp::LeftShift),
        SyntaxKind::SHR_EQ => Some(BinaryOp::RightShift),
        SyntaxKind::USHR_EQ => Some(BinaryOp::UnsignedRightShift),
        _ => None,
    }
}

fn convert_method_call(node: SyntaxNode) -> BytecodeResult<Expression> {
    // Arguments live inside the ARG_LIST child; split on commas so each argument
    // (which may itself be an operator chain) is folded independently.
    let mut args = Vec::new();
    if let Some(arg_list) = node.children().find(|c| c.kind() == SyntaxKind::ARG_LIST) {
        for seg in split_arg_list(&arg_list) {
            args.push(convert_expr_nodes(&seg)?);
        }
    }

    // The method name is the direct IDENT token of the call node, present for a
    // qualified call `receiver.method(..)`. For an unqualified call `m(..)` the
    // name lives inside the single NAME_EXPR child instead.
    let direct_ident = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string());

    // The expression child before the ARG_LIST is the receiver (qualified call)
    // or the callee name node (unqualified call).
    let expr_child = node.children().find(|c| {
        matches!(
            c.kind(),
            SyntaxKind::NAME_EXPR | SyntaxKind::FIELD_ACCESS_EXPR
        ) || (is_expression_node(c.kind()) && c.kind() != SyntaxKind::ARG_LIST)
    });

    let (receiver, method) = match direct_ident {
        Some(name) => {
            // Qualified call: the expression child is the receiver.
            let receiver = expr_child
                .map(|c| convert_expression(c).map(Box::new))
                .transpose()?;
            (receiver, name)
        }
        None => match expr_child {
            Some(c) if c.kind() == SyntaxKind::FIELD_ACCESS_EXPR => {
                if let Expression::FieldAccess {
                    receiver, field, ..
                } = convert_field_access(c)?
                {
                    (receiver, field)
                } else {
                    (None, String::from("unknown"))
                }
            }
            Some(c) => {
                let name = c
                    .children_with_tokens()
                    .filter_map(NodeOrToken::into_token)
                    .find(|t| t.kind() == SyntaxKind::IDENT)
                    .map(|t| t.text().to_string())
                    .unwrap_or_else(|| c.text().to_string());
                (None, name)
            }
            None => (None, String::from("unknown")),
        },
    };

    Ok(Expression::MethodCall {
        receiver,
        method,
        args,
    })
}

fn convert_type(node: SyntaxNode) -> BytecodeResult<JavaType> {
    // `var` (Java 10+): a sentinel the backend resolves from the initializer.
    // `var` is a reserved type name, so Object("var") can never collide with a
    // real class.
    if node.kind() == SyntaxKind::VAR_TYPE {
        return Ok(JavaType::Object("var".to_string()));
    }

    // Array type: `Elem[]` — recurse into the element type and wrap.
    if node.kind() == SyntaxKind::ARRAY_TYPE {
        let dimensions = node
            .children_with_tokens()
            .filter(|e| e.kind() == SyntaxKind::L_BRACKET)
            .count()
            .max(1) as u8;
        let element = node
            .children()
            .find(|c| is_type_node(c.kind()))
            .map(convert_type)
            .transpose()?
            .unwrap_or(JavaType::Object("java/lang/Object".to_string()));
        return Ok(JavaType::Array {
            element_type: Box::new(element),
            dimensions,
        });
    }

    // Class / qualified reference types: erase to the raw type name. Generic
    // type arguments (`Foo<Bar>`) are dropped — we collect only the IDENTs that
    // are *not* inside a `TYPE_ARGS` subtree, so `List<String>` erases to `List`
    // (not the bogus `List/String` a flat descendant-walk produced).
    if matches!(
        node.kind(),
        SyntaxKind::CLASS_TYPE | SyntaxKind::QUALIFIED_TYPE
    ) {
        let mut segments = Vec::new();
        collect_raw_type_idents(&node, &mut segments);
        let name = segments.join("/");
        return Ok(JavaType::Object(qualify_class_name(&name)));
    }

    // Check node kind first
    let java_type = match node.kind() {
        SyntaxKind::INT_KW => Some(JavaType::Int),
        SyntaxKind::LONG_KW => Some(JavaType::Long),
        SyntaxKind::FLOAT_KW => Some(JavaType::Float),
        SyntaxKind::DOUBLE_KW => Some(JavaType::Double),
        SyntaxKind::BOOLEAN_KW => Some(JavaType::Boolean),
        SyntaxKind::CHAR_KW => Some(JavaType::Char),
        SyntaxKind::BYTE_KW => Some(JavaType::Byte),
        SyntaxKind::SHORT_KW => Some(JavaType::Short),
        SyntaxKind::VOID_KW => Some(JavaType::Void),
        _ => None,
    };

    if let Some(ty) = java_type {
        return Ok(ty);
    }

    // Check tokens for type keywords or identifiers
    for elem in node.children_with_tokens() {
        if let NodeOrToken::Token(token) = elem {
            match token.kind() {
                SyntaxKind::INT_KW => return Ok(JavaType::Int),
                SyntaxKind::LONG_KW => return Ok(JavaType::Long),
                SyntaxKind::FLOAT_KW => return Ok(JavaType::Float),
                SyntaxKind::DOUBLE_KW => return Ok(JavaType::Double),
                SyntaxKind::BOOLEAN_KW => return Ok(JavaType::Boolean),
                SyntaxKind::CHAR_KW => return Ok(JavaType::Char),
                SyntaxKind::BYTE_KW => return Ok(JavaType::Byte),
                SyntaxKind::SHORT_KW => return Ok(JavaType::Short),
                SyntaxKind::VOID_KW => return Ok(JavaType::Void),
                SyntaxKind::IDENT => return Ok(JavaType::Object(qualify_class_name(token.text()))),
                _ => {}
            }
        }
    }

    // Check children recursively
    for child in node.children() {
        if is_type_node(child.kind()) {
            return convert_type(child);
        }
    }

    Err(parse_error(format!("Unknown type: {:?}", node.kind())))
}

/// Convert a parsed source file into a `ClassDecl` (the first top-level class).
///
/// Unlike [`convert_source_file`], which flattens every method body into a single
/// statement list, this preserves the full class structure (methods, params,
/// return types) needed to emit a real multi-method class file.
pub fn convert_class_file(node: SyntaxNode) -> BytecodeResult<ClassDecl> {
    install_unit_ctx(&node);
    for child in node.children() {
        match child.kind() {
            SyntaxKind::CLASS_DECL => return convert_class_decl(child),
            SyntaxKind::RECORD_DECL => return convert_record_decl(child),
            _ => {}
        }
    }
    Err(parse_error("No class declaration found in source file"))
}

/// Convert *every* class, record, enum, interface and annotation declaration in a
/// compilation unit — top-level and nested — so a file's helper and nested classes are
/// available for cross-class calls (`new ListNode()` for a nested `ListNode`). Each is
/// emitted as a standalone class under its simple name, and an enum constant with a body
/// adds a subclass of its enum. A declaration that fails to convert fails the unit.
/// (Nested classes are compiled as if static — an inner class's capture of the enclosing
/// instance is not modelled.)
pub fn convert_compilation_unit(node: SyntaxNode) -> BytecodeResult<Vec<ClassDecl>> {
    install_unit_ctx(&node);
    let mut classes = Vec::new();
    // The subclasses of enum constants with bodies. They are not nested declarations of the unit.
    let mut enum_subclasses = Vec::new();
    for decl_node in node.descendants() {
        if decl_node.kind() == SyntaxKind::ENUM_DECL {
            let mut decls = convert_enum_decl(decl_node.clone())?;
            let mut enum_decl = decls.remove(0);
            enum_decl.enclosing_static_fields = enclosing_static_fields(&decl_node, &classes);
            classes.push((decl_node, enum_decl));
            enum_subclasses.extend(decls);
            continue;
        }
        let decl = match decl_node.kind() {
            SyntaxKind::CLASS_DECL => convert_class_decl(decl_node.clone()),
            SyntaxKind::RECORD_DECL => convert_record_decl(decl_node.clone()),
            SyntaxKind::INTERFACE_DECL => convert_interface_decl(decl_node.clone()),
            SyntaxKind::ANNOTATION_DECL => convert_annotation_decl(decl_node.clone()),
            _ => continue,
        };
        // A class that cannot be converted fails the unit. Skipping it would leave the
        // program to fail later, at run time, with a missing class.
        let mut decl = decl?;
        decl.enclosing_static_fields = enclosing_static_fields(&decl_node, &classes);
        classes.push((decl_node, decl));
    }
    Ok(classes
        .into_iter()
        .map(|(_, decl)| decl)
        .chain(enum_subclasses)
        .collect())
}

/// The static fields of the classes that enclose a declaration, with each owner's internal name.
/// Pre-order traversal converts an enclosing class before the classes nested in it.
fn enclosing_static_fields(
    decl_node: &SyntaxNode,
    converted: &[(SyntaxNode, ClassDecl)],
) -> Vec<(String, FieldDecl)> {
    let mut fields = Vec::new();
    for ancestor in decl_node.ancestors().skip(1) {
        if let Some((_, owner)) = converted.iter().find(|(node, _)| *node == ancestor) {
            fields.extend(
                owner
                    .fields
                    .iter()
                    .filter(|field| field.is_static)
                    .map(|field| (owner.name.clone(), field.clone())),
            );
        }
    }
    fields
}

/// The hidden leading parameters of an enum constructor: the constant's name and ordinal, which
/// the constructor passes to `java.lang.Enum`.
const ENUM_NAME_PARAM: &str = "$name";
const ENUM_ORDINAL_PARAM: &str = "$ordinal";

/// Convert an enum declaration. The enum is a class extending `java.lang.Enum`. Each constant is a
/// static field, created in `<clinit>` with its name and ordinal. Each constructor takes those as
/// hidden leading parameters and passes them to `Enum`. `values()` and `valueOf(String)` are
/// generated. Constants with bodies and enums that implement interfaces are refused, not compiled.
fn convert_enum_decl(node: SyntaxNode) -> BytecodeResult<Vec<ClassDecl>> {
    let name = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .ok_or_else(|| parse_error("Enum declaration missing name"))?
        .replace('.', "/");
    let enum_type = JavaType::Object(name.clone());
    let string_type = JavaType::Object("java/lang/String".to_string());

    // Each constant: its name, its arguments, and its class body if it has one.
    let mut constants: Vec<(String, Vec<Expression>, Option<SyntaxNode>)> = Vec::new();
    for constant in node
        .children()
        .filter(|child| child.kind() == SyntaxKind::ENUM_CONSTANT)
    {
        let constant_name = constant
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|t| t.kind() == SyntaxKind::IDENT)
            .map(|t| t.text().to_string())
            .ok_or_else(|| parse_error("Enum constant missing name"))?;
        if constant.children().any(|child| {
            child.kind() != SyntaxKind::ARG_LIST && !is_class_member_kind(child.kind())
        }) {
            return Err(BytecodeError::UnsupportedFeature {
                feature: format!("enum constant {constant_name} with an unsupported member"),
            });
        }
        let has_body = constant
            .children()
            .any(|child| is_class_member_kind(child.kind()));
        let mut args = Vec::new();
        if let Some(arg_list) = constant
            .children()
            .find(|child| child.kind() == SyntaxKind::ARG_LIST)
        {
            for segment in split_arg_list(&arg_list) {
                args.push(convert_expr_nodes(&segment)?);
            }
        }
        constants.push((constant_name, args, has_body.then(|| constant.clone())));
    }

    let Members {
        mut methods,
        mut fields,
        static_init,
        instance_init,
    } = convert_members(&node, &HashSet::new(), false)?;

    // The parameter types of each constructor as declared, for the subclass of a constant with a body.
    let mut declared_ctor_params: Vec<Vec<JavaType>> = methods
        .iter()
        .filter(|m| m.name == "<init>")
        .map(|m| m.params.iter().map(|p| p.ty.clone()).collect())
        .collect();
    if declared_ctor_params.is_empty() {
        declared_ctor_params.push(Vec::new()); // the default constructor
    }

    // Every constructor takes the hidden name and ordinal, and gives them to `Enum`. A constructor
    // that delegates with `this(...)` passes them on to the constructor it delegates to.
    for method in methods.iter_mut().filter(|m| m.name == "<init>") {
        method.params.splice(0..0, hidden_enum_params());
        let delegates = match method.body.first() {
            Some(Statement::ExplicitConstructorCall { is_super, .. }) => Some(*is_super),
            _ => None,
        };
        match delegates {
            Some(true) => {
                return Err(BytecodeError::UnsupportedFeature {
                    feature: format!("enum {name} whose constructor calls super(...)"),
                })
            }
            Some(false) => {
                if let Some(Statement::ExplicitConstructorCall { args, .. }) =
                    method.body.first_mut()
                {
                    args.splice(0..0, hidden_enum_args());
                }
            }
            None => method.body.insert(
                0,
                Statement::ExplicitConstructorCall {
                    is_super: true,
                    args: hidden_enum_args(),
                },
            ),
        }
    }
    if !methods.iter().any(|m| m.name == "<init>") {
        methods.push(MethodDecl {
            is_abstract: false,
            name: "<init>".to_string(),
            params: hidden_enum_params(),
            return_type: JavaType::Void,
            is_static: false,
            body: vec![Statement::ExplicitConstructorCall {
                is_super: true,
                args: hidden_enum_args(),
            }],
        });
    }

    // Each constant with a body is an instance of its own subclass of this enum, which has the body's
    // members. The subclass's constructor takes the constant's arguments, typed as the enum's
    // constructor of that arity, and passes them to it. A body can name the enum's static fields and
    // the constants by their simple names, so they are the subclass's enclosing statics.
    let enum_statics: Vec<(String, FieldDecl)> = constants
        .iter()
        .map(|(constant, _, _)| FieldDecl {
            name: constant.clone(),
            ty: enum_type.clone(),
            is_static: true,
        })
        .chain(fields.iter().filter(|f| f.is_static).cloned())
        .map(|field| (name.clone(), field))
        .collect();
    let mut anonymous = Vec::new();
    let mut anon_names: Vec<Option<String>> = Vec::new();
    for (constant, args, body) in &constants {
        let Some(body) = body else {
            anon_names.push(None);
            continue;
        };
        let anon_name = format!("{name}${}", anonymous.len() + 1);
        let param_types =
            constructor_params_of_arity(&declared_ctor_params, args.len(), &name, constant)?;
        let Members {
            methods: body_methods,
            fields: body_fields,
            static_init: body_static,
            instance_init: body_instance,
        } = convert_members(body, &HashSet::new(), false)?;
        if !body_static.is_empty() {
            return Err(BytecodeError::UnsupportedFeature {
                feature: format!("static initializer in the body of enum constant {constant}"),
            });
        }
        let mut ctor_params = hidden_enum_params();
        let mut ctor_args = hidden_enum_args();
        for (i, ty) in param_types.into_iter().enumerate() {
            ctor_params.push(Param {
                name: format!("$arg{i}"),
                ty,
            });
            ctor_args.push(Expression::NameExpr(format!("$arg{i}")));
        }
        let mut subclass_methods = vec![MethodDecl {
            is_abstract: false,
            name: "<init>".to_string(),
            params: ctor_params,
            return_type: JavaType::Void,
            is_static: false,
            body: vec![Statement::ExplicitConstructorCall {
                is_super: true,
                args: ctor_args,
            }],
        }];
        subclass_methods.extend(body_methods);
        anonymous.push(ClassDecl {
            name: anon_name.clone(),
            super_name: name.clone(),
            is_interface: false,
            is_annotation: false,
            declared_abstract: false,
            interfaces: Vec::new(),
            supertype_arguments: Vec::new(),
            fields: body_fields,
            methods: subclass_methods,
            enclosing_static_fields: enum_statics.clone(),
            static_init: Vec::new(),
            instance_init: body_instance,
        });
        anon_names.push(Some(anon_name));
    }

    // The constants are the first static fields, and the first static initializers.
    let mut constant_fields = Vec::new();
    let mut constant_inits = Vec::new();
    for (ordinal, ((constant, args, _), anon)) in constants.iter().zip(&anon_names).enumerate() {
        let ordinal = i32::try_from(ordinal)
            .map_err(|_| parse_error("enum with more constants than an int can index"))?;
        constant_fields.push(FieldDecl {
            name: constant.clone(),
            ty: enum_type.clone(),
            is_static: true,
        });
        let mut new_args = vec![
            Expression::StringLiteral(constant.clone()),
            Expression::IntLiteral(ordinal),
        ];
        new_args.extend(args.iter().cloned());
        constant_inits.push(Statement::Expression(Expression::Assignment {
            target: Box::new(Expression::NameExpr(constant.clone())),
            value: Box::new(Expression::NewExpr {
                class: anon.clone().unwrap_or_else(|| name.clone()),
                args: new_args,
            }),
        }));
    }
    fields.splice(0..0, constant_fields);
    let static_init = constant_inits.into_iter().chain(static_init).collect();

    // `values()` returns a new array of the constants, as `Enum.values()` does.
    let constant_names: Vec<Expression> = constants
        .iter()
        .map(|(constant, _, _)| Expression::NameExpr(constant.clone()))
        .collect();
    methods.push(MethodDecl {
        is_abstract: false,
        name: "values".to_string(),
        params: Vec::new(),
        return_type: JavaType::Array {
            element_type: Box::new(enum_type.clone()),
            dimensions: 1,
        },
        is_static: true,
        body: vec![Statement::Return(Some(Expression::ArrayInit {
            element_type: Some(enum_type.clone()),
            elements: constant_names,
        }))],
    });

    // `valueOf(String)` returns the constant with that name, or throws as `Enum.valueOf` does.
    let mut value_of = Vec::new();
    for (constant, _, _) in &constants {
        value_of.push(Statement::If {
            condition: Expression::MethodCall {
                receiver: Some(Box::new(Expression::NameExpr(ENUM_NAME_PARAM.to_string()))),
                method: "equals".to_string(),
                args: vec![Expression::StringLiteral(constant.clone())],
            },
            then_stmt: Box::new(Statement::Return(Some(Expression::NameExpr(
                constant.clone(),
            )))),
            else_stmt: None,
        });
    }
    value_of.push(Statement::Throw(Expression::NewExpr {
        class: "java/lang/IllegalArgumentException".to_string(),
        args: vec![Expression::BinaryExpr {
            left: Box::new(Expression::StringLiteral(format!(
                "No enum constant {}.",
                name.replace('/', ".")
            ))),
            op: BinaryOp::Add,
            right: Box::new(Expression::NameExpr(ENUM_NAME_PARAM.to_string())),
        }],
    }));
    methods.push(MethodDecl {
        is_abstract: false,
        name: "valueOf".to_string(),
        params: vec![Param {
            name: ENUM_NAME_PARAM.to_string(),
            ty: string_type,
        }],
        return_type: enum_type,
        is_static: true,
        body: value_of,
    });

    let enum_decl = ClassDecl {
        name,
        super_name: "java/lang/Enum".to_string(),
        is_interface: false,
        is_annotation: false,
        declared_abstract: false,
        interfaces: header_types_after(&node, SyntaxKind::IMPLEMENTS_KW)?,
        supertype_arguments: header_type_arguments(&node),
        fields,
        methods,
        enclosing_static_fields: Vec::new(),
        static_init,
        instance_init,
    };
    Ok(std::iter::once(enum_decl).chain(anonymous).collect())
}

/// The parameter types of the enum constructor that a constant's arguments select. Overloads of one
/// arity cannot be told apart by the argument count, so they are refused.
fn constructor_params_of_arity(
    ctors: &[Vec<JavaType>],
    arity: usize,
    enum_name: &str,
    constant: &str,
) -> BytecodeResult<Vec<JavaType>> {
    let matching: Vec<&Vec<JavaType>> = ctors
        .iter()
        .filter(|params| params.len() == arity)
        .collect();
    match matching.as_slice() {
        [only] => Ok((*only).clone()),
        [] => Err(BytecodeError::UnsupportedFeature {
            feature: format!(
                "enum constant {constant} with a body: no constructor of {enum_name} takes {arity} argument(s)"
            ),
        }),
        _ => Err(BytecodeError::UnsupportedFeature {
            feature: format!(
                "enum constant {constant} with a body: more than one constructor of {enum_name} takes {arity} argument(s)"
            ),
        }),
    }
}

/// Whether a child of an enum constant is a class member, which makes the constant's body.
fn is_class_member_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::METHOD_DECL
            | SyntaxKind::FIELD_DECL
            | SyntaxKind::STATIC_INIT
            | SyntaxKind::INSTANCE_INIT
            | SyntaxKind::MODIFIER_LIST
    )
}

fn hidden_enum_params() -> Vec<Param> {
    vec![
        Param {
            name: ENUM_NAME_PARAM.to_string(),
            ty: JavaType::Object("java/lang/String".to_string()),
        },
        Param {
            name: ENUM_ORDINAL_PARAM.to_string(),
            ty: JavaType::Int,
        },
    ]
}

fn hidden_enum_args() -> Vec<Expression> {
    vec![
        Expression::NameExpr(ENUM_NAME_PARAM.to_string()),
        Expression::NameExpr(ENUM_ORDINAL_PARAM.to_string()),
    ]
}

/// The types a declaration header names after a keyword, as internal names: `implements A, B` or
/// `extends C`. A type with type arguments names its erasure.
fn header_types_after(node: &SyntaxNode, keyword: SyntaxKind) -> BytecodeResult<Vec<String>> {
    let mut after_keyword = false;
    let mut names = Vec::new();
    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) => {
                if token.kind() == keyword {
                    after_keyword = true;
                } else if token.kind() == SyntaxKind::L_BRACE {
                    break;
                }
            }
            NodeOrToken::Node(child) => {
                if after_keyword && is_type_node(child.kind()) {
                    match convert_type(child)? {
                        JavaType::Object(name) => names.push(name),
                        other => {
                            return Err(parse_error(format!("unsupported supertype {other:?}")))
                        }
                    }
                }
            }
        }
    }
    Ok(names)
}

/// The type arguments named in a declaration's `extends` and `implements` clauses, as
/// [`type_arguments_from_type`] reads them from each supertype.
fn header_type_arguments(node: &SyntaxNode) -> Vec<JavaType> {
    let mut in_header = false;
    let mut arguments = Vec::new();
    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::EXTENDS_KW | SyntaxKind::IMPLEMENTS_KW => in_header = true,
                SyntaxKind::L_BRACE => break,
                _ => {}
            },
            NodeOrToken::Node(child) => {
                if in_header && is_type_node(child.kind()) {
                    arguments.extend(type_arguments_from_type(&child));
                }
            }
        }
    }
    arguments
}

/// Convert an interface declaration. An interface is a class with ACC_INTERFACE. A method with no
/// body is abstract, and a `default` or `static` method has a body. Its fields are static
/// constants, initialized in `<clinit>`. It extends `java.lang.Object` and lists the interfaces it
/// extends.
fn convert_interface_decl(node: SyntaxNode) -> BytecodeResult<ClassDecl> {
    convert_interface_like(node, false)
}

/// Convert an annotation type declaration. It is an interface with ACC_ANNOTATION. Its elements are
/// abstract methods. An element's `default` value is not kept: no code reads it in this runtime.
/// It does not extend `java.lang.annotation.Annotation`, which the VM has no class for, so the
/// superinterface is not recorded.
fn convert_annotation_decl(node: SyntaxNode) -> BytecodeResult<ClassDecl> {
    convert_interface_like(node, true)
}

fn convert_interface_like(node: SyntaxNode, is_annotation: bool) -> BytecodeResult<ClassDecl> {
    let name = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .ok_or_else(|| parse_error("Interface declaration missing name"))?
        .replace('.', "/");
    let class_tvars = collect_type_params(&node);
    let Members {
        methods,
        fields,
        static_init,
        instance_init,
    } = convert_members(&node, &class_tvars, true)?;
    Ok(ClassDecl {
        name,
        super_name: "java/lang/Object".to_string(),
        fields,
        methods,
        enclosing_static_fields: Vec::new(),
        static_init,
        instance_init,
        is_interface: true,
        is_annotation,
        declared_abstract: false,
        interfaces: header_types_after(&node, SyntaxKind::EXTENDS_KW)?,
        supertype_arguments: header_type_arguments(&node),
    })
}

fn convert_class_decl(class_node: SyntaxNode) -> BytecodeResult<ClassDecl> {
    // Class name: first direct IDENT token of the CLASS_DECL.
    let name = class_node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .ok_or_else(|| parse_error("Class declaration missing name"))?
        .replace('.', "/");

    // Class-level type parameters (`class Box<T>`) are erased to Object in all
    // descriptors of the class. A *nested* class also sees the type variables of
    // its enclosing classes (`Graph<E>`'s inner `Node` uses `E`), which must
    // erase the same way — else `Node(E name)` registers as `(LE;)V` but is
    // called as `(Object)V` and fails to resolve. Gather them from ancestors too.
    let mut class_tvars = collect_type_params(&class_node);
    for anc in class_node.ancestors().skip(1) {
        if matches!(
            anc.kind(),
            SyntaxKind::CLASS_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
        ) {
            class_tvars.extend(collect_type_params(&anc));
        }
    }
    let Members {
        methods,
        fields,
        static_init,
        instance_init,
    } = convert_members(&class_node, &class_tvars, false)?;
    let super_name = extends_super_name(&class_node);

    Ok(ClassDecl {
        is_interface: false,
        is_annotation: false,
        declared_abstract: has_abstract_modifier(&class_node),
        interfaces: header_types_after(&class_node, SyntaxKind::IMPLEMENTS_KW)?,
        supertype_arguments: header_type_arguments(&class_node),
        enclosing_static_fields: Vec::new(),
        static_init,
        instance_init,
        name,
        super_name,
        fields,
        methods,
    })
}

/// The internal name of a class's `extends` superclass, or `java/lang/Object`
/// when none is declared. The top-level `extends` type is the first type node
/// after the direct `EXTENDS_KW` token (type-parameter bounds nest their own
/// `extends` inside `TYPE_PARAM_LIST`, so a direct-child scan ignores them).
fn extends_super_name(class_node: &SyntaxNode) -> String {
    let mut after_extends = false;
    for elem in class_node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::EXTENDS_KW => after_extends = true,
            NodeOrToken::Node(n) if after_extends && is_type_node(n.kind()) => {
                if let Ok(JavaType::Object(name)) = convert_type(n) {
                    return name;
                }
                return "java/lang/Object".to_string();
            }
            _ => {}
        }
    }
    "java/lang/Object".to_string()
}

/// Convert the methods, constructors, and fields declared directly under a
/// class/record body. Shared by [`convert_class_decl`] and
/// [`convert_record_decl`].
/// The members of a type declaration, with the initializers that its fields and blocks make.
struct Members {
    methods: Vec<MethodDecl>,
    fields: Vec<FieldDecl>,
    static_init: Vec<Statement>,
    instance_init: Vec<Statement>,
}

fn convert_members(
    decl: &SyntaxNode,
    class_tvars: &HashSet<String>,
    implicitly_static_fields: bool,
) -> BytecodeResult<Members> {
    let mut methods = Vec::new();
    let mut fields = Vec::new();
    let mut static_init = Vec::new();
    let mut instance_init = Vec::new();
    // The parser emits a member's modifiers as a `MODIFIER_LIST` sibling that
    // immediately precedes the member node (rather than nesting it inside).
    // Track the most recent one so we can apply it to the next member.
    let mut pending_static = false;
    for child in decl.children() {
        match child.kind() {
            SyntaxKind::MODIFIER_LIST => {
                pending_static = contains_static(&child);
            }
            SyntaxKind::METHOD_DECL => {
                let is_static = pending_static || has_static_modifier(&child);
                if let Some(method) = convert_method(child, is_static, class_tvars)? {
                    methods.push(method);
                }
                pending_static = false;
            }
            SyntaxKind::CONSTRUCTOR_DECL => {
                methods.push(convert_constructor(child, class_tvars)?);
                pending_static = false;
            }
            SyntaxKind::FIELD_DECL => {
                let is_static =
                    implicitly_static_fields || pending_static || has_static_modifier(&child);
                let initializers = convert_field_initializers(&child, is_static)?;
                fields.extend(convert_field(child, is_static, class_tvars)?);
                if is_static {
                    static_init.extend(initializers);
                } else {
                    instance_init.extend(initializers);
                }
                pending_static = false;
            }
            // Initializer blocks run in source order with the field initializers.
            SyntaxKind::STATIC_INIT => {
                if let Some(block) = child
                    .children()
                    .find(|c| c.kind() == SyntaxKind::BLOCK_STMT)
                {
                    static_init.push(Statement::Block(convert_block(block)?));
                }
                pending_static = false;
            }
            SyntaxKind::INSTANCE_INIT => {
                if let Some(block) = child
                    .children()
                    .find(|c| c.kind() == SyntaxKind::BLOCK_STMT)
                {
                    instance_init.push(Statement::Block(convert_block(block)?));
                }
                pending_static = false;
            }
            _ => {}
        }
    }
    Ok(Members {
        methods,
        fields,
        static_init,
        instance_init,
    })
}

/// The assignments that a field declaration's initializers make, in declaration order. A static
/// field is assigned by its simple name, which resolves to the field in `<clinit>`. An instance
/// field is assigned through `this`, so a constructor parameter of the same name is not the
/// target.
fn convert_field_initializers(
    node: &SyntaxNode,
    is_static: bool,
) -> BytecodeResult<Vec<Statement>> {
    let declared = match convert_var_decl(node.clone())? {
        Statement::Multi(list) => list,
        single => vec![single],
    };
    let mut assignments = Vec::new();
    for declaration in declared {
        let Statement::VariableDeclaration {
            name,
            initializer: Some(value),
            ..
        } = declaration
        else {
            continue;
        };
        let target = if is_static {
            Expression::NameExpr(name)
        } else {
            Expression::FieldAccess {
                receiver: None,
                field: name,
                class: String::new(),
            }
        };
        assignments.push(Statement::Expression(Expression::Assignment {
            target: Box::new(target),
            value: Box::new(value),
        }));
    }
    Ok(assignments)
}

/// Parse the `PARAM_LIST` of a method/constructor/record header into typed
/// parameters, erasing any type variables in scope.
fn convert_params(decl: &SyntaxNode, tvars: &HashSet<String>) -> BytecodeResult<Vec<Param>> {
    let mut params = Vec::new();
    if let Some(param_list) = decl.children().find(|c| c.kind() == SyntaxKind::PARAM_LIST) {
        for param in param_list
            .children()
            .filter(|c| c.kind() == SyntaxKind::PARAM)
        {
            params.push(convert_param(&param, tvars)?);
        }
    }
    Ok(params)
}

/// Parse a single `PARAM` node into a typed `Param`.
///
/// A parameter's type can be split across sibling nodes/tokens, exactly like a
/// variable declaration: `char[] v` is `PRIMITIVE_TYPE` + a bracket-only
/// `ARRAY_TYPE` sibling, `int b[]` is `PRIMITIVE_TYPE` + trailing `[]` tokens
/// *after* the name, and `String... rest` is `CLASS_TYPE` + `ELLIPSIS` (a
/// varargs array). Reading only the first type node would drop the array
/// dimension and produce a wrong descriptor; collecting all dims keeps it right.
/// The name may be a contextual keyword (`to`, `record`, `var`, …) — Java allows
/// those as identifiers, so accept any `is_identifier()` token, not just `IDENT`.
fn convert_param(param: &SyntaxNode, tvars: &HashSet<String>) -> BytecodeResult<Param> {
    let mut base_type: Option<JavaType> = None;
    let mut dims: u8 = 0;
    let mut name: Option<String> = None;
    let mut seen_name = false;

    for elem in param.children_with_tokens() {
        match elem {
            NodeOrToken::Node(child) => match child.kind() {
                // A bracket-only `ARRAY_TYPE` sibling (`char[] v`) is one extra
                // dimension; an `ARRAY_TYPE` that nests its element is the base.
                SyntaxKind::ARRAY_TYPE if !seen_name => {
                    if child.children().any(|c| is_type_node(c.kind())) {
                        base_type = Some(convert_type(child)?);
                    } else {
                        dims += 1;
                    }
                }
                k if is_type_node(k) && !seen_name && base_type.is_none() => {
                    base_type = Some(convert_type(child)?);
                }
                _ => {}
            },
            NodeOrToken::Token(token) => match token.kind() {
                // Varargs `T...` is sugar for a `T[]` parameter.
                SyntaxKind::ELLIPSIS if !seen_name => dims += 1,
                k if k.is_identifier() && !seen_name => {
                    seen_name = true;
                    name = Some(token.text().to_string());
                }
                // C-style `int a[]` — trailing dimension after the name.
                SyntaxKind::L_BRACKET if seen_name => dims += 1,
                _ => {}
            },
        }
    }

    let name = name.ok_or_else(|| parse_error("Parameter missing name"))?;
    let ty = erase_type_vars(compose_var_type(base_type, dims), tvars);
    Ok(Param { name, ty })
}

/// Convert a `record Name(components) { … }` to a `ClassDecl`, synthesizing the
/// final component fields, the canonical constructor (assigning each component),
/// and the component accessor methods — unless the body already declares them.
/// (`super` is `java/lang/Object`, not `java/lang/Record`, since the stubbed VM
/// lacks `Record.<init>`; record identity/`toString` are not synthesized.)
fn convert_record_decl(node: SyntaxNode) -> BytecodeResult<ClassDecl> {
    let name = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .ok_or_else(|| parse_error("Record declaration missing name"))?
        .replace('.', "/");

    let class_tvars = collect_type_params(&node);
    let components = convert_params(&node, &class_tvars)?;
    let Members {
        mut methods,
        mut fields,
        static_init,
        instance_init,
    } = convert_members(&node, &class_tvars, false)?;

    // Synthesize a final field per component (skip any the body already declares).
    for c in &components {
        if !fields.iter().any(|f| f.name == c.name) {
            fields.push(FieldDecl {
                name: c.name.clone(),
                ty: c.ty.clone(),
                is_static: false,
            });
        }
    }

    // Canonical constructor: `this.c = c;` for each component (unless declared).
    if !methods.iter().any(|m| m.name == "<init>") {
        let body = components
            .iter()
            .map(|c| {
                Statement::Expression(Expression::Assignment {
                    target: Box::new(Expression::FieldAccess {
                        receiver: Some(Box::new(Expression::This)),
                        field: c.name.clone(),
                        class: name.clone(),
                    }),
                    value: Box::new(Expression::NameExpr(c.name.clone())),
                })
            })
            .collect();
        methods.push(MethodDecl {
            is_abstract: false,
            name: "<init>".to_string(),
            params: components.clone(),
            return_type: JavaType::Void,
            is_static: false,
            body,
        });
    }

    // Accessor `c()` returning the field (unless the body declares it).
    for c in &components {
        let has_accessor = methods
            .iter()
            .any(|m| m.name == c.name && m.params.is_empty());
        if !has_accessor {
            methods.push(MethodDecl {
                is_abstract: false,
                name: c.name.clone(),
                params: Vec::new(),
                return_type: c.ty.clone(),
                is_static: false,
                body: vec![Statement::Return(Some(Expression::FieldAccess {
                    receiver: Some(Box::new(Expression::This)),
                    field: c.name.clone(),
                    class: name.clone(),
                }))],
            });
        }
    }

    Ok(ClassDecl {
        is_interface: false,
        is_annotation: false,
        declared_abstract: false,
        interfaces: header_types_after(&node, SyntaxKind::IMPLEMENTS_KW)?,
        supertype_arguments: header_type_arguments(&node),
        enclosing_static_fields: Vec::new(),
        static_init,
        instance_init,
        name,
        super_name: "java/lang/Object".to_string(),
        fields,
        methods,
    })
}

/// True if a `static` keyword appears anywhere within the node's subtree.
/// `MODIFIER_LIST` nests each keyword inside a `MODIFIER` node, so a shallow
/// token scan is insufficient.
fn contains_static(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .any(|e| e.kind() == SyntaxKind::STATIC_KW)
}

/// True if the declaration has an `abstract` modifier, in its own modifier list or in the
/// one the parser puts just before it.
fn has_abstract_modifier(node: &SyntaxNode) -> bool {
    node.children()
        .chain(node.prev_sibling())
        .filter(|c| c.kind() == SyntaxKind::MODIFIER_LIST)
        .any(|m| {
            m.descendants_with_tokens()
                .any(|e| e.kind() == SyntaxKind::ABSTRACT_KW)
        })
}

fn has_static_modifier(node: &SyntaxNode) -> bool {
    node.children()
        .find(|c| c.kind() == SyntaxKind::MODIFIER_LIST)
        .map(|m| contains_static(&m))
        .unwrap_or(false)
}

/// Convert a METHOD_DECL node. Returns `None` for methods without a body
/// (abstract/native) which the backend cannot yet emit.
fn convert_method(
    node: SyntaxNode,
    is_static: bool,
    class_tvars: &HashSet<String>,
) -> BytecodeResult<Option<MethodDecl>> {
    // Name: first direct IDENT token of the METHOD_DECL.
    let name = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .ok_or_else(|| parse_error("Method declaration missing name"))?;

    // Type variables in scope = class-level ∪ this method's own (`<T> T id(...)`).
    let mut tvars = class_tvars.clone();
    tvars.extend(collect_type_params(&node));

    // Return type: first child node that is a type. Absent → void.
    let return_type = node
        .children()
        .find(|c| is_type_node(c.kind()))
        .map(convert_type)
        .transpose()?
        .map(|t| erase_type_vars(t, &tvars))
        .unwrap_or(JavaType::Void);

    // Parameters from the PARAM_LIST.
    let params = convert_params(&node, &tvars)?;

    // Body. A method with no body is abstract (or native): it is declared, with no code.
    let (body, is_abstract) = match node.children().find(|c| c.kind() == SyntaxKind::BLOCK_STMT) {
        Some(block) => (convert_block(block)?, false),
        None => (Vec::new(), true),
    };

    Ok(Some(MethodDecl {
        name,
        params,
        return_type,
        is_static,
        body,
        is_abstract,
    }))
}

/// Convert a CONSTRUCTOR_DECL into a `<init>` method returning void.
fn convert_constructor(
    node: SyntaxNode,
    class_tvars: &HashSet<String>,
) -> BytecodeResult<MethodDecl> {
    let mut tvars = class_tvars.clone();
    tvars.extend(collect_type_params(&node));
    let params = convert_params(&node, &tvars)?;
    let body = match node.children().find(|c| c.kind() == SyntaxKind::BLOCK_STMT) {
        Some(block) => convert_block(block)?,
        None => Vec::new(),
    };
    Ok(MethodDecl {
        is_abstract: false,
        name: "<init>".to_string(),
        params,
        return_type: JavaType::Void,
        is_static: false,
        body,
    })
}

/// Convert a FIELD_DECL node into one `FieldDecl` per declared name.
fn convert_field(
    node: SyntaxNode,
    is_static: bool,
    class_tvars: &HashSet<String>,
) -> BytecodeResult<Vec<FieldDecl>> {
    let ty = node
        .children()
        .find(|c| is_type_node(c.kind()))
        .map(convert_type)
        .transpose()?
        .map(|t| erase_type_vars(t, class_tvars))
        .unwrap_or(JavaType::Int);
    let fields = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| FieldDecl {
            name: t.text().to_string(),
            ty: ty.clone(),
            is_static,
        })
        .collect();
    Ok(fields)
}

/// Resolve a (possibly simple) class name to an internal JVM name.
///
/// Unqualified references to common `java.lang` types are mapped to their fully
/// qualified internal form; already-qualified names (containing `/`) are kept.
/// Collect the IDENT segments of a class/qualified type for type erasure,
/// pruning any identifiers nested inside a generic type-argument list. Walks
/// the subtree in source order but never descends into a `TYPE_ARGS` node, so
/// `Map<String, List<T>>` yields just `["Map"]` and `java.util.List<T>` yields
/// `["java", "util", "List"]`.
fn collect_raw_type_idents(node: &SyntaxNode, out: &mut Vec<String>) {
    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => {
                out.push(t.text().to_string());
            }
            NodeOrToken::Node(n) if n.kind() != SyntaxKind::TYPE_ARGS => {
                collect_raw_type_idents(&n, out);
            }
            _ => {}
        }
    }
}

/// Collect the names declared in a node's direct `TYPE_PARAM_LIST`, e.g. the
/// `T`, `K`, `V` of `class Box<T>` or `<K, V> Map<K, V> of(...)`. These are the
/// only names we erase to `java/lang/Object` — using the *declaration* as the
/// signal (not a naming convention) avoids mistaking a one-letter class for a
/// type variable.
fn collect_type_params(node: &SyntaxNode) -> HashSet<String> {
    let mut set = HashSet::new();
    if let Some(list) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::TYPE_PARAM_LIST)
    {
        for tp in list
            .children()
            .filter(|c| c.kind() == SyntaxKind::TYPE_PARAM)
        {
            if let Some(id) = tp
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT)
            {
                set.insert(id.text().to_string());
            }
        }
    }
    set
}

/// Erase declared type variables to `java/lang/Object` (generics → erasure),
/// descending through array element types. Names not in `tvars` are untouched.
fn erase_type_vars(ty: JavaType, tvars: &HashSet<String>) -> JavaType {
    match ty {
        JavaType::Object(name) if tvars.contains(&name) => {
            JavaType::Object("java/lang/Object".to_string())
        }
        JavaType::Array {
            element_type,
            dimensions,
        } => JavaType::Array {
            element_type: Box::new(erase_type_vars(*element_type, tvars)),
            dimensions,
        },
        other => other,
    }
}

pub(crate) fn qualify_class_name(name: &str) -> String {
    if name.contains('/') {
        return name.to_string();
    }
    // Import- and unit-aware resolution takes precedence over the built-in
    // java.lang defaults: a class declared in this unit stays unqualified (it is
    // a user type), an explicit single-type import wins outright, and a wildcard
    // import qualifies only names Hyperion knows how to model.
    if let Some(resolved) = UNIT_CTX.with(|c| {
        let ctx = c.borrow();
        if ctx.unit_classes.contains(name) {
            return Some(name.to_string());
        }
        if let Some(fqn) = ctx.imports.get(name) {
            return Some(fqn.clone());
        }
        for pkg in &ctx.wildcards {
            if pkg == "java/util" && JAVA_UTIL_TYPES.contains(&name) {
                return Some(format!("java/util/{name}"));
            }
        }
        None
    }) {
        return resolved;
    }
    match name {
        "String" => "java/lang/String".to_string(),
        "Object" => "java/lang/Object".to_string(),
        "Integer" => "java/lang/Integer".to_string(),
        "Long" => "java/lang/Long".to_string(),
        "Double" => "java/lang/Double".to_string(),
        "Float" => "java/lang/Float".to_string(),
        "Boolean" => "java/lang/Boolean".to_string(),
        "Character" => "java/lang/Character".to_string(),
        "System" => "java/lang/System".to_string(),
        "StringBuilder" => "java/lang/StringBuilder".to_string(),
        "Math" => "java/lang/Math".to_string(),
        "Exception" => "java/lang/Exception".to_string(),
        "RuntimeException" => "java/lang/RuntimeException".to_string(),
        "Throwable" => "java/lang/Throwable".to_string(),
        // Common java.lang exceptions/errors fall back to the java.lang package.
        other if other.ends_with("Exception") || other.ends_with("Error") => {
            format!("java/lang/{other}")
        }
        // The rest of java.lang that the unit can name: Runnable, Comparable, AutoCloseable.
        other if hyperion_types::is_core_java_lang_type(other) => format!("java/lang/{other}"),
        other => other.to_string(),
    }
}

fn is_expression_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::INT_LITERAL
            | SyntaxKind::LONG_LITERAL
            | SyntaxKind::FLOAT_LITERAL
            | SyntaxKind::DOUBLE_LITERAL
            | SyntaxKind::STRING_LITERAL
            | SyntaxKind::BOOLEAN_LITERAL
            | SyntaxKind::NULL_LITERAL
            | SyntaxKind::CHAR_LITERAL
            | SyntaxKind::IDENT
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::METHOD_REF_EXPR
            | SyntaxKind::CONSTRUCTOR_REF_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
            // The kinds below are not all converted, but each must be an expression, so that a
            // filter over expression nodes cannot drop one silently. Each either converts or is
            // refused in `convert_expression`.
            | SyntaxKind::INSTANCEOF_EXPR
            | SyntaxKind::PATTERN_INSTANCEOF_EXPR
            | SyntaxKind::CLASS_LITERAL_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::SWITCH_EXPR
    )
}

fn is_type_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PRIMITIVE_TYPE
            | SyntaxKind::CLASS_TYPE
            | SyntaxKind::ARRAY_TYPE
            | SyntaxKind::VAR_TYPE
            | SyntaxKind::INT_KW
            | SyntaxKind::LONG_KW
            | SyntaxKind::FLOAT_KW
            | SyntaxKind::DOUBLE_KW
            | SyntaxKind::BOOLEAN_KW
            | SyntaxKind::CHAR_KW
            | SyntaxKind::BYTE_KW
            | SyntaxKind::SHORT_KW
            | SyntaxKind::VOID_KW
            | SyntaxKind::IDENT
    )
}

fn token_to_binary_op(kind: SyntaxKind) -> Option<BinaryOp> {
    match kind {
        SyntaxKind::PLUS => Some(BinaryOp::Add),
        SyntaxKind::MINUS => Some(BinaryOp::Subtract),
        SyntaxKind::STAR => Some(BinaryOp::Multiply),
        SyntaxKind::SLASH => Some(BinaryOp::Divide),
        SyntaxKind::PERCENT => Some(BinaryOp::Modulo),
        SyntaxKind::AMP => Some(BinaryOp::BitwiseAnd),
        SyntaxKind::PIPE => Some(BinaryOp::BitwiseOr),
        SyntaxKind::CARET => Some(BinaryOp::BitwiseXor),
        SyntaxKind::SHL => Some(BinaryOp::LeftShift),
        SyntaxKind::SHR => Some(BinaryOp::RightShift),
        SyntaxKind::USHR => Some(BinaryOp::UnsignedRightShift),
        SyntaxKind::AMP_AMP => Some(BinaryOp::LogicalAnd),
        SyntaxKind::PIPE_PIPE => Some(BinaryOp::LogicalOr),
        SyntaxKind::EQ_EQ => Some(BinaryOp::Equal),
        SyntaxKind::BANG_EQ => Some(BinaryOp::NotEqual),
        SyntaxKind::LT => Some(BinaryOp::LessThan),
        SyntaxKind::LT_EQ => Some(BinaryOp::LessOrEqual),
        SyntaxKind::GT => Some(BinaryOp::GreaterThan),
        SyntaxKind::GT_EQ => Some(BinaryOp::GreaterOrEqual),
        _ => None,
    }
}

fn token_to_unary_op(kind: SyntaxKind) -> Option<UnaryOp> {
    match kind {
        SyntaxKind::MINUS => Some(UnaryOp::Negate),
        SyntaxKind::BANG => Some(UnaryOp::LogicalNot),
        SyntaxKind::TILDE => Some(UnaryOp::BitwiseNot),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};

    fn parse_and_convert(source: &str) -> BytecodeResult<Vec<Statement>> {
        let tokens = Lexer::new(source).collect();
        let (green_node, errors) = Parser::parse(tokens);

        if !errors.is_empty() {
            return Err(parse_error(format!("Parse errors: {:?}", errors)));
        }

        let root = SyntaxNode::new_root(green_node);
        convert_source_file(root)
    }

    #[test]
    fn test_convert_int_literal() {
        let source = "class Test { void test() { 42; } }";
        let result = parse_and_convert(source);
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_convert_binary_expr() {
        let source = "class Test { void test() { 1 + 2; } }";
        let result = parse_and_convert(source);
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    #[test]
    fn test_convert_var_decl() {
        let source = "class Test { void test() { int x = 5; } }";
        let result = parse_and_convert(source);
        assert!(result.is_ok(), "Failed: {:?}", result);
    }

    fn parse_class(source: &str) -> ClassDecl {
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "parse errors: {:?}", errors);
        convert_class_file(SyntaxNode::new_root(green)).expect("convert to ClassDecl")
    }

    // Generic type arguments must be erased from descriptors: `List<String>` is
    // `Ljava/util/List;`, not the bogus `Ljava/util/List/String;` a flat IDENT
    // walk produced. Bare type variables erase to Object.
    #[test]
    fn erasure_drops_type_arguments() {
        // `T` is a *declared* class type parameter, so it erases to Object; a
        // one-letter class name that is NOT declared as a type param would not.
        let src = "class Box<T> { \
            java.util.List<String> items() { return null; } \
            void put(java.util.Map<String, Integer> m) {} \
            T identity(T x) { return x; } }";
        let c = parse_class(src);
        let desc = |name: &str| {
            c.methods
                .iter()
                .find(|m| m.name == name)
                .unwrap_or_else(|| panic!("method {name} not found"))
                .descriptor()
        };
        assert_eq!(desc("items"), "()Ljava/util/List;");
        assert_eq!(desc("put"), "(Ljava/util/Map;)V");
        // Bare type variable T erases to java/lang/Object on both sides.
        assert_eq!(desc("identity"), "(Ljava/lang/Object;)Ljava/lang/Object;");
    }
}
