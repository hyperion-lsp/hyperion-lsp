// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression test for exponential traversal of a diamond hierarchy.
//!
//! Java diamond inheritance -- `class Foo implements B, C` where `B extends A`
//! and `C extends A` -- reaches `A` by two paths, and everything above `A` is
//! reachable twice again. A hierarchy walk that does not deduplicate visited
//! owners is exponential in depth: a 3-wide x 14-deep interface graph forces
//! ~3^14 = 4.8M traversals. Deep generic interface hierarchies of exactly this
//! shape are ordinary in real code (framework bean/interceptor hierarchies, the
//! Kotlin stdlib collection interfaces).
//!
//! This used to guard `NavigationProvider::find_method_with_inheritance_by_args`,
//! a hand-rolled recursive walk over `parent_class` and `interfaces`. That
//! walk had no production caller and has been deleted; every
//! feature now goes through the canonical effective-method election, so the
//! bound is asserted there instead -- on the code path that actually runs.
//!
//! The test was unreachable before interface multi-extends indexing was fixed:
//! the indexer used to flatten `interface C extends A, B` to a single broken
//! `parent_class = "A.B"` with an empty `interfaces` list, so the diamond
//! fan-out could not be constructed at all. `diamond_hierarchy_is_actually_wired_up`
//! guards against that regressing and silently defanging the bound below.

use std::sync::Arc;
use std::time::{Duration, Instant};

use hyperion_workspace::WorkspaceIndex;
use tempfile::TempDir;

/// Build a 3-wide × `depth`-deep diamond interface hierarchy plus a class
/// implementing the three root interfaces.
///
/// At every level i in 0..depth-1, we create three interfaces
/// `diamond.I_i_0/1/2`, each declaring `extends I_(i+1)_0, I_(i+1)_1,
/// I_(i+1)_2`. The deepest level (depth-1) has three leaf interfaces with
/// no parents. The class `diamond.Root` implements `I_0_0, I_0_1, I_0_2`.
///
/// Without dedup, walking from `Root` upward visits every path through
/// this graph: 3^depth traversals. With dedup, ~3*depth nodes total.
fn build_diamond_hierarchy_workspace(depth: usize) -> (TempDir, Arc<WorkspaceIndex>) {
    let temp_dir = TempDir::new().expect("tempdir");
    let workspace_root = temp_dir.path().to_path_buf();
    let pkg_dir = workspace_root.join("src/main/java/diamond");
    std::fs::create_dir_all(&pkg_dir).unwrap();

    let workspace_index = Arc::new(WorkspaceIndex::new(workspace_root.clone()));

    for level in 0..depth {
        for n in 0..3 {
            let name = format!("I_{level}_{n}");
            let extends = if level + 1 < depth {
                format!(
                    " extends I_{next}_0, I_{next}_1, I_{next}_2",
                    next = level + 1
                )
            } else {
                String::new()
            };
            let src = format!("package diamond; public interface {name}{extends} {{ }}");
            let path = pkg_dir.join(format!("{name}.java"));
            std::fs::write(&path, &src).unwrap();
            workspace_index
                .index_file_with_source(&path, &src)
                .expect("index interface");
        }
    }

    let root_src = "package diamond; public class Root implements I_0_0, I_0_1, I_0_2 { }";
    let root_path = pkg_dir.join("Root.java");
    std::fs::write(&root_path, root_src).unwrap();
    workspace_index
        .index_file_with_source(&root_path, root_src)
        .expect("index Root");

    (temp_dir, workspace_index)
}

/// Sanity: confirm the diamond is actually wired up the way we think it
/// is. If interface multi-extends regresses again the inheritance walk
/// becomes shallow and the timing test below would silently pass even on
/// the buggy traversal — this guards against that.
#[test]
fn diamond_hierarchy_is_actually_wired_up() {
    let (_tmp, idx) = build_diamond_hierarchy_workspace(3);

    let root = idx.get_symbol("diamond.Root").expect("Root indexed");
    assert_eq!(
        root.interfaces.len(),
        3,
        "Root should implement 3 interfaces"
    );

    // I_0_0 must extend three level-1 interfaces.
    let i00 = idx.get_symbol("diamond.I_0_0").expect("I_0_0 indexed");
    assert_eq!(
        i00.interfaces.len(),
        3,
        "I_0_0 should extend 3 interfaces (got {:?})",
        i00.interfaces
    );
    assert!(i00.parent_class.is_none(), "interface has no parent_class");
}

/// Searching for a method that does not exist anywhere in the diamond graph
/// must terminate quickly. Without visited-owner dedup, a 3-wide x 14-deep
/// hierarchy forces ~3^14 = 4.8M traversals and takes well over a second.
#[test]
fn member_election_terminates_quickly_on_diamond_inheritance() {
    let depth = 14;
    let (_temp_dir, workspace_index) = build_diamond_hierarchy_workspace(depth);

    let start = Instant::now();
    let by_name = workspace_index.find_effective_methods_by_name("diamond.Root", "noSuchMethod");
    let elapsed = start.elapsed();

    assert!(
        by_name.is_empty(),
        "did not expect to find `noSuchMethod` anywhere in the diamond"
    );

    let bound = Duration::from_millis(500);
    assert!(
        elapsed < bound,
        "REGRESSION: find_effective_methods_by_name took {elapsed:?} on a 3-wide x {depth}-deep \
         diamond inheritance graph (bound: {bound:?}). Expected linear traversal via visited-owner \
         dedup; got exponential."
    );
    eprintln!("find_effective_methods_by_name on diamond({depth}) took {elapsed:?}");
}

/// The same bound for the whole inherited-member set, which every
/// override/implement feature asks for.
#[test]
fn inherited_member_collection_terminates_quickly_on_diamond_inheritance() {
    let depth = 14;
    let (_temp_dir, workspace_index) = build_diamond_hierarchy_workspace(depth);

    let start = Instant::now();
    let inherited = workspace_index.find_inherited_methods("diamond.Root", &[]);
    let elapsed = start.elapsed();

    assert!(
        inherited.is_empty(),
        "the diamond declares no methods, got {inherited:?}"
    );

    let bound = Duration::from_millis(500);
    assert!(
        elapsed < bound,
        "REGRESSION: find_inherited_methods took {elapsed:?} on a 3-wide x {depth}-deep diamond \
         inheritance graph (bound: {bound:?})."
    );
    eprintln!("find_inherited_methods on diamond({depth}) took {elapsed:?}");
}
