// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Reachability analysis (JLS 14.22): `missing return statement` and
//! `unreachable statement`, plus `return` value/void agreement.
//!
//! The analysis is three-valued. A statement *definitely completes*,
//! *definitely cannot complete*, or *may or may not*: the last covers loop
//! conditions that could be constant expressions (`while (DEBUG)`), enhanced
//! switches and anything else the syntax alone cannot decide. Errors are only
//! reported from the two definite outcomes.

use hyperion_parser::modifiers::declaration_modifiers;
use hyperion_parser::{SyntaxKind, SyntaxNode};
use rowan::TextRange;

use crate::file_semantics::FileSemantics;
use crate::resolved_name::ResolvedName;
use crate::semantic_error::{SemanticError, SemanticErrorKind};

/// Can control flow run off the end of a statement?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Definitely completes normally.
    Completes,
    /// Definitely does not (return, throw, endless loop, ...).
    Stops,
    /// Cannot be decided from syntax.
    Unknown,
}

impl Flow {
    /// Either branch may be taken.
    const fn either(self, other: Self) -> Self {
        match (self, other) {
            (Self::Completes, _) | (_, Self::Completes) => Self::Completes,
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            _ => Self::Stops,
        }
    }
}

/// Value of a loop condition as far as JLS 15.29 constant folding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Condition {
    ConstantTrue,
    /// Provably not a constant expression.
    NotConstant,
    /// Might be a constant expression (or a constant false).
    Unknown,
}

#[derive(Debug)]
struct Frame {
    label: Option<String>,
    is_loop: bool,
    is_switch: bool,
    /// A reachable `break` targets this frame.
    broken: bool,
    /// A reachable `continue` targets this frame.
    continued: bool,
}

struct Analyzer<'a> {
    file_semantics: &'a FileSemantics,
    root: &'a SyntaxNode,
    frames: Vec<Frame>,
    errors: Vec<SemanticError>,
    /// An unreachable statement was reported in the current body; the end of
    /// the body is then in javac's "recovery" state and is not judged.
    recovered: bool,
}

/// Run the reachability checks over every body in the file.
pub fn check_flow(root: &SyntaxNode, file_semantics: &FileSemantics) -> Vec<SemanticError> {
    let mut analyzer = Analyzer {
        file_semantics,
        root,
        frames: Vec::new(),
        errors: Vec::new(),
        recovered: false,
    };
    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::METHOD_DECL => analyzer.method(&node),
            SyntaxKind::CONSTRUCTOR_DECL | SyntaxKind::STATIC_INIT | SyntaxKind::INSTANCE_INIT => {
                if let Some(body) = body_block(&node) {
                    analyzer.body(&body, BodyKind::Void);
                }
            }
            SyntaxKind::LAMBDA_EXPR => {
                if let Some(body) = node
                    .children()
                    .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
                {
                    analyzer.body(&body, BodyKind::Lambda);
                }
            }
            SyntaxKind::SWITCH_EXPR => {
                for clause in node
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::CASE_CLAUSE)
                {
                    for block in clause
                        .children()
                        .filter(|child| child.kind() == SyntaxKind::BLOCK_STMT)
                    {
                        analyzer.body(&block, BodyKind::Lambda);
                    }
                }
            }
            _ => {}
        }
    }
    analyzer.errors
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyKind {
    /// A method returning a value: falling off the end is an error.
    Value,
    /// Constructors, initializers and void methods.
    Void,
    /// Lambda and switch-expression blocks: only reachability inside is checked.
    Lambda,
}

fn body_block(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.children()
        .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
}

const fn is_statement(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::BLOCK_STMT
            | SyntaxKind::EXPR_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::DO_WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOREACH_STMT
            | SyntaxKind::SWITCH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::TRY_WITH_RESOURCES_STMT
            | SyntaxKind::SYNCHRONIZED_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::THROW_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::ASSERT_STMT
            | SyntaxKind::EMPTY_STMT
            | SyntaxKind::LABELED_STMT
            | SyntaxKind::YIELD_STMT
            | SyntaxKind::LOCAL_VAR_DECL
            | SyntaxKind::CLASS_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::ENUM_DECL
            | SyntaxKind::RECORD_DECL
    )
}

impl Analyzer<'_> {
    fn method(&mut self, node: &SyntaxNode) {
        let Some(body) = body_block(node) else {
            return; // abstract / native
        };
        let returns_void = node
            .children()
            .find(|child| {
                matches!(
                    child.kind(),
                    SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::CLASS_TYPE | SyntaxKind::ARRAY_TYPE
                )
            })
            .is_some_and(|ty| {
                ty.children_with_tokens()
                    .any(|element| element.kind() == SyntaxKind::VOID_KW)
            });
        self.body(
            &body,
            if returns_void {
                BodyKind::Void
            } else {
                BodyKind::Value
            },
        );
    }

    fn body(&mut self, body: &SyntaxNode, kind: BodyKind) {
        self.frames.clear();
        self.recovered = false;
        let flow = self.block(body);
        if kind == BodyKind::Value && flow == Flow::Completes && !self.recovered {
            let closing = body
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .filter(|token| token.kind() == SyntaxKind::R_BRACE)
                .last()
                .map_or_else(|| body.text_range(), |token| token.text_range());
            self.errors.push(SemanticError::new(
                closing,
                SemanticErrorKind::MissingReturn,
            ));
        }
    }

    /// Statements of a block or switch group, reporting the first
    /// unreachable one.
    fn sequence(&mut self, statements: impl Iterator<Item = SyntaxNode>) -> Flow {
        let mut state = Flow::Completes;
        let mut reported = false;
        for statement in statements {
            if state == Flow::Stops {
                if !reported && statement.kind() != SyntaxKind::EMPTY_STMT {
                    self.errors.push(SemanticError::new(
                        statement_head_range(&statement),
                        SemanticErrorKind::UnreachableStatement,
                    ));
                    self.recovered = true;
                    reported = true;
                }
                // javac keeps analysing as if reachable (no cascade).
                state = Flow::Completes;
            }
            let next = self.statement(&statement);
            state = match (state, next) {
                (Flow::Unknown, Flow::Stops) => Flow::Stops,
                (Flow::Unknown, _) => Flow::Unknown,
                (_, next) => next,
            };
        }
        state
    }

    fn block(&mut self, node: &SyntaxNode) -> Flow {
        self.sequence(node.children().filter(|child| is_statement(child.kind())))
    }

    fn statement(&mut self, node: &SyntaxNode) -> Flow {
        match node.kind() {
            SyntaxKind::BLOCK_STMT => self.block(node),
            SyntaxKind::RETURN_STMT | SyntaxKind::THROW_STMT | SyntaxKind::YIELD_STMT => {
                Flow::Stops
            }
            SyntaxKind::BREAK_STMT => {
                self.jump(node, true);
                Flow::Stops
            }
            SyntaxKind::CONTINUE_STMT => {
                self.jump(node, false);
                Flow::Stops
            }
            SyntaxKind::IF_STMT => self.if_statement(node),
            SyntaxKind::WHILE_STMT | SyntaxKind::FOR_STMT => self.loop_statement(node),
            SyntaxKind::FOREACH_STMT => {
                self.push_loop(node);
                self.loop_body(node);
                self.frames.pop();
                Flow::Completes
            }
            SyntaxKind::DO_WHILE_STMT => self.do_while(node),
            SyntaxKind::LABELED_STMT => self.labeled(node),
            SyntaxKind::SYNCHRONIZED_STMT => {
                body_block(node).map_or(Flow::Completes, |body| self.block(&body))
            }
            SyntaxKind::SWITCH_STMT => self.switch_statement(node),
            SyntaxKind::TRY_STMT | SyntaxKind::TRY_WITH_RESOURCES_STMT => self.try_statement(node),
            _ => Flow::Completes,
        }
    }

    fn jump(&mut self, node: &SyntaxNode, is_break: bool) {
        let label = node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|token| token.kind().is_identifier())
            .map(|token| token.text().to_owned());
        let target = self.frames.iter_mut().rev().find(|frame| match &label {
            Some(name) => frame.label.as_deref() == Some(name.as_str()),
            None if is_break => frame.is_loop || frame.is_switch,
            None => frame.is_loop,
        });
        if let Some(frame) = target {
            if is_break {
                frame.broken = true;
            } else {
                frame.continued = true;
            }
        }
    }

    fn push_loop(&mut self, node: &SyntaxNode) {
        // A `label: while (..)` shares its label with the loop.
        let label = node
            .parent()
            .filter(|parent| parent.kind() == SyntaxKind::LABELED_STMT)
            .and_then(|parent| label_name(&parent));
        self.frames.push(Frame {
            label,
            is_loop: true,
            is_switch: false,
            broken: false,
            continued: false,
        });
    }

    fn loop_body(&mut self, node: &SyntaxNode) {
        let body = node
            .children()
            .filter(|child| is_statement(child.kind()))
            .last();
        if let Some(body) = body {
            self.statement(&body);
        }
    }

    fn if_statement(&mut self, node: &SyntaxNode) -> Flow {
        let mut branches = node
            .children()
            .filter(|child| is_statement(child.kind()))
            .collect::<Vec<_>>()
            .into_iter();
        let then_flow = branches
            .next()
            .map_or(Flow::Completes, |branch| self.statement(&branch));
        match branches.next() {
            // `if (c) S` completes whenever it is reachable (JLS 14.22).
            None => {
                let _ = then_flow;
                Flow::Completes
            }
            Some(else_branch) => {
                let else_flow = self.statement(&else_branch);
                then_flow.either(else_flow)
            }
        }
    }

    fn loop_statement(&mut self, node: &SyntaxNode) -> Flow {
        let condition = if node.kind() == SyntaxKind::FOR_STMT {
            // for (init; cond; update): the condition sits between the
            // semicolons; an empty condition is constant true.
            self.for_condition(node)
        } else {
            node.children()
                .find(|child| !is_statement(child.kind()))
                .map_or(Condition::Unknown, |cond| self.condition(&cond))
        };
        self.push_loop(node);
        self.loop_body(node);
        let frame = self.frames.pop();
        let broken = frame.is_some_and(|frame| frame.broken);
        match condition {
            Condition::NotConstant => Flow::Completes,
            Condition::ConstantTrue if broken => Flow::Completes,
            Condition::ConstantTrue => Flow::Stops,
            Condition::Unknown if broken => Flow::Completes,
            Condition::Unknown => Flow::Unknown,
        }
    }

    fn for_condition(&self, node: &SyntaxNode) -> Condition {
        let mut semicolons = 0;
        let mut condition: Option<SyntaxNode> = None;
        for element in node.children_with_tokens() {
            match element {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::SEMICOLON => {
                    semicolons += 1;
                }
                rowan::NodeOrToken::Node(child)
                    if semicolons == 1 && !is_statement(child.kind()) =>
                {
                    condition = Some(child);
                }
                _ => {}
            }
        }
        // A LOCAL_VAR_DECL init is a statement kind and never mistaken for the condition.
        condition.map_or(Condition::ConstantTrue, |cond| self.condition(&cond))
    }

    fn do_while(&mut self, node: &SyntaxNode) -> Flow {
        self.push_loop(node);
        let body_flow = node
            .children()
            .find(|child| is_statement(child.kind()))
            .map_or(Flow::Completes, |body| self.statement(&body));
        let frame = self.frames.pop();
        let (broken, continued) = frame.map_or((false, false), |f| (f.broken, f.continued));
        let condition = node
            .children()
            .filter(|child| !is_statement(child.kind()))
            .last()
            .map_or(Condition::Unknown, |cond| self.condition(&cond));
        let condition_reachable = body_flow != Flow::Stops || continued;
        let by_condition = if !condition_reachable && body_flow == Flow::Stops {
            Flow::Stops
        } else {
            match condition {
                Condition::NotConstant => Flow::Completes,
                Condition::ConstantTrue => Flow::Stops,
                Condition::Unknown => Flow::Unknown,
            }
        };
        if broken {
            Flow::Completes
        } else if body_flow == Flow::Unknown && by_condition == Flow::Stops {
            Flow::Unknown
        } else {
            by_condition
        }
    }

    fn labeled(&mut self, node: &SyntaxNode) -> Flow {
        let Some(body) = node.children().find(|child| is_statement(child.kind())) else {
            return Flow::Completes;
        };
        // A labeled loop registers the label on its own frame.
        if matches!(
            body.kind(),
            SyntaxKind::WHILE_STMT
                | SyntaxKind::FOR_STMT
                | SyntaxKind::FOREACH_STMT
                | SyntaxKind::DO_WHILE_STMT
        ) {
            return self.statement(&body);
        }
        self.frames.push(Frame {
            label: label_name(node),
            is_loop: false,
            is_switch: false,
            broken: false,
            continued: false,
        });
        let flow = self.statement(&body);
        let frame = self.frames.pop();
        if frame.is_some_and(|frame| frame.broken) {
            Flow::Completes
        } else {
            flow
        }
    }

    fn try_statement(&mut self, node: &SyntaxNode) -> Flow {
        let mut flow = Flow::Stops;
        let mut finally_flow = None;
        let mut saw_body = false;
        for child in node.children() {
            match child.kind() {
                SyntaxKind::BLOCK_STMT if !saw_body => {
                    saw_body = true;
                    flow = self.block(&child);
                }
                SyntaxKind::CATCH_CLAUSE => {
                    if let Some(block) = body_block(&child) {
                        flow = flow.either(self.block(&block));
                    }
                }
                SyntaxKind::FINALLY_CLAUSE => {
                    if let Some(block) = body_block(&child) {
                        finally_flow = Some(self.block(&block));
                    }
                }
                _ => {}
            }
        }
        match finally_flow {
            Some(Flow::Stops) => Flow::Stops,
            Some(Flow::Unknown) if flow == Flow::Completes => Flow::Unknown,
            _ => flow,
        }
    }

    fn switch_statement(&mut self, node: &SyntaxNode) -> Flow {
        let clauses: Vec<SyntaxNode> = node
            .children()
            .filter(|child| child.kind() == SyntaxKind::CASE_CLAUSE)
            .collect();
        let has_default = clauses.iter().any(|clause| {
            clause
                .children_with_tokens()
                .any(|element| element.kind() == SyntaxKind::DEFAULT_KW)
        });
        // Pattern and `case null` labels make the switch exhaustive-by-rule.
        let enhanced = clauses.iter().any(|clause| {
            clause.descendants().any(|descendant| {
                matches!(
                    descendant.kind(),
                    SyntaxKind::TYPE_PATTERN | SyntaxKind::RECORD_PATTERN
                )
            }) || clause
                .children()
                .any(|child| child.kind() == SyntaxKind::LITERAL_EXPR && child.text() == "null")
        });
        self.frames.push(Frame {
            label: None,
            is_loop: false,
            is_switch: true,
            broken: false,
            continued: false,
        });
        let mut result = Flow::Stops;
        let mut last_group = Flow::Completes;
        for clause in &clauses {
            let arrow = clause
                .children_with_tokens()
                .any(|element| element.kind() == SyntaxKind::ARROW);
            let flow = if arrow {
                // `case X -> expr;` completes; a block or throw decides.
                let body = clause.children().find(|child| {
                    matches!(
                        child.kind(),
                        SyntaxKind::BLOCK_STMT | SyntaxKind::THROW_STMT
                    )
                });
                body.map_or(Flow::Completes, |body| self.statement(&body))
            } else {
                let statements = clause
                    .children()
                    .filter(|child| is_statement(child.kind()))
                    .collect::<Vec<_>>();
                if statements.is_empty() {
                    Flow::Completes
                } else {
                    self.sequence(statements.into_iter())
                }
            };
            if arrow {
                result = result.either(flow);
            }
            last_group = flow;
        }
        let frame = self.frames.pop();
        let broken = frame.is_some_and(|frame| frame.broken);
        let old_style_completes = clauses
            .iter()
            .all(|clause| {
                !clause
                    .children_with_tokens()
                    .any(|element| element.kind() == SyntaxKind::ARROW)
            })
            .then_some(last_group);
        let flow = old_style_completes.unwrap_or(result);
        if clauses.is_empty() || broken {
            return Flow::Completes;
        }
        if !has_default {
            return if enhanced {
                Flow::Unknown
            } else {
                Flow::Completes
            };
        }
        flow
    }

    /// Classify a loop condition.
    fn condition(&self, node: &SyntaxNode) -> Condition {
        match node.kind() {
            SyntaxKind::LITERAL_EXPR => {
                if node.text() == "true" {
                    Condition::ConstantTrue
                } else {
                    Condition::Unknown
                }
            }
            SyntaxKind::PAREN_EXPR => node
                .children()
                .next()
                .map_or(Condition::Unknown, |inner| self.condition(&inner)),
            SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
            | SyntaxKind::INSTANCEOF_EXPR
            | SyntaxKind::PATTERN_INSTANCEOF_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::LAMBDA_EXPR => Condition::NotConstant,
            SyntaxKind::NAME_EXPR => self.name_condition(node),
            SyntaxKind::FIELD_ACCESS_EXPR => Condition::Unknown,
            SyntaxKind::UNARY_EXPR | SyntaxKind::BINARY_EXPR | SyntaxKind::TERNARY_EXPR => {
                let mut combined = Condition::Unknown;
                let mut any_not_constant = false;
                for child in node.children() {
                    match self.condition(&child) {
                        Condition::NotConstant => any_not_constant = true,
                        Condition::ConstantTrue | Condition::Unknown => {}
                    }
                    combined = Condition::Unknown;
                }
                if any_not_constant {
                    Condition::NotConstant
                } else {
                    combined
                }
            }
            _ => Condition::Unknown,
        }
    }

    /// A simple name is a constant only when it is a `final` variable with a
    /// constant initializer; parameters and non-final variables never are.
    ///
    /// Finality is read from the declaration in this file's syntax tree: the
    /// modifier text on resolved names is not reliable for same-file symbols.
    fn name_condition(&self, node: &SyntaxNode) -> Condition {
        let Some(ident) = node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|token| token.kind().is_identifier())
        else {
            return Condition::Unknown;
        };
        let (name, location) = match self.file_semantics.resolve_at(ident.text_range()) {
            Some(ResolvedName::LocalVariable {
                is_parameter: true, ..
            }) => return Condition::NotConstant,
            Some(
                ResolvedName::LocalVariable {
                    name,
                    location: Some(location),
                    ..
                }
                | ResolvedName::Field {
                    name,
                    location: Some(location),
                    ..
                },
            ) => (name, location),
            _ => return Condition::Unknown,
        };
        let offset = rowan::TextSize::from(location.range.start);
        let Some(token) = self.root.token_at_offset(offset).right_biased() else {
            return Condition::Unknown;
        };
        // The location must point at the declaring identifier of this file.
        if token.text() != name {
            return Condition::Unknown;
        }
        let declaration = token.parent_ancestors().find(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::LOCAL_VAR_DECL | SyntaxKind::FIELD_DECL | SyntaxKind::PARAM
            )
        });
        match declaration {
            Some(declaration) if declaration.kind() == SyntaxKind::PARAM => Condition::NotConstant,
            // Interface fields are implicitly `final`.
            Some(declaration)
                if declaration.kind() == SyntaxKind::FIELD_DECL
                    && declaration
                        .ancestors()
                        .skip(1)
                        .find(|ancestor| {
                            matches!(
                                ancestor.kind(),
                                SyntaxKind::CLASS_DECL
                                    | SyntaxKind::INTERFACE_DECL
                                    | SyntaxKind::ENUM_DECL
                                    | SyntaxKind::RECORD_DECL
                            )
                        })
                        .is_some_and(|owner| owner.kind() == SyntaxKind::INTERFACE_DECL) =>
            {
                Condition::Unknown
            }
            Some(declaration) if !declares_final(&declaration) => Condition::NotConstant,
            _ => Condition::Unknown,
        }
    }
}

fn declares_final(declaration: &SyntaxNode) -> bool {
    declaration_modifiers(declaration).is_final()
        || declaration
            .children_with_tokens()
            .any(|element| element.kind() == SyntaxKind::FINAL_KW)
}

fn label_name(labeled: &SyntaxNode) -> Option<String> {
    labeled
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind().is_identifier())
        .map(|token| token.text().to_owned())
}

/// The first line's worth of a statement, so a long unreachable block is
/// underlined at its start rather than as a whole.
fn statement_head_range(statement: &SyntaxNode) -> TextRange {
    let range = statement.text_range();
    let start = statement
        .first_token()
        .map_or(range.start(), |token| token.text_range().start());
    let end = statement
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| matches!(token.kind(), SyntaxKind::SEMICOLON | SyntaxKind::L_BRACE))
        .map_or(range.end(), |token| token.text_range().end());
    TextRange::new(start, end.min(range.end()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::FileId;
    use crate::semantic_walker::compute_file_semantics;
    use hyperion_parser::{lexer::Lexer, parser::Parser};
    use hyperion_workspace::WorkspaceIndex;

    fn flow_errors(source: &str) -> Vec<String> {
        let tokens: Vec<_> = Lexer::new(source).collect();
        let (green, parse_errors) = Parser::parse(tokens);
        assert!(parse_errors.is_empty(), "parse errors: {parse_errors:?}");
        let root = SyntaxNode::new_root(green);
        let temp = tempfile::TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let path = temp.path().join("Main.java");
        index.index_file_with_source(&path, source).unwrap();
        let file_id = index.get_file_id(&path).unwrap_or(FileId(0));
        let resolver = crate::resolver::ResolverBuilder::with_workspace(
            file_id,
            std::sync::Arc::new(index.clone()),
        )
        .build_from_syntax(&root);
        let semantics = compute_file_semantics(&root, file_id, resolver.symbol_table(), &index);
        check_flow(&root, &semantics)
            .into_iter()
            .map(|error| error.message())
            .collect()
    }

    #[test]
    fn a_constant_final_field_condition_is_undecided() {
        let found = flow_errors(
            "class Main { static final boolean DEBUG = true; int f() { while (DEBUG) { } } }",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_mutable_field_condition_can_complete() {
        let found = flow_errors("class Main { boolean flag; int f() { while (flag) { } } }");
        assert_eq!(found, vec!["missing return statement".to_owned()]);
    }
}
