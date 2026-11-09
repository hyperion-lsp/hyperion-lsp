// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Conformance matrix for simple-name -> FQN resolution.

use std::collections::{HashMap, HashSet};

use hyperion_semantics::resolved_name::ResolvedName;
use hyperion_semantics::unified_resolver::{NameContext, ResolutionResult, UnifiedTypeResolver};
use hyperion_types::ResolutionContext;
use hyperion_workspace::import_resolver::ImportResolver;
use hyperion_workspace::{
    DeclarationNamespace, ImportMap, Language, Modifiers, Symbol, TextRange, TypeNameResolution,
    WorkspaceIndex,
};

fn class_symbol(fqn: &str) -> Symbol {
    let simple = fqn.rsplit('.').next().unwrap_or(fqn);
    Symbol {
        fqn: fqn.into(),
        simple_name: simple.into(),
        range: TextRange::new(0, 1),
        kind: lsp_types::SymbolKind::CLASS,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Fqn(String),
    Ambiguous(Vec<String>),
    Unresolved,
}

impl Outcome {
    fn fqn(value: &str) -> Self {
        Self::Fqn(value.to_owned())
    }
}

impl From<TypeNameResolution> for Outcome {
    fn from(value: TypeNameResolution) -> Self {
        match value {
            TypeNameResolution::Exact(fqn) => Self::Fqn(fqn),
            TypeNameResolution::Missing => Self::Unresolved,
            TypeNameResolution::Ambiguous(all) => Self::Ambiguous(all),
        }
    }
}

/// The engines under test, in the order [`run`] reports them.
///
/// These are every simple-name -> FQN entry point a feature can reach.
const ENGINES: [&str; 7] = [
    "hyperion_types::ResolutionContext::resolve_identifier_without_lookup",
    "hyperion_types::ResolutionContext::resolve_identifier_with_lookup",
    "hyperion_workspace::ImportResolver::resolve (candidates[0])",
    "hyperion_workspace::ImportResolver::resolve_type_identity",
    "hyperion_workspace::WorkspaceIndex::resolve_type_name_in_context",
    "hyperion_semantics::UnifiedTypeResolver::resolve_name",
    "hyperion_semantics::UnifiedTypeResolver::resolve_type",
];

/// A documented deviation from the JLS answer that one engine's contract
/// permits. Every entry states *why*; none of them may be added merely to
/// make a test pass.
struct Deviation {
    engine: &'static str,
    /// The rendered outcome this engine produces instead of the JLS answer.
    expected: &'static str,
    reason: &'static str,
}

/// One resolution scenario, expressed only in source-visible facts.
struct Scenario {
    label: &'static str,
    package: Option<&'static str>,
    explicit: &'static [(&'static str, &'static str)],
    wildcards: &'static [&'static str],
    static_imports: &'static [(&'static str, &'static str)],
    /// Enclosing type FQNs, nearest first (for nested-type scenarios).
    lexical_owners: &'static [&'static str],
    /// Types the workspace has indexed, as bare declarations.
    indexed: &'static [&'static str],
    /// Files indexed from source, for scenarios that need real modifiers or
    /// containment (static member types).
    sources: &'static [(&'static str, &'static str)],
    written: &'static str,
    /// What the JLS says this spelling denotes.
    jls: Jls,
    deviations: &'static [Deviation],
}

/// The JLS answer for one scenario.
enum Jls {
    /// Denotes exactly this type.
    Denotes(&'static str),
    /// Ambiguous: a compile error, so no engine may silently pick one.
    Ambiguous(&'static [&'static str]),
    /// Denotes nothing; the name must be reported.
    Nothing,
}

impl Jls {
    fn rendered(&self) -> String {
        match self {
            Self::Denotes(fqn) => (*fqn).to_owned(),
            Self::Ambiguous(all) => all.join(" | "),
            Self::Nothing => UNRESOLVED.to_owned(),
        }
    }
}

/// How an engine that can express failure renders one.
const UNRESOLVED: &str = "<unresolved>";

/// Render an outcome as the flat spelling the expectation table uses.
fn render(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Fqn(fqn) => fqn.clone(),
        Outcome::Ambiguous(all) => all.join(" | "),
        Outcome::Unresolved => UNRESOLVED.to_owned(),
    }
}

impl Scenario {
    fn imports(&self) -> ImportMap {
        let mut imports = ImportMap::new();
        imports.current_package = self.package.map(ToOwned::to_owned);
        for (name, fqn) in self.explicit {
            imports.add_import((*name).to_owned(), (*fqn).to_owned());
        }
        for package in self.wildcards {
            imports.add_wildcard((*package).to_owned());
        }
        for (name, fqn) in self.static_imports {
            imports.add_static_import((*name).to_owned(), (*fqn).to_owned());
        }
        imports
    }

    fn resolution_context(&self) -> ResolutionContext {
        let explicit = self
            .explicit
            .iter()
            .map(|(name, fqn)| ((*name).to_owned(), (*fqn).to_owned()))
            .collect::<HashMap<_, _>>();
        ResolutionContext::with_wildcards(
            explicit,
            self.wildcards.iter().map(|p| (*p).to_owned()).collect(),
            self.package.map(ToOwned::to_owned),
            self.lexical_owners.first().map(|o| (*o).to_owned()),
        )
    }
}

fn string_outcome(written: &str, resolved: String) -> Outcome {
    if resolved == written && !written.contains('.') {
        Outcome::Unresolved
    } else {
        Outcome::Fqn(resolved)
    }
}

/// Every engine's answer for one scenario, in [`ENGINES`] order.
fn run(scenario: &Scenario) -> Vec<Outcome> {
    let temp = tempfile::tempdir().expect("temp dir");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for fqn in scenario.indexed {
        index.add_external_symbol(class_symbol(fqn));
    }
    for (name, source) in scenario.sources {
        index
            .index_file_with_source(&temp.path().join(name), source)
            .expect("index fixture source");
    }

    let imports = scenario.imports();
    let context = scenario.resolution_context();
    let import_resolver = ImportResolver::new(&imports, &index);
    let lexical_owners = scenario
        .lexical_owners
        .iter()
        .map(|o| (*o).to_owned())
        .collect::<Vec<_>>();

    let written = scenario.written;
    let unified = UnifiedTypeResolver::new(&index, &imports, None);
    let type_parameters = HashSet::new();
    let local_types = HashSet::new();
    let name_context = NameContext {
        in_switch_case: false,
        type_parameters: &type_parameters,
        variable_scopes: &[],
        local_types: &local_types,
        lexical_owners: &[],
        allow_external_type: true,
        expected_namespace: Some(DeclarationNamespace::Type),
    };

    vec![
        string_outcome(written, context.resolve_identifier_without_lookup(written)),
        string_outcome(
            written,
            context.resolve_identifier_with_lookup(written, &index),
        ),
        import_resolver
            .resolve(written)
            .first()
            .map_or(Outcome::Unresolved, |first| Outcome::Fqn(first.clone())),
        Outcome::from(import_resolver.resolve_type_identity(written)),
        Outcome::from(index.resolve_type_name_in_context(
            &imports,
            &lexical_owners,
            written,
            Language::Java,
        )),
        match unified.resolve_name(written, 0, &name_context) {
            Some(ResolvedName::Type { fqn, .. } | ResolvedName::External { fqn }) => {
                Outcome::Fqn(fqn)
            }
            Some(other) => Outcome::Fqn(format!("<{other:?}>")),
            None => Outcome::Unresolved,
        },
        match unified.resolve_type(written, 0) {
            ResolutionResult::Workspace { fqn } => Outcome::Fqn(fqn),
            ResolutionResult::External => Outcome::fqn("<external>"),
            ResolutionResult::SameFile { .. } => Outcome::fqn("<same-file>"),
            ResolutionResult::NotFound => Outcome::Unresolved,
        },
    ]
}

/// A JDK indexed down to the handful of names these scenarios reference.
const JDK: &[&str] = &[
    "java.lang.Object",
    "java.lang.String",
    "java.lang.Record",
    "java.lang.Comparable",
    "java.util.List",
];

/// Degradation reasons, named once so the same contract limit is not
/// re-argued per scenario.
mod why {
    /// The lookup-less mode cannot evaluate the tiers the JLS makes
    /// conditional on a type existing, so it guesses in likelihood order.
    pub const NO_EVIDENCE: &str =
        "degraded mode: no workspace lookup, so existence-conditional tiers are guesses";
    /// The `String`-returning engines cannot express "unresolved" or
    /// "ambiguous", so they return their best guess.
    pub const STRING_CONTRACT: &str =
        "returns a String, so it cannot express Missing or Ambiguous and yields its best guess";
    /// The index has no declaration to point at, but the language binding
    /// still holds.
    pub const NEEDS_INDEXED_JDK: &str =
        "resolves against indexed declarations only, so an unindexed JDK is Missing";
    /// Suppression of a cold dependency, not resolution.
    pub const COLD_DEPENDENCY: &str =
        "suppresses rather than resolves while the only possible reading's package is unindexed";
    /// No public lexical-owner-aware entry point.
    pub const NO_LEXICAL_OWNERS: &str =
        "public entry point takes no lexical owners, so member types of an enclosing type are invisible";
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        label: "explicit single-type import",
        package: Some("app"),
        explicit: &[("User", "com.model.User")],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &["com.model.User"],
        sources: &[],
        written: "User",
        jls: Jls::Denotes("com.model.User"),
        deviations: &[],
    },
    Scenario {
        label: "explicit single-type import shadows java.lang (JLS 7.5.1)",
        package: Some("app"),
        explicit: &[("String", "custom.String")],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &["custom.String", "java.lang.Object", "java.lang.String"],
        sources: &[],
        written: "String",
        jls: Jls::Denotes("custom.String"),
        deviations: &[],
    },
    Scenario {
        label: "same-package type that exists",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &["app.Widget", "app.Other"],
        sources: &[],
        written: "Widget",
        jls: Jls::Denotes("app.Widget"),
        deviations: &[],
    },
    Scenario {
        label: "same-package name whose package is entirely unindexed",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &[],
        sources: &[],
        written: "Ghost",
        // The workspace has no evidence either way. `app.Ghost` is the only
        // reading the spelling has, so nothing may claim it is wrong.
        jls: Jls::Denotes("app.Ghost"),
        deviations: &[
            Deviation {
                engine: "hyperion_workspace::ImportResolver::resolve_type_identity",
                expected: UNRESOLVED,
                reason: why::NEEDS_INDEXED_JDK,
            },
            Deviation {
                engine: "hyperion_workspace::WorkspaceIndex::resolve_type_name_in_context",
                expected: UNRESOLVED,
                reason: why::NEEDS_INDEXED_JDK,
            },
            Deviation {
                engine: "hyperion_semantics::UnifiedTypeResolver::resolve_type",
                expected: UNRESOLVED,
                reason: "disagrees with resolve_name, which suppresses this as cold",
            },
        ],
    },
    Scenario {
        label: "java.lang with an indexed JDK",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: JDK,
        sources: &[],
        written: "String",
        jls: Jls::Denotes("java.lang.String"),
        deviations: &[],
    },
    Scenario {
        label: "java.lang without an indexed JDK",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &["app.Other"],
        sources: &[],
        written: "String",
        jls: Jls::Denotes("java.lang.String"),
        deviations: &[
            Deviation {
                engine: "hyperion_workspace::WorkspaceIndex::resolve_type_name_in_context",
                expected: UNRESOLVED,
                reason: why::NEEDS_INDEXED_JDK,
            },
            Deviation {
                engine: "hyperion_semantics::UnifiedTypeResolver::resolve_type",
                expected: "<external>",
                reason: why::COLD_DEPENDENCY,
            },
        ],
    },
    Scenario {
        // This is the case the second java.lang table got wrong.
        label: "java.lang interface without an indexed JDK",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &["app.Other"],
        sources: &[],
        written: "Comparable",
        jls: Jls::Denotes("java.lang.Comparable"),
        deviations: &[
            Deviation {
                engine: "hyperion_workspace::WorkspaceIndex::resolve_type_name_in_context",
                expected: UNRESOLVED,
                reason: why::NEEDS_INDEXED_JDK,
            },
            Deviation {
                engine: "hyperion_semantics::UnifiedTypeResolver::resolve_type",
                expected: "<external>",
                reason: why::COLD_DEPENDENCY,
            },
        ],
    },
    Scenario {
        // A package member shadows the java.lang import-on-demand.
        label: "JLS 6.5.5: an existing same-package type shadows java.lang",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &[
            "java.lang.Object",
            "java.lang.String",
            "java.lang.Record",
            "app.Record",
        ],
        sources: &[],
        written: "Record",
        jls: Jls::Denotes("app.Record"),
        deviations: &[Deviation {
            engine: "hyperion_types::ResolutionContext::resolve_identifier_without_lookup",
            expected: "java.lang.Record",
            reason: why::NO_EVIDENCE,
        }],
    },
    Scenario {
        label: "single on-demand import",
        package: Some("app"),
        explicit: &[],
        wildcards: &["lib"],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &["lib.Helper"],
        sources: &[],
        written: "Helper",
        jls: Jls::Denotes("lib.Helper"),
        deviations: &[Deviation {
            engine: "hyperion_types::ResolutionContext::resolve_identifier_without_lookup",
            expected: "app.Helper",
            reason: why::NO_EVIDENCE,
        }],
    },
    Scenario {
        label: "two on-demand imports both match (compile error, JLS 7.5.2)",
        package: Some("app"),
        explicit: &[],
        wildcards: &["a", "b"],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &["a.Dup", "b.Dup"],
        sources: &[],
        written: "Dup",
        jls: Jls::Ambiguous(&["a.Dup", "b.Dup"]),
        deviations: &[
            Deviation {
                engine: "hyperion_types::ResolutionContext::resolve_identifier_without_lookup",
                expected: "app.Dup",
                reason: why::STRING_CONTRACT,
            },
            Deviation {
                engine: "hyperion_types::ResolutionContext::resolve_identifier_with_lookup",
                expected: "a.Dup",
                reason: why::STRING_CONTRACT,
            },
            Deviation {
                engine: "hyperion_workspace::ImportResolver::resolve (candidates[0])",
                expected: "a.Dup",
                reason: "candidate vector: ambiguity is the caller's to see in the full list",
            },
            Deviation {
                engine: "hyperion_semantics::UnifiedTypeResolver::resolve_name",
                expected: UNRESOLVED,
                reason: "an ambiguous spelling has no identity to cache",
            },
            Deviation {
                engine: "hyperion_semantics::UnifiedTypeResolver::resolve_type",
                expected: UNRESOLVED,
                reason: "an ambiguous spelling has no identity to cache",
            },
        ],
    },
    Scenario {
        label: "same package shadows an on-demand import",
        package: Some("app"),
        explicit: &[],
        wildcards: &["lib"],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &["app.Shared", "lib.Shared"],
        sources: &[],
        written: "Shared",
        jls: Jls::Denotes("app.Shared"),
        deviations: &[],
    },
    Scenario {
        label: "nested type written Outer.Inner",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &["app.Outer", "app.Outer.Inner"],
        sources: &[],
        written: "Outer.Inner",
        jls: Jls::Denotes("app.Outer.Inner"),
        deviations: &[Deviation {
            engine: "hyperion_types::ResolutionContext::resolve_identifier_without_lookup",
            expected: "Outer.Inner",
            reason: why::NO_EVIDENCE,
        }],
    },
    Scenario {
        label: "nested type by simple name inside its owner",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &["app.Outer"],
        indexed: &["app.Outer", "app.Outer.Inner"],
        sources: &[],
        written: "Inner",
        jls: Jls::Denotes("app.Outer.Inner"),
        deviations: &[
            Deviation {
                engine: "hyperion_types::ResolutionContext::resolve_identifier_without_lookup",
                expected: "app.Inner",
                reason: why::NO_EVIDENCE,
            },
            Deviation {
                engine: "hyperion_workspace::ImportResolver::resolve (candidates[0])",
                expected: "app.Inner",
                reason: why::NO_LEXICAL_OWNERS,
            },
            Deviation {
                engine: "hyperion_workspace::ImportResolver::resolve_type_identity",
                expected: UNRESOLVED,
                reason: why::NO_LEXICAL_OWNERS,
            },
            Deviation {
                engine: "hyperion_semantics::UnifiedTypeResolver::resolve_name",
                expected: UNRESOLVED,
                reason: why::NO_LEXICAL_OWNERS,
            },
            Deviation {
                engine: "hyperion_semantics::UnifiedTypeResolver::resolve_type",
                expected: UNRESOLVED,
                reason: why::NO_LEXICAL_OWNERS,
            },
        ],
    },
    Scenario {
        label: "fully qualified name",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: JDK,
        sources: &[],
        written: "java.util.List",
        jls: Jls::Denotes("java.util.List"),
        deviations: &[],
    },
    Scenario {
        label: "genuine typo in a fully indexed package",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[],
        lexical_owners: &[],
        indexed: &[
            "app.Widget",
            "app.Other",
            "java.lang.Object",
            "java.lang.String",
        ],
        sources: &[],
        written: "Widgit",
        jls: Jls::Nothing,
        deviations: &[
            Deviation {
                engine: "hyperion_types::ResolutionContext::resolve_identifier_without_lookup",
                expected: "app.Widgit",
                reason: why::STRING_CONTRACT,
            },
            Deviation {
                engine: "hyperion_types::ResolutionContext::resolve_identifier_with_lookup",
                expected: "app.Widgit",
                reason: why::STRING_CONTRACT,
            },
            Deviation {
                engine: "hyperion_workspace::ImportResolver::resolve (candidates[0])",
                expected: "app.Widgit",
                reason: "candidate vector: every entry is a probe target, not a claim",
            },
        ],
    },
    Scenario {
        label: "static-imported member type",
        package: Some("app"),
        explicit: &[],
        wildcards: &[],
        static_imports: &[("Nested", "lib.Owner.Nested")],
        lexical_owners: &[],
        indexed: &[],
        sources: &[(
            "Owner.java",
            "package lib; public class Owner { public static class Nested {} }",
        )],
        written: "Nested",
        jls: Jls::Denotes("lib.Owner.Nested"),
        deviations: &[
            Deviation {
                engine: "hyperion_types::ResolutionContext::resolve_identifier_without_lookup",
                expected: "app.Nested",
                reason: "ResolutionContext carries no static imports at all",
            },
            Deviation {
                engine: "hyperion_types::ResolutionContext::resolve_identifier_with_lookup",
                expected: "app.Nested",
                reason: "ResolutionContext carries no static imports at all",
            },
            Deviation {
                engine: "hyperion_workspace::ImportResolver::resolve (candidates[0])",
                expected: "app.Nested",
                reason: "the candidate vector models type imports only, never static imports",
            },
        ],
    },
];

/// Expected rendered outcomes for one scenario, in [`ENGINES`] order, each
/// with the reason a deviation from the JLS answer is permitted.
fn expectations(scenario: &Scenario) -> Vec<(String, Option<&'static str>)> {
    let jls = scenario.jls.rendered();
    ENGINES
        .iter()
        .map(|engine| {
            scenario
                .deviations
                .iter()
                .find(|deviation| deviation.engine == *engine)
                .map_or_else(
                    || (jls.clone(), None),
                    |deviation| (deviation.expected.to_owned(), Some(deviation.reason)),
                )
        })
        .collect()
}

/// Every simple-name -> FQN engine, over the same inputs, checked
/// against the JLS rather than against each other's current behaviour.
///
/// A `Deviation` entry is a *contract* statement, not a way to record whatever
/// an engine happens to do: it must name the property of that engine's
/// signature or purpose that makes the JLS answer unreachable. Anything else
/// is a bug to fix, not a deviation to add.
#[test]
fn every_engine_matches_the_jls_or_a_documented_contract_limit() {
    let mut failures = Vec::new();
    for scenario in SCENARIOS {
        let actual = run(scenario);
        for ((engine, (expected, reason)), got) in ENGINES
            .iter()
            .zip(expectations(scenario))
            .zip(actual.iter().map(render))
        {
            if got != expected {
                failures.push(format!(
                    "\n{}\n  engine   : {engine}\n  expected : {expected}{}\n  actual   : {got}",
                    scenario.label,
                    reason.map_or(String::new(), |reason| format!("  ({reason})")),
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "name resolution conformance divergences:{}",
        failures.join("")
    );
}

/// The engines that can express a resolution failure must agree on it. A
/// spelling that denotes nothing, in a workspace that has the evidence to know
/// that, must not resolve anywhere.
#[test]
fn identity_engines_agree_that_a_typo_is_a_typo() {
    let typo = SCENARIOS
        .iter()
        .find(|scenario| scenario.label == "genuine typo in a fully indexed package")
        .expect("typo scenario");
    let actual = run(typo);

    for engine in [
        "hyperion_workspace::ImportResolver::resolve_type_identity",
        "hyperion_workspace::WorkspaceIndex::resolve_type_name_in_context",
        "hyperion_semantics::UnifiedTypeResolver::resolve_name",
        "hyperion_semantics::UnifiedTypeResolver::resolve_type",
    ] {
        let position = ENGINES.iter().position(|name| *name == engine).unwrap();
        assert_eq!(
            actual[position],
            Outcome::Unresolved,
            "{engine} must report a typo it has the evidence to detect"
        );
    }
}

/// Print the full matrix. Useful when adding a scenario or changing a tier.
#[test]
#[ignore = "diagnostic output, not an assertion"]
fn print_conformance_matrix() {
    for scenario in SCENARIOS {
        println!("\n=== {} :: `{}`", scenario.label, scenario.written);
        for (engine, outcome) in ENGINES.iter().zip(run(scenario)) {
            println!("  {engine:<62} {outcome:?}");
        }
    }
}
