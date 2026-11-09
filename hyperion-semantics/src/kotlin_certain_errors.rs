// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin errors that are *certain*.
//!
//! A false error on valid code is worse than a missed error, so each check here
//! fires only when every input it needs is known and nothing that could change
//! the verdict (an unindexed supertype, an overload, a lambda receiver, a named
//! argument, a same-named extension) is in play:
//!
//! * a literal or constructor call initialising a declared type it cannot be;
//! * a call to the only function of that name, with a provably wrong number of
//!   positional arguments;
//! * a member that exists nowhere in the workspace, the JDK or the standard
//!   library, selected on a value of a fully indexed Kotlin class.

use crate::db::FileId;
use crate::semantic_error::{SemanticError, SemanticErrorKind};
use crate::symbol::{SymbolTable, Type};
use crate::{KotlinTypeInferenceEngine, SymbolResolver};
use hyperion_kotlin::{SyntaxKind as Kt, SyntaxNode, SyntaxToken};
use hyperion_workspace::{DeclarationNamespace, SymbolTarget, TypeNameResolution, WorkspaceIndex};
use lsp_types::SymbolKind;
use rowan::NodeOrToken;
use std::collections::HashSet;
use std::sync::Arc;

const BUILTIN_TYPES: &[&str] = &[
    "String", "Int", "Long", "Short", "Byte", "Double", "Float", "Boolean", "Char",
];

/// Run every certain-error check over one Kotlin file.
pub fn check(
    root: &SyntaxNode,
    file_id: FileId,
    index: &Arc<WorkspaceIndex>,
    symbol_table: &Arc<SymbolTable>,
) -> Vec<SemanticError> {
    let mut checker = Checker {
        file_id,
        index,
        errors: Vec::new(),
        declared_here: DeclaredHere::collect(root),
        engine: None,
        resolver: None,
        symbol_table,
    };
    for node in root.descendants() {
        match node.kind() {
            Kt::PROPERTY_DECL => checker.check_declaration(&node),
            Kt::CALL_EXPR => checker.check_call_arity(&node),
            Kt::MEMBER_ACCESS_EXPR | Kt::SAFE_ACCESS_EXPR => checker.check_member(&node),
            _ => {}
        }
    }
    checker.errors
}

/// Names declared anywhere in the file by something other than a top-level or
/// member function, so a call `f(..)` may be a call of a value.
struct DeclaredHere {
    values: HashSet<String>,
    functions: Vec<String>,
}

impl DeclaredHere {
    fn collect(root: &SyntaxNode) -> Self {
        let mut values = HashSet::new();
        let mut functions = Vec::new();
        for node in root.descendants() {
            match node.kind() {
                Kt::PROPERTY_DECL
                | Kt::PARAMETER
                | Kt::LAMBDA_PARAMETERS
                | Kt::FOR_LOOP_VAR
                | Kt::DESTRUCTURING_DECL => {
                    for token in node
                        .children_with_tokens()
                        .filter_map(NodeOrToken::into_token)
                    {
                        if matches!(token.kind(), Kt::EQ | Kt::COLON | Kt::IN_KW) {
                            break;
                        }
                        if matches!(token.kind(), Kt::IDENT | Kt::BACKTICK_IDENT) {
                            values.insert(token.text().trim_matches('`').to_string());
                        }
                    }
                }
                Kt::FUNCTION_DECL => {
                    if let Some(name) = function_name(&node) {
                        functions.push(name);
                    }
                }
                _ => {}
            }
        }
        Self { values, functions }
    }
}

struct Checker<'a> {
    file_id: FileId,
    index: &'a Arc<WorkspaceIndex>,
    errors: Vec<SemanticError>,
    declared_here: DeclaredHere,
    engine: Option<KotlinTypeInferenceEngine>,
    resolver: Option<SymbolResolver>,
    symbol_table: &'a Arc<SymbolTable>,
}

/// `Some(true/false)` when `sub`'s whole hierarchy is indexed and does
/// (not) reach `sup`; `None` when some supertype cannot be resolved.
pub(crate) fn indexed_subtype(index: &WorkspaceIndex, sub: &str, sup: &str) -> Option<bool> {
    let mut queue = vec![sub.to_string()];
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(next) = queue.pop() {
        if next == sup {
            return Some(true);
        }
        if !seen.insert(next.clone()) || seen.len() > 128 {
            continue;
        }
        let symbol = index.get_symbol(&next)?;
        let written = usize::from(symbol.parent_class.is_some()) + symbol.interfaces.len();
        let edges = index.direct_supertypes(&next);
        if edges.len() != written {
            return None;
        }
        queue.extend(edges);
    }
    Some(false)
}

fn function_name(node: &SyntaxNode) -> Option<String> {
    let mut receiver_seen = false;
    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Node(child) if child.kind() == Kt::TYPE_REF => receiver_seen = true,
            NodeOrToken::Token(token) => {
                if token.kind() == Kt::DOT || token.kind() == Kt::SAFE_CALL {
                    receiver_seen = true;
                }
                if matches!(token.kind(), Kt::IDENT | Kt::BACKTICK_IDENT) {
                    return Some(token.text().trim_matches('`').to_string());
                }
                if token.kind() == Kt::L_PAREN {
                    return None;
                }
            }
            NodeOrToken::Node(_) => {}
        }
    }
    let _ = receiver_seen;
    None
}

/// Literal kinds relevant to declaration checks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Literal {
    Int,
    Long,
    Double,
    Float,
    Str,
    Bool,
    Char,
    Null,
}

impl Literal {
    const fn kotlin_type(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Long => "Long",
            Self::Double => "Double",
            Self::Float => "Float",
            Self::Str => "String",
            Self::Bool => "Boolean",
            Self::Char => "Char",
            Self::Null => "Nothing?",
        }
    }
}

fn literal_of(node: &SyntaxNode) -> Option<Literal> {
    if node.kind() != Kt::LITERAL_EXPR {
        return None;
    }
    let token = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| !token.kind().is_trivia())?;
    Some(match token.kind() {
        Kt::INT_LITERAL => Literal::Int,
        Kt::LONG_LITERAL => Literal::Long,
        Kt::DOUBLE_LITERAL => Literal::Double,
        Kt::FLOAT_LITERAL => Literal::Float,
        Kt::STRING_LITERAL | Kt::RAW_STRING_LITERAL => {
            // A template is still a String, but an unterminated/odd one is not
            // worth an opinion.
            Literal::Str
        }
        Kt::TRUE_KW | Kt::FALSE_KW => Literal::Bool,
        Kt::CHAR_LITERAL => Literal::Char,
        Kt::NULL_KW => Literal::Null,
        _ => return None,
    })
}

/// Whether `literal` can initialise a declared non-null builtin `declared`.
fn literal_fits(declared: &str, literal: Literal) -> bool {
    match declared {
        "String" => literal == Literal::Str,
        "Boolean" => literal == Literal::Bool,
        "Char" => literal == Literal::Char,
        // An integer literal adapts to Long/Short/Byte, not to Double/Float.
        "Int" => literal == Literal::Int,
        "Long" | "Short" | "Byte" => {
            literal == Literal::Int || (declared == "Long" && literal == Literal::Long)
        }
        "Double" => literal == Literal::Double,
        "Float" => literal == Literal::Float,
        _ => true,
    }
}

impl Checker<'_> {
    fn range_of(node: &SyntaxNode) -> rowan::TextRange {
        let start = node
            .descendants_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| !token.kind().is_trivia())
            .map_or_else(
                || node.text_range().start(),
                |token| token.text_range().start(),
            );
        let end = node
            .descendants_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .filter(|token| !token.kind().is_trivia())
            .last()
            .map_or_else(|| node.text_range().end(), |token| token.text_range().end());
        rowan::TextRange::new(start, end)
    }

    // ------------------------------------------------------------ declarations

    fn check_declaration(&mut self, node: &SyntaxNode) {
        let mut declared: Option<String> = None;
        let mut initializer: Option<NodeOrToken<SyntaxNode, SyntaxToken>> = None;
        let mut seen_eq = false;
        for element in node.children_with_tokens() {
            match &element {
                NodeOrToken::Node(child) if !seen_eq && child.kind() == Kt::TYPE_REF => {
                    declared = Some(
                        child
                            .descendants_with_tokens()
                            .filter_map(NodeOrToken::into_token)
                            .filter(|token| !token.kind().is_trivia())
                            .map(|token| token.text().to_string())
                            .collect::<String>(),
                    );
                }
                NodeOrToken::Token(token) if token.kind() == Kt::EQ => seen_eq = true,
                NodeOrToken::Token(token) if token.kind() == Kt::BY_KW => return,
                NodeOrToken::Node(child) if seen_eq && child.kind() == Kt::PROPERTY_ACCESSOR => {
                    break
                }
                _ if seen_eq && initializer.is_none() && !element.kind().is_trivia() => {
                    initializer = Some(element.clone());
                }
                _ => {}
            }
        }
        let (Some(declared), Some(NodeOrToken::Node(initializer))) = (declared, initializer) else {
            return;
        };
        if declared.contains(['<', '(', '-']) || declared.contains('.') {
            return;
        }
        let nullable = declared.ends_with('?');
        let base = declared.trim_end_matches('?').to_string();
        let is_builtin = BUILTIN_TYPES.contains(&base.as_str());

        if let Some(literal) = literal_of(&initializer) {
            if literal == Literal::Null {
                if !nullable && (is_builtin || self.source_class(&base).is_some()) {
                    self.errors.push(SemanticError::new(
                        Self::range_of(&initializer),
                        SemanticErrorKind::KotlinTypeMismatch {
                            message: format!("Null cannot be a value of a non-null type '{base}'."),
                        },
                    ));
                }
                return;
            }
            if is_builtin && !literal_fits(&base, literal) {
                self.errors.push(SemanticError::new(
                    Self::range_of(&initializer),
                    SemanticErrorKind::KotlinTypeMismatch {
                        message: format!(
                            "Initializer type mismatch: expected '{declared}', actual '{}'.",
                            literal.kotlin_type()
                        ),
                    },
                ));
            }
            return;
        }

        // `val c: String = Circle(4.0)`
        if initializer.kind() == Kt::CALL_EXPR {
            if let Some(constructed) = self.constructed_class(&initializer) {
                let mismatch = if is_builtin {
                    true
                } else if let Some(expected) = self.source_class(&base) {
                    match self.is_subtype(&constructed.fqn, &expected) {
                        Some(subtype) => !subtype,
                        None => false,
                    }
                } else {
                    false
                };
                if mismatch {
                    self.errors.push(SemanticError::new(
                        Self::range_of(&initializer),
                        SemanticErrorKind::KotlinTypeMismatch {
                            message: format!(
                                "Initializer type mismatch: expected '{declared}', actual '{}'.",
                                constructed.name
                            ),
                        },
                    ));
                }
            }
        }
    }

    /// A workspace-declared class (not interface, object or annotation) that a
    /// call `Name(..)` constructs, with nothing else callable under that name.
    fn constructed_class(&self, call: &SyntaxNode) -> Option<ConstructedClass> {
        let name = callee_identifier(call)?;
        if !name.chars().next().is_some_and(char::is_uppercase) {
            return None;
        }
        // Not a same-named function or value (`fun Circle()`, factory functions).
        if self.declared_here.values.contains(&name) || self.declared_here.functions.contains(&name)
        {
            return None;
        }
        let fqn = self.source_class(&name)?;
        let symbol = self.index.get_symbol(&fqn)?;
        if symbol.kind != SymbolKind::CLASS {
            return None;
        }
        let only_types = self
            .index
            .find_by_simple_name(&name)
            .iter()
            .all(|location| {
                matches!(
                    location.kind,
                    SymbolKind::CLASS
                        | SymbolKind::INTERFACE
                        | SymbolKind::ENUM
                        | SymbolKind::OBJECT
                        | SymbolKind::CONSTRUCTOR
                )
            });
        only_types.then_some(ConstructedClass { fqn, name })
    }

    /// FQN of the *source* class a written name resolves to.
    fn source_class(&self, written: &str) -> Option<String> {
        let TypeNameResolution::Exact(fqn) =
            self.index.resolve_type_name(self.file_id, &[], written)
        else {
            return None;
        };
        let location = self.index.find_symbol(&fqn)?;
        if self.index.is_external_file(location.file_id) {
            return None;
        }
        let symbol = self.index.get_symbol(&fqn)?;
        matches!(
            symbol.kind,
            SymbolKind::CLASS | SymbolKind::INTERFACE | SymbolKind::ENUM
        )
        .then_some(fqn)
    }

    /// [`indexed_subtype`] in this checker's index.
    fn is_subtype(&self, sub: &str, sup: &str) -> Option<bool> {
        indexed_subtype(self.index, sub, sup)
    }

    // --------------------------------------------------------------- arguments

    fn check_call_arity(&mut self, call: &SyntaxNode) {
        let Some(name) = callee_identifier(call) else {
            return;
        };
        // Named arguments, spread, defaults interplay: only plain positional calls.
        let shape = call_shape(call);
        if shape.named || shape.spread || shape.unbalanced {
            return;
        }
        if self.declared_here.values.contains(&name) {
            return;
        }
        if hyperion_kotlin::is_kotlin_stdlib_name(&name) {
            return;
        }
        // An explicit import names the function the call binds to, which need
        // not be the one declaration of that name we can see; a wildcard import
        // can bring overloads we cannot enumerate.
        if let Some(imports) = self.index.get_imports(self.file_id) {
            if imports.explicit.contains_key(&name) || !imports.wildcards.is_empty() {
                return;
            }
        }
        // Implicit receivers (lambdas with receiver, supertypes we cannot see,
        // extension bodies) can contribute overloads.
        if Self::has_implicit_receiver_context(call) {
            return;
        }

        let candidates: Vec<_> = self
            .index
            .find_by_simple_name(&name)
            .into_iter()
            .filter_map(|location| self.index.get_symbol(&location.fqn).map(|s| (location, s)))
            .filter(|(_, symbol)| {
                symbol
                    .kotlin
                    .as_ref()
                    .is_none_or(|kotlin| kotlin.jvm_origin.is_none())
            })
            .collect();
        let [(location, symbol)] = candidates.as_slice() else {
            return;
        };
        // The by-name index keeps one entry per FQN, and Kotlin overloads share
        // an FQN: count the real declarations.
        if self.index.declaration_count(&symbol.fqn) != 1 {
            return;
        }
        let is_function = symbol.kind == SymbolKind::FUNCTION && symbol.receiver_type.is_none();
        let is_class = symbol.kind == SymbolKind::CLASS;
        if !is_function && !is_class {
            return;
        }
        if self.index.is_external_file(location.file_id) {
            return;
        }
        // The one declaration; a local function of the same name in this file
        // (unindexed) would be a second candidate.
        let declared_locally = self
            .declared_here
            .functions
            .iter()
            .filter(|f| **f == name)
            .count();
        let target_is_here = location.file_id == self.file_id;
        if declared_locally > usize::from(target_is_here) {
            return;
        }

        let Some(signature) = self.declaration_signature(symbol, location.file_id, is_class) else {
            return;
        };
        if signature.has_vararg || signature.has_secondary_constructor {
            return;
        }
        let supplied = shape.positional + usize::from(shape.trailing_lambda);
        let required = signature
            .parameters
            .iter()
            .filter(|parameter| !parameter.has_default)
            .count();
        let total = signature.parameters.len();
        let rendered = format!(
            "{name}({})",
            signature
                .parameters
                .iter()
                .map(|parameter| parameter.rendered.clone())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let message = if supplied > total {
            format!(
                "Too many arguments for '{rendered}': expected at most {total}, found {supplied}."
            )
        } else if supplied < required {
            let missing = signature
                .parameters
                .iter()
                .filter(|parameter| !parameter.has_default)
                .nth(supplied)
                .map_or_else(String::new, |parameter| parameter.name.clone());
            format!("No value passed for parameter '{missing}' of '{rendered}'.")
        } else {
            return;
        };
        self.errors.push(SemanticError::new(
            Self::range_of(call),
            SemanticErrorKind::KotlinArgumentCount { message },
        ));
    }

    fn has_implicit_receiver_context(node: &SyntaxNode) -> bool {
        node.ancestors().any(|ancestor| match ancestor.kind() {
            Kt::LAMBDA_EXPR => true,
            Kt::FUNCTION_DECL | Kt::PROPERTY_DECL => ancestor
                .children_with_tokens()
                .take_while(|element| {
                    !matches!(element, NodeOrToken::Token(token) if token.kind() == Kt::L_PAREN)
                })
                .any(|element| {
                    matches!(&element, NodeOrToken::Token(token) if matches!(token.kind(), Kt::DOT | Kt::SAFE_CALL))
                        || matches!(&element, NodeOrToken::Node(child) if child.kind() == Kt::TYPE_REF)
                            && ancestor
                                .children_with_tokens()
                                .any(|other| matches!(&other, NodeOrToken::Token(t) if t.kind() == Kt::DOT))
                }),
            Kt::CLASS_DECL | Kt::OBJECT_DECL | Kt::INTERFACE_DECL => ancestor
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .any(|token| token.kind() == Kt::COLON),
            _ => false,
        })
    }

    /// Parameters of the declaration `symbol` came from, read from its source.
    fn declaration_signature(
        &self,
        symbol: &hyperion_workspace::Symbol,
        file_id: FileId,
        is_class: bool,
    ) -> Option<Signature> {
        let revision = self.index.current_file_revision(file_id)?;
        let indexed = self.index.get_indexed_revision_content(revision)?;
        let parsed = hyperion_kotlin::parse(indexed.content.as_str());
        let root = SyntaxNode::new_root(parsed.green_node);
        let start = rowan::TextSize::from(symbol.range.start);
        let name_token = root.token_at_offset(start).right_biased()?;
        let declaration = name_token.parent_ancestors().find(|ancestor| {
            if is_class {
                ancestor.kind() == Kt::CLASS_DECL
            } else {
                ancestor.kind() == Kt::FUNCTION_DECL
            }
        })?;
        let list = declaration
            .children()
            .find(|child| child.kind() == Kt::PARAMETER_LIST)?;
        let mut parameters = Vec::new();
        let mut has_vararg = false;
        for parameter in list
            .children()
            .filter(|child| child.kind() == Kt::PARAMETER)
        {
            let mut name = String::new();
            let mut ty = String::new();
            let mut has_default = false;
            let mut seen_colon = false;
            for element in parameter.children_with_tokens() {
                match element {
                    NodeOrToken::Token(token) => match token.kind() {
                        Kt::VARARG_KW => has_vararg = true,
                        Kt::COLON => seen_colon = true,
                        Kt::EQ => has_default = true,
                        Kt::IDENT | Kt::BACKTICK_IDENT if !seen_colon => {
                            name = token.text().trim_matches('`').to_string();
                        }
                        _ => {}
                    },
                    NodeOrToken::Node(child)
                        if ty.is_empty()
                            && matches!(child.kind(), Kt::TYPE_REF | Kt::FUNCTION_TYPE) =>
                    {
                        ty = child
                            .descendants_with_tokens()
                            .filter_map(NodeOrToken::into_token)
                            .filter(|t| !t.kind().is_trivia())
                            .map(|t| t.text().to_string())
                            .collect::<String>();
                    }
                    NodeOrToken::Node(_) => {}
                }
            }
            parameters.push(Parameter {
                rendered: format!("{name}: {ty}"),
                name,
                has_default,
            });
        }
        let has_secondary_constructor = is_class
            && declaration
                .descendants()
                .any(|node| node.kind() == Kt::CONSTRUCTOR_DECL);
        // A class without any parameter list still has the implicit constructor.
        Some(Signature {
            parameters,
            has_vararg,
            has_secondary_constructor,
        })
    }

    // ----------------------------------------------------------------- members

    fn check_member(&mut self, access: &SyntaxNode) {
        // `variable.member` where `variable` is a plain identifier.
        let mut receiver: Option<SyntaxToken> = None;
        let mut member: Option<SyntaxToken> = None;
        let mut seen_separator = false;
        for element in access.children_with_tokens() {
            match element {
                NodeOrToken::Node(_) if !seen_separator => return,
                NodeOrToken::Token(token) => match token.kind() {
                    Kt::DOT | Kt::SAFE_CALL => seen_separator = true,
                    Kt::IDENT if !seen_separator && receiver.is_none() => receiver = Some(token),
                    Kt::IDENT if seen_separator && member.is_none() => member = Some(token),
                    kind if !kind.is_trivia() && member.is_none() && !seen_separator => return,
                    _ => {}
                },
                NodeOrToken::Node(_) => {}
            }
        }
        let (Some(receiver), Some(member)) = (receiver, member) else {
            return;
        };
        let name = member.text();
        // Cheap, decisive filters first: a name that exists nowhere.
        if hyperion_kotlin::is_kotlin_stdlib_name(name)
            || !self.index.find_by_simple_name(name).is_empty()
        {
            return;
        }
        let (getter, setter) = hyperion_types::jvm_names::accessor_names(name);
        if !self.index.find_by_simple_name(&getter).is_empty()
            || !self.index.find_by_simple_name(&setter).is_empty()
        {
            return;
        }
        // The receiver must be a value of a fully indexed Kotlin-source class.
        let head = receiver.text();
        if head.chars().next().is_some_and(char::is_uppercase)
            || self.declared_here.functions.iter().any(|f| f == head)
        {
            return;
        }
        let engine = self.engine.get_or_insert_with(|| {
            KotlinTypeInferenceEngine::new(Arc::clone(self.index), self.file_id)
        });
        let resolver = self.resolver.get_or_insert_with(|| {
            SymbolResolver::with_cached_table(Arc::clone(self.symbol_table))
        });
        let receiver_type = engine.infer_token_expression(&receiver, resolver);
        let Type::Class(class) = receiver_type else {
            return;
        };
        let fqn = if class.package.is_some() {
            let qualified = class.qualified_name().into_owned();
            if !self.index.has_class(&qualified) {
                return;
            }
            qualified
        } else {
            // Inference left the package out: resolve the written name the way
            // the file's own imports would.
            match self.source_class(&class.name) {
                Some(fqn) => fqn,
                None => return,
            }
        };
        let Some(location) = self.index.find_symbol(&fqn) else {
            return;
        };
        let is_kotlin_source = !self.index.is_external_file(location.file_id)
            && self
                .index
                .get_file_path(location.file_id)
                .is_some_and(|path| {
                    matches!(
                        path.extension().and_then(|e| e.to_str()),
                        Some("kt" | "kts")
                    )
                });
        if !is_kotlin_source {
            return;
        }
        if self.is_subtype(&fqn, "__never__") != Some(false) {
            return; // hierarchy not fully indexed
        }
        // A companion, delegation or nested type can still supply the name.
        let _ = DeclarationNamespace::Value;
        let _ = SymbolTarget::new(fqn.as_str(), DeclarationNamespace::Type);
        self.errors.push(SemanticError::new(
            member.text_range(),
            SemanticErrorKind::UnresolvedReference {
                symbol_name: name.to_string(),
            },
        ));
    }
}

struct ConstructedClass {
    fqn: String,
    name: String,
}

struct Parameter {
    name: String,
    rendered: String,
    has_default: bool,
}

struct Signature {
    parameters: Vec<Parameter>,
    has_vararg: bool,
    has_secondary_constructor: bool,
}

/// The identifier a `name(...)` / `name<T>(...)` / `name { }` call names.
fn callee_identifier(call: &SyntaxNode) -> Option<String> {
    let mut first = None;
    for element in call.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) if token.kind().is_trivia() => {}
            NodeOrToken::Token(token) => {
                if first.is_none() && matches!(token.kind(), Kt::IDENT | Kt::BACKTICK_IDENT) {
                    first = Some(token.text().trim_matches('`').to_string());
                    continue;
                }
                break;
            }
            NodeOrToken::Node(child) => {
                if first.is_some() && matches!(child.kind(), Kt::TYPE_ARGUMENT_LIST) {
                    continue;
                }
                break;
            }
        }
    }
    first
}

struct CallShape {
    positional: usize,
    named: bool,
    spread: bool,
    trailing_lambda: bool,
    unbalanced: bool,
}

fn call_shape(call: &SyntaxNode) -> CallShape {
    let mut shape = CallShape {
        positional: 0,
        named: false,
        spread: false,
        trailing_lambda: false,
        unbalanced: false,
    };
    let mut inside = false;
    let mut closed = false;
    let mut current_has_content = false;
    for element in call.children_with_tokens() {
        let kind = element.kind();
        if kind.is_trivia() {
            continue;
        }
        match kind {
            Kt::L_PAREN if !inside && !closed => inside = true,
            Kt::R_PAREN if inside => {
                if current_has_content {
                    shape.positional += 1;
                }
                inside = false;
                closed = true;
            }
            Kt::COMMA if inside => {
                if current_has_content {
                    shape.positional += 1;
                }
                current_has_content = false;
            }
            Kt::LAMBDA_EXPR if !inside => shape.trailing_lambda = true,
            _ if inside => {
                current_has_content = true;
                if let NodeOrToken::Node(node) = &element {
                    if node.kind() == Kt::BINARY_EXPR
                        && node
                            .children_with_tokens()
                            .filter(|e| !e.kind().is_trivia())
                            .nth(1)
                            .is_some_and(|second| second.kind() == Kt::EQ)
                    {
                        shape.named = true;
                    }
                    if node.kind() == Kt::UNARY_EXPR
                        && node
                            .children_with_tokens()
                            .find(|e| !e.kind().is_trivia())
                            .is_some_and(|first| first.kind() == Kt::STAR)
                    {
                        shape.spread = true;
                    }
                }
            }
            _ => {}
        }
    }
    shape.unbalanced = inside;
    shape
}
