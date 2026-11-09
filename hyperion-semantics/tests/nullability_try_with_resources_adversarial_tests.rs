// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Adversarial try-with-resources tests.
//!
//! Resource initialization and closing form a stack of implicit cleanup
//! scopes. Initializers run left-to-right, successful resources close in the
//! opposite order, and close failures are suppressed only while an existing
//! exception is unwinding through that resource.

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_parser::{lexer::Lexer, parser::Parser, syntax::SyntaxKind};
use hyperion_semantics::nullability::{
    extract_method_info, BlockId, ControlFlowGraph, DiagnosticKind, NullabilityAnalysis,
    StatementRef, Terminator,
};

fn dereference_warnings(analysis: &NullabilityAnalysis) -> Vec<&str> {
    analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
        .map(|diagnostic| diagnostic.variable_name.as_str())
        .collect()
}

fn build_test_cfg(source: &str) -> ControlFlowGraph {
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    assert!(errors.is_empty(), "fixture must parse cleanly: {errors:?}");
    let root = hyperion_parser::syntax::SyntaxNode::new_root(green);
    let body = root
        .descendants()
        .find(|node| {
            node.kind() == SyntaxKind::METHOD_DECL
                && extract_method_info(node).is_some_and(|info| info.name == "test")
        })
        .and_then(|method| {
            method
                .children()
                .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
        })
        .expect("test method body");
    ControlFlowGraph::build(&body)
}

fn block_with_analyzed_text(cfg: &ControlFlowGraph, needle: &str) -> BlockId {
    cfg.blocks
        .values()
        .find(|block| {
            block.statements.iter().any(|statement| {
                matches!(statement, StatementRef::Analyze(node) if node.text().to_string().contains(needle))
            })
        })
        .map_or_else(
            || panic!("analyzed statement containing {needle:?}"),
            |block| block.id,
        )
}

fn close_block(cfg: &ControlFlowGraph, needle: &str) -> BlockId {
    cfg.blocks
        .values()
        .find(|block| {
            block.statements.iter().any(|statement| {
                matches!(statement, StatementRef::CloseResource(node) if node.text().to_string().contains(needle))
            })
        })
        .map_or_else(
            || panic!("resource close containing {needle:?}"),
            |block| block.id,
        )
}

#[test]
fn null_resource_skips_close_and_cannot_enter_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            static class Resource implements AutoCloseable {
                public void close() throws Exception {}
            }

            void test() {
                Object state = new Object();
                Resource resource = null;
                try (resource) {
                } catch (Exception impossible) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "Java tests a captured resource against null and skips close(): {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nonnull_resource_close_failure_enters_the_try_catch() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            static class Resource implements AutoCloseable {
                public void close() throws Exception {}
            }

            void test() {
                Object state = new Object();
                Resource resource = new Resource();
                try (resource) {
                } catch (Exception closeFailure) {
                    state = null;
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["state"]);
}

#[test]
fn body_exception_remains_primary_when_resource_close_also_fails() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            static class BodyFailure extends Exception {}
            static class CloseFailure extends Exception {}
            static class Resource implements AutoCloseable {
                public void close() throws CloseFailure {}
            }

            void test() {
                Object state = null;
                try (Resource resource = new Resource()) {
                    throw new BodyFailure();
                } catch (CloseFailure suppressed) {
                    state = null;
                } catch (BodyFailure primary) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "the close failure is suppressed and must not replace the body exception: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn initializer_exception_remains_primary_while_earlier_resource_closes() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            static class CloseFailure extends Exception {}
            static class Resource implements AutoCloseable {
                public void close() throws CloseFailure {}
            }

            void test() {
                Object state = null;
                Resource[] missing = null;
                try (
                    Resource first = new Resource();
                    Resource failed = missing[0]
                ) {
                    state = null;
                } catch (CloseFailure suppressed) {
                    state = null;
                } catch (NullPointerException primary) {
                    state = new Object();
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["missing"],
        "the expected initializer dereference warns, but the suppressed close catch must not taint state: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn resources_close_in_reverse_acquisition_order() {
    let cfg = build_test_cfg(
        r#"
        class Test {
            static class Resource implements AutoCloseable {
                public void close() throws Exception {}
            }

            void test() {
                try (
                    Resource first = new Resource();
                    Resource second = new Resource()
                ) {
                    bodyMarker();
                }
            }
        }
        "#,
    );
    let body = block_with_analyzed_text(&cfg, "bodyMarker");
    let first_close = close_block(&cfg, "Resource first");
    let second_close = close_block(&cfg, "Resource second");

    assert!(matches!(
        cfg.blocks.get(&body).map(|block| &block.terminator),
        Some(Terminator::Goto(target)) if *target == second_close
    ));
    assert!(matches!(
        cfg.blocks
            .get(&second_close)
            .map(|block| &block.terminator),
        Some(Terminator::CleanupEnd { normal_target, .. }) if *normal_target == first_close
    ));
}

#[test]
fn resource_is_acquired_only_after_its_initializer_completes() {
    let cfg = build_test_cfg(
        r#"
        class Test {
            static class Resource implements AutoCloseable {
                public void close() throws Exception {}
            }

            void test() {
                try (
                    Resource first = new Resource();
                    Resource second = makeResource()
                ) {
                }
            }
        }
        "#,
    );
    let first_initializer = block_with_analyzed_text(&cfg, "Resource first");
    let second_initializer = block_with_analyzed_text(&cfg, "Resource second");
    let first_close = close_block(&cfg, "Resource first");
    let second_close = close_block(&cfg, "Resource second");

    assert!(
        !cfg.all_successors(first_initializer).contains(&first_close),
        "a resource whose initializer fails was never acquired"
    );
    assert!(
        cfg.all_successors(second_initializer)
            .contains(&first_close),
        "failure of a later initializer must close an earlier acquired resource"
    );
    assert!(
        !cfg.all_successors(second_initializer)
            .contains(&second_close),
        "failure of the second initializer must not close the unacquired second resource"
    );
}

#[test]
fn value_return_enters_the_innermost_resource_cleanup() {
    let cfg = build_test_cfg(
        r#"
        class Test {
            static class Resource implements AutoCloseable {
                public void close() throws Exception {}
            }

            Object test() {
                try (
                    Resource first = new Resource();
                    Resource second = new Resource()
                ) {
                    return new Object();
                }
            }
        }
        "#,
    );
    let second_close = close_block(&cfg, "Resource second");
    let return_cleanup_target = cfg
        .blocks
        .values()
        .find_map(|block| match &block.terminator {
            Terminator::ReturnThroughCleanup { target, .. } => Some(*target),
            _ => None,
        });

    assert_eq!(return_cleanup_target, Some(second_close));
}

#[test]
fn close_failure_can_replace_a_pending_break() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            static class Resource implements AutoCloseable {
                public void close() throws Exception {}
            }

            void test() {
                Object state = new Object();
                while (true) {
                    try (Resource resource = new Resource()) {
                        break;
                    } catch (Exception closeFailure) {
                        state = null;
                        break;
                    }
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["state"]);
}

#[test]
fn close_failure_can_replace_a_pending_continue() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            static class Resource implements AutoCloseable {
                public void close() throws Exception {}
            }

            void test() {
                Object state = new Object();
                for (int attempt = 0; attempt < 1; attempt++) {
                    try (Resource resource = new Resource()) {
                        continue;
                    } catch (Exception closeFailure) {
                        state = null;
                        break;
                    }
                }
                state.toString();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["state"]);
}

#[test]
fn close_failure_can_replace_a_pending_value_return() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            static class Resource implements AutoCloseable {
                public void close() throws Exception {}
            }

            @NonNull Object test() {
                Object fallback = null;
                try (Resource resource = new Resource()) {
                    return new Object();
                } catch (Exception closeFailure) {
                    return fallback;
                }
            }
        }
        "#,
    );

    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::NullReturnFromNonNull),
        "a close failure replaces the captured non-null return and reaches the nullable catch return: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn close_failure_is_caught_before_explicit_finally_runs() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            static class Resource implements AutoCloseable {
                public void close() throws Exception {}
            }

            void test() {
                Object state = new Object();
                try (Resource resource = new Resource()) {
                } catch (Exception closeFailure) {
                    state = null;
                } finally {
                    state.toString();
                }
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["state"]);
}
