// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! LSP adapter for canonical semantic call resolution.

use std::sync::Arc;

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::call_resolution::{
    lambda_call_may_apply, resolve_call, resolve_call_with_lambda_inference,
    resolve_projected_call_with_lambda_inference, LambdaReturnEvidence, ResolvedCall,
};
use hyperion_semantics::db::FileId;
use hyperion_semantics::resolver::ResolverBuilder;
use hyperion_semantics::symbol::Type;
use hyperion_semantics::TypeInferenceEngine;
use hyperion_workspace::{ProjectedEffectiveMethod, WorkspaceIndex};

use super::argument_inference::InferredType;

pub use hyperion_semantics::call_resolution::{Conversion, MatchScore};
pub type OverloadMatch = ResolvedCall;

/// Resolve a method call while preserving the syntax of direct lambda arguments.
pub(crate) fn resolve_overload_with_arguments(
    workspace_index: &Arc<WorkspaceIndex>,
    class_fqn: &str,
    receiver_type: Option<&Type>,
    method_name: &str,
    argument_types: &[InferredType],
    arguments: &SyntaxNode,
    file_id: FileId,
    syntax_root: &SyntaxNode,
) -> Option<OverloadMatch> {
    let types = argument_types
        .iter()
        .map(|argument| argument.ty.clone())
        .collect::<Vec<_>>();
    let lambdas = direct_lambdas(arguments);
    if lambdas.is_empty() {
        return resolve_call(
            workspace_index,
            class_fqn,
            receiver_type,
            method_name,
            &types,
        );
    }
    let engine = TypeInferenceEngine::new(Arc::clone(workspace_index), file_id);
    let positions = lambdas
        .iter()
        .map(|(position, lambda)| engine.lambda_argument(*position, lambda))
        .collect::<Vec<_>>();
    let resolver = ResolverBuilder::with_workspace(file_id, Arc::clone(workspace_index))
        .build_from_syntax(syntax_root);
    resolve_call_with_lambda_inference(
        workspace_index,
        class_fqn,
        receiver_type,
        method_name,
        &types,
        &positions,
        |position, parameter_types| {
            lambdas.iter().find(|(index, _)| *index == position).map_or(
                LambdaReturnEvidence::Incompatible,
                |(_, lambda)| {
                    engine.infer_lambda_with_parameters(lambda, &resolver, parameter_types)
                },
            )
        },
    )
}

/// Apply the same lambda typing to a visibility-filtered static-import set.
pub(crate) fn resolve_imported_overload_with_arguments(
    workspace_index: &Arc<WorkspaceIndex>,
    candidates: Vec<ProjectedEffectiveMethod>,
    argument_types: &[Type],
    arguments: &SyntaxNode,
    file_id: FileId,
    syntax_root: &SyntaxNode,
) -> Option<OverloadMatch> {
    let lambdas = direct_lambdas(arguments);
    let engine = TypeInferenceEngine::new(Arc::clone(workspace_index), file_id);
    let positions = lambdas
        .iter()
        .map(|(position, lambda)| engine.lambda_argument(*position, lambda))
        .collect::<Vec<_>>();
    let resolver = ResolverBuilder::with_workspace(file_id, Arc::clone(workspace_index))
        .build_from_syntax(syntax_root);
    resolve_projected_call_with_lambda_inference(
        workspace_index,
        candidates,
        None,
        argument_types,
        &positions,
        |position, parameter_types| {
            lambdas.iter().find(|(index, _)| *index == position).map_or(
                LambdaReturnEvidence::Incompatible,
                |(_, lambda)| {
                    engine.infer_lambda_with_parameters(lambda, &resolver, parameter_types)
                },
            )
        },
    )
}

fn direct_lambdas(arguments: &SyntaxNode) -> Vec<(usize, SyntaxNode)> {
    arguments
        .children()
        .enumerate()
        .filter_map(|(index, argument)| {
            (argument.kind() == SyntaxKind::LAMBDA_EXPR).then_some((index, argument))
        })
        .collect()
}

/// Preserve an enclosing method tier unless lambda syntax proves every
/// declaration in that tier incompatible.
pub(crate) fn enclosing_overload_may_apply_with_arguments(
    workspace_index: &Arc<WorkspaceIndex>,
    owner_fqn: &str,
    method_name: &str,
    argument_types: &[Type],
    arguments: &SyntaxNode,
    file_id: FileId,
    syntax_root: &SyntaxNode,
) -> bool {
    let lambdas = direct_lambdas(arguments);
    let engine = TypeInferenceEngine::new(Arc::clone(workspace_index), file_id);
    let positions = lambdas
        .iter()
        .map(|(position, lambda)| engine.lambda_argument(*position, lambda))
        .collect::<Vec<_>>();
    let resolver = ResolverBuilder::with_workspace(file_id, Arc::clone(workspace_index))
        .build_from_syntax(syntax_root);
    lambda_call_may_apply(
        workspace_index,
        owner_fqn,
        method_name,
        argument_types,
        &positions,
        |position, parameter_types| {
            lambdas.iter().find(|(index, _)| *index == position).map_or(
                LambdaReturnEvidence::Incompatible,
                |(_, lambda)| {
                    engine.infer_lambda_with_parameters(lambda, &resolver, parameter_types)
                },
            )
        },
    )
}

/// Resolve an overload from LSP argument inference results.
///
/// Syntax-specific inference stays in the LSP; candidate selection and Java
/// conversions are owned by `hyperion-semantics`.
pub fn resolve_overload(
    workspace_index: &Arc<WorkspaceIndex>,
    class_fqn: &str,
    method_name: &str,
    argument_types: &[InferredType],
) -> Option<OverloadMatch> {
    resolve_overload_with_receiver(
        workspace_index,
        class_fqn,
        None,
        method_name,
        argument_types,
    )
}

/// Resolve with a semantic receiver when the caller has one. Passing `None`
/// is an explicitly degraded/raw path and cannot project concrete owner args.
pub fn resolve_overload_with_receiver(
    workspace_index: &Arc<WorkspaceIndex>,
    class_fqn: &str,
    receiver_type: Option<&Type>,
    method_name: &str,
    argument_types: &[InferredType],
) -> Option<OverloadMatch> {
    let argument_types: Vec<_> = argument_types
        .iter()
        .map(|inferred| inferred.ty.clone())
        .collect();
    let receiver_owner = receiver_type
        .and_then(|receiver| receiver_owner_for_lookup(workspace_index, class_fqn, receiver));
    if receiver_type.is_some() && receiver_owner.is_none() {
        return None;
    }
    let owner_fqn = receiver_owner.as_deref().unwrap_or(class_fqn);
    resolve_call(
        workspace_index,
        owner_fqn,
        receiver_type,
        method_name,
        &argument_types,
    )
}

pub(crate) fn receiver_owner_for_lookup(
    workspace_index: &WorkspaceIndex,
    class_fqn: &str,
    receiver: &Type,
) -> Option<String> {
    match receiver {
        Type::Class(class) => {
            let inferred = class.qualified_name();
            if class.package.is_some() || workspace_index.get_symbol(&inferred).is_some() {
                return Some(inferred.into_owned());
            }

            let candidates = workspace_index.get_fqns_by_simple_name(&class.name);
            match candidates.as_slice() {
                [resolved] => Some(resolved.clone()),
                _ if class_fqn
                    .rsplit(['.', '$'])
                    .next()
                    .is_some_and(|terminal| terminal == class.name) =>
                {
                    // The caller already resolved this exact receiver spelling
                    // in its source/import context. Reuse that canonical owner;
                    // never choose among unrelated same-simple-name symbols here.
                    Some(class_fqn.to_owned())
                }
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use hyperion_semantics::symbol::{ClassType, Type};

    use super::*;

    #[test]
    fn source_index_overload_resolution_uses_string_literal_type() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let source = r"
            package com.example;
            public class TypeOverloadDemo {
                public void process(Integer value) {}
                public void process(String value) {}
            }
        ";
        let path = temp.path().join("com/example/TypeOverloadDemo.java");
        index
            .index_file_with_source(&path, source)
            .expect("source should index");

        let string_arg = InferredType::certain(Type::Class(ClassType::string()));
        let matched = resolve_overload(
            &index,
            "com.example.TypeOverloadDemo",
            "process",
            &[string_arg],
        )
        .expect("String overload should match");

        assert_eq!(
            matched.declaration.parameters.first().map(AsRef::as_ref),
            Some("java.lang.String value")
        );
    }

    #[test]
    fn typed_adapter_preserves_concrete_receiver_projection_for_lsp_consumers() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(
                &temp.path().join("p/Box.java"),
                concat!(
                    "package p; class Text {} class List<E> {} ",
                    "class Box<T> { T replace(T value) { return value; } } ",
                    "class Parent<T> { List<T> values() { return null; } } ",
                    "class Child<A> extends Parent<List<A>> {}"
                ),
            )
            .expect("generic box should index");
        let receiver = hyperion_semantics::type_parser::parse_type_string("p.Box<String>")
            .expect("receiver type");
        let string = hyperion_semantics::type_parser::parse_type_string("String")
            .expect("String argument type");
        let argument = InferredType::certain(string.clone());

        let matched = resolve_overload_with_receiver(
            &index,
            "p.Box",
            Some(&receiver),
            "replace",
            &[argument],
        )
        .expect("typed LSP overload should resolve");
        assert_eq!(matched.return_type, string);
        assert_eq!(matched.parameter_declarations, ["String value"]);

        let child = hyperion_semantics::type_parser::parse_type_string("p.Child<p.Text>")
            .expect("child receiver");
        let inherited = resolve_overload_with_receiver(
            &index,
            // Hover metadata names the declaring owner; the typed adapter must
            // use the actual receiver and traverse Child -> Parent itself.
            "p.Parent",
            Some(&child),
            "values",
            &[],
        )
        .expect("transformed inherited receiver projection");
        assert_eq!(inherited.return_type.to_string(), "p.List<p.List<p.Text>>");

        let simple_receiver = hyperion_semantics::type_parser::parse_type_string("Box<String>")
            .expect("source-relative receiver type");
        index
            .index_file_with_source(
                &temp.path().join("q/Box.java"),
                "package q; class Box<T> { T replace(T value) { return value; } }",
            )
            .expect("same-simple-name ambiguity fixture");
        let matched = resolve_overload_with_receiver(
            &index,
            "p.Box",
            Some(&simple_receiver),
            "replace",
            &[InferredType::certain(
                hyperion_semantics::type_parser::parse_type_string("String")
                    .expect("String argument type"),
            )],
        )
        .expect("canonical owner must retain simple receiver arguments");
        assert_eq!(matched.parameter_declarations, ["String value"]);
        assert!(
            resolve_overload_with_receiver(
                &index,
                "p.Parent",
                Some(&simple_receiver),
                "replace",
                &[InferredType::certain(
                    hyperion_semantics::type_parser::parse_type_string("String")
                        .expect("String argument type"),
                )],
            )
            .is_none(),
            "an ambiguous simple receiver must not bind to an unrelated canonical owner"
        );
    }
}
