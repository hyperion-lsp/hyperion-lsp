// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Import candidates for unresolved simple names.
//!
//! Candidates come from the workspace index only (project sources, declared
//! dependencies, the JDK). They are filtered for what a developer could
//! legally and sensibly import, then ranked so the expected answer comes
//! first: `List` -> `java.util.List` before `java.awt.List`, and never
//! `com.sun.tools.javac.util.List`.

use hyperion_workspace::model::is_type_declaration_kind;
use hyperion_workspace::WorkspaceIndex;

/// Packages a developer means when several JDK types share a simple name.
const PREFERRED_PACKAGES: &[&str] = &[
    "java.util",
    "java.util.function",
    "java.util.stream",
    "java.util.concurrent",
    "java.util.concurrent.atomic",
    "java.util.regex",
    "java.io",
    "java.nio.file",
    "java.nio",
    "java.net",
    "java.time",
    "java.time.format",
    "java.math",
    "java.text",
    "java.lang.reflect",
    "java.lang.annotation",
];

/// JDK packages that are not exported API (or are implementation detail).
fn is_jdk_internal(package: &str) -> bool {
    package.starts_with("sun.")
        || package.starts_with("jdk.internal.")
        || package.starts_with("com.sun.")
        || package.starts_with("apple.")
        || package.starts_with("com.apple.")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub fqn: String,
    tier: u8,
    preference: usize,
}

/// Ranked import candidates for `simple_name`, best first.
pub fn import_candidates(
    index: &WorkspaceIndex,
    simple_name: &str,
    current_package: &str,
) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = Vec::new();
    for fqn in index.get_fqns_by_simple_name(simple_name) {
        let Some(symbol) = index.get_symbol(&fqn) else {
            continue;
        };
        // Types only: never constructors, methods, fields or enum constants.
        if !is_type_declaration_kind(symbol.kind) {
            continue;
        }
        // Walk out to the outermost owning type; its package is the package.
        let mut outer = fqn.clone();
        let mut nested = false;
        while let Some(owner) = index
            .get_symbol(&outer)
            .and_then(|s| s.container.as_deref().map(str::to_string))
        {
            if index
                .get_symbol(&owner)
                .is_some_and(|s| is_type_declaration_kind(s.kind))
            {
                outer = owner;
                nested = true;
            } else {
                break;
            }
        }
        let package = outer
            .rsplit_once('.')
            .map_or(String::new(), |(package, _)| package.to_string());
        if symbol.modifiers.is_private {
            continue;
        }
        let same_package = package == current_package;
        // Package-private (or protected) types are importable only from
        // their own package, which needs no import at all.
        if !symbol.modifiers.is_public {
            continue;
        }
        if !nested && same_package {
            continue;
        }
        let (tier, jdk) = classify(index, &fqn, &package);
        if jdk && is_jdk_internal(&package) {
            continue;
        }
        let preference = PREFERRED_PACKAGES
            .iter()
            .position(|preferred| *preferred == package)
            .unwrap_or(PREFERRED_PACKAGES.len());
        candidates.push(Candidate {
            fqn,
            tier,
            preference,
        });
    }
    candidates.sort_by(|a, b| (a.tier, a.preference, &a.fqn).cmp(&(b.tier, b.preference, &b.fqn)));
    candidates.dedup_by(|a, b| a.fqn == b.fqn);
    candidates
}

/// `(tier, is_jdk)`: 0 = java.*/javax.*, 1 = project sources, 2 = declared
/// dependencies, 3 = other JDK modules.
fn is_jdk_source(name: &str) -> bool {
    name == "jdk"
        || (std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jmod"))
            && name.contains("jmods"))
        || name.ends_with("lib/modules")
}

fn classify(index: &WorkspaceIndex, fqn: &str, package: &str) -> (u8, bool) {
    let Some(location) = index.find_symbol(fqn) else {
        return (2, false);
    };
    if !index.is_external_file(location.file_id) {
        return (1, false);
    }
    // JDK classes are indexed from the installation's `jmods` archives (or
    // the legacy virtual "jdk" source); anything else external is a
    // dependency jar.
    let jdk = index
        .get_external_source_name(location.file_id)
        .is_some_and(|name| is_jdk_source(&name));
    if jdk {
        if package.starts_with("java.") || package.starts_with("javax.") {
            (0, true)
        } else {
            (3, true)
        }
    } else {
        (2, false)
    }
}

/// The candidate to import automatically for `name`, when there is a clear
/// winner (a sole candidate, or a sole preferred-package candidate).
pub fn unambiguous(candidates: &[Candidate]) -> Option<&Candidate> {
    match candidates {
        [] => None,
        [only] => Some(only),
        [first, rest @ ..] => {
            let preferred = |c: &Candidate| c.preference < PREFERRED_PACKAGES.len();
            (preferred(first)
                && !rest.iter().any(preferred)
                && rest.iter().all(|c| c.tier >= first.tier))
            .then_some(first)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jdk_sources_are_recognised_by_their_archive_spelling() {
        assert!(is_jdk_source("jdk"));
        assert!(is_jdk_source(
            "/usr/lib/jvm/java-26-openjdk/jmods/jdk.compiler.jmod"
        ));
        assert!(is_jdk_source("/opt/jdk/lib/modules"));
        assert!(!is_jdk_source("/home/u/.gradle/caches/x/jna-5.14.0.jar"));
    }

    #[test]
    fn jdk_internal_packages_are_never_candidates_but_library_com_sun_is_not_jdk() {
        assert!(is_jdk_internal("com.sun.tools.javac.util"));
        assert!(is_jdk_internal("jdk.internal.misc"));
        assert!(is_jdk_internal("sun.security.x509"));
        assert!(!is_jdk_internal("java.util"));
        assert!(!is_jdk_internal("javax.swing"));
    }

    #[test]
    fn a_sole_preferred_package_candidate_wins_over_other_java_packages() {
        let candidate = |fqn: &str, preference: usize| Candidate {
            fqn: fqn.to_string(),
            tier: 0,
            preference,
        };
        let list = [
            candidate("java.util.List", 0),
            candidate("java.awt.List", PREFERRED_PACKAGES.len()),
        ];
        assert_eq!(
            unambiguous(&list).map(|c| c.fqn.as_str()),
            Some("java.util.List")
        );
        let ambiguous = [
            candidate("java.util.Date", 0),
            candidate("java.sql.Date", PREFERRED_PACKAGES.len()),
            candidate("java.util.Date2", 0),
        ];
        assert_eq!(unambiguous(&ambiguous), None);
    }
}
