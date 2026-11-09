// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! The class a method call's method belongs to, for the call edges of the summary generator
//! and the fixpoint.
//!
//! A qualified call names its class through its qualifier. A variable names the class it is
//! declared with, so `dao.find(id)` with `UserDao dao` targets `UserDao.find`. A name with no
//! variable in scope is taken as a class, as in `Helper.run()`. Each written class name is
//! resolved the way Java resolves it in its file, against the classes the workspace indexes.

use hyperion_parser::{SyntaxKind, SyntaxNode};
use hyperion_workspace::WorkspaceIndex;

/// How a source file's written class names resolve to workspace classes: the enclosing class's
/// nested classes, then single-type imports, the file's package, and on-demand imports.
pub(crate) struct TypeScope<'a> {
    workspace: &'a WorkspaceIndex,
    package: Option<String>,
    /// Imports as written, `a.b.C` or `a.b.*`. Static imports name members, not types.
    imports: Vec<String>,
}

impl<'a> TypeScope<'a> {
    /// The scope of the source file whose syntax tree contains `node`.
    pub(crate) fn for_node(workspace: &'a WorkspaceIndex, node: &SyntaxNode) -> Self {
        let root = node.ancestors().last().unwrap_or_else(|| node.clone());
        Self {
            workspace,
            package: package_name(&root),
            imports: import_names(&root),
        }
    }

    /// The FQN of the workspace class a written type names, if there is one.
    fn resolve(&self, written: &str, class_fqn: &str) -> Option<String> {
        let known = |fqn: String| self.workspace.has_class(&fqn).then_some(fqn);
        if written.contains('.') {
            return known(written.to_string());
        }
        known(format!("{class_fqn}.{written}"))
            .or_else(|| {
                self.imports
                    .iter()
                    .filter(|import| import.rsplit('.').next() == Some(written))
                    .find_map(|import| known(import.clone()))
            })
            .or_else(|| {
                let in_package = self.package.as_ref().map_or_else(
                    || written.to_string(),
                    |package| format!("{package}.{written}"),
                );
                known(in_package)
            })
            .or_else(|| {
                self.imports
                    .iter()
                    .filter_map(|import| import.strip_suffix(".*"))
                    .find_map(|package| known(format!("{package}.{written}")))
            })
    }
}

/// The FQN of the class a call's method belongs to. An unqualified call and `this.m()` name the
/// enclosing class, `class_fqn`. `None` when the qualifier does not name a workspace class.
pub(crate) fn call_target_class(
    call: &SyntaxNode,
    class_fqn: &str,
    scope: &TypeScope<'_>,
) -> Option<String> {
    let qualified = call
        .children_with_tokens()
        .any(|child| child.kind() == SyntaxKind::DOT);
    if !qualified {
        return Some(class_fqn.to_string());
    }
    let qualifier = call.children().next()?;
    match qualifier.kind() {
        SyntaxKind::THIS_EXPR => Some(class_fqn.to_string()),
        SyntaxKind::NAME_EXPR => {
            let name = last_identifier(&qualifier)?;
            let written = match nearest_declaration(call, &name) {
                Some(declaration) => declared_class_type(&declaration)?,
                None => name,
            };
            scope.resolve(&written, class_fqn)
        }
        // `this.field.m()`: the field's declared type in the enclosing class.
        SyntaxKind::FIELD_ACCESS_EXPR
            if qualifier
                .children()
                .any(|child| child.kind() == SyntaxKind::THIS_EXPR) =>
        {
            let name = last_identifier(&qualifier)?;
            let declaration = field_declaration(call, &name)?;
            scope.resolve(&declared_class_type(&declaration)?, class_fqn)
        }
        // `super.m()`, and qualifiers that are expressions or longer chains, are not modelled.
        _ => None,
    }
}

/// The declaration that gives `name` its meaning at `call`: the closest parameter or local
/// declared before the call and still in scope there, else a field of the enclosing class.
fn nearest_declaration(call: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    let method = call.ancestors().find(|node| {
        matches!(
            node.kind(),
            SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL
        )
    });
    let local = method.and_then(|method| {
        method
            .descendants()
            .filter(|node| matches!(node.kind(), SyntaxKind::PARAM | SyntaxKind::LOCAL_VAR_DECL))
            .filter(|node| declared_name(node).as_deref() == Some(name))
            .filter(|node| node.text_range().start() < call.text_range().start())
            .filter(|node| node.kind() == SyntaxKind::PARAM || in_block_of(node, call))
            .last()
    });
    local.or_else(|| field_declaration(call, name))
}

/// Whether a local's enclosing block contains the call, so the local is in scope there.
fn in_block_of(declaration: &SyntaxNode, call: &SyntaxNode) -> bool {
    declaration
        .parent()
        .is_some_and(|block| call.ancestors().any(|ancestor| ancestor == block))
}

/// The field of the enclosing class that is declared as `name`.
fn field_declaration(call: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    let class = call.ancestors().find(|node| {
        matches!(
            node.kind(),
            SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::ENUM_DECL
        )
    })?;
    class.children().find(|node| {
        node.kind() == SyntaxKind::FIELD_DECL && declared_name(node).as_deref() == Some(name)
    })
}

/// The name a parameter, local or field declares: its first identifier, a direct child.
fn declared_name(declaration: &SyntaxNode) -> Option<String> {
    declaration
        .children_with_tokens()
        .filter_map(|child| child.into_token())
        .find(|token| token.kind() == SyntaxKind::IDENT)
        .map(|token| token.text().to_string())
}

/// The last identifier directly inside a node: a `NAME_EXPR`'s name, or a field access's member.
fn last_identifier(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(|child| child.into_token())
        .filter(|token| token.kind() == SyntaxKind::IDENT)
        .last()
        .map(|token| token.text().to_string())
}

/// The class a declaration's written type names, without type arguments. A `var`, a primitive
/// or an array names no class.
fn declared_class_type(declaration: &SyntaxNode) -> Option<String> {
    let ty = declaration
        .children()
        .find(|node| node.kind() == SyntaxKind::CLASS_TYPE)?;
    let name = ty
        .children()
        .find(|node| node.kind() == SyntaxKind::QUALIFIED_NAME)
        .unwrap_or_else(|| ty.clone());
    let parts: Vec<String> = name
        .children_with_tokens()
        .filter_map(|child| child.into_token())
        .filter(|token| token.kind() == SyntaxKind::IDENT)
        .map(|token| token.text().to_string())
        .collect();
    (!parts.is_empty()).then(|| parts.join("."))
}

/// The package a source file declares, as `a.b`.
fn package_name(root: &SyntaxNode) -> Option<String> {
    let decl = root
        .children()
        .find(|child| child.kind() == SyntaxKind::PACKAGE_DECL)?;
    let text = decl.text().to_string();
    let name = text
        .trim()
        .strip_prefix("package")?
        .trim()
        .trim_end_matches(';');
    let name: String = name.split_whitespace().collect();
    (!name.is_empty()).then_some(name)
}

/// The imports of a source file, as `a.b.C` or `a.b.*`.
fn import_names(root: &SyntaxNode) -> Vec<String> {
    root.children()
        .filter(|child| child.kind() == SyntaxKind::IMPORT_DECL)
        .filter_map(|decl| {
            let text = decl.text().to_string();
            let body = text
                .trim()
                .strip_prefix("import")?
                .trim()
                .trim_end_matches(';');
            (!body.starts_with("static")).then(|| body.split_whitespace().collect::<String>())
        })
        .collect()
}
