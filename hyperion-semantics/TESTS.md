# Semantics testing strategy

The semantics suite should answer one question: if parsing, indexing or
resolution changes, which user-visible language behaviors can no longer be
trusted?

Test count and line coverage are not goals. Prefer a smaller regression that
would fail for the original bug over tests of constructors, derived traits or
private implementation steps.

## Test layers

### Local invariants

Unit tests under `src/` cover algorithms whose contracts are useful in
isolation: scope lookup, substitution, assignability, overload ranking and
nullability transfer functions.

These tests should assert outputs and invariants. A test that only constructs a
database, parses text or prints a result is not a test.

### Semantic behavior

Integration tests under `tests/` provide source snippets and assert resolved
symbols, inferred types, diagnostics or invalidation behavior. This is the
preferred layer for regressions in inference and resolution because it survives
internal refactoring.

Name regression tests after the behavior or bug they protect. Include both the
positive case and the nearest plausible false positive when that distinction
matters.

### Properties

The `proptest_inference` and `proptest_resolver` targets exercise invariants
across generated inputs. Properties should state semantic rules—termination,
substitution consistency, scope isolation—not duplicate a table of examples.

### Cross-crate behavior

Protocol behavior belongs in `hyperion-lsp/tests`, where tests can exercise the
real request path. Corpus and differential checks that require a JDK or large
third-party checkout remain explicit heavy tests rather than silently skipping
inside the default semantics suite.

## Running tests

```bash
# This crate, including doctests
cargo test -p hyperion-semantics

# One integration target
cargo test -p hyperion-semantics --test proptest_resolver

# One regression by name
cargo test -p hyperion-semantics nested_generic_method_resolution -- --exact

# Repository-wide default suite
cargo test --workspace
```

Use `RUST_BACKTRACE=1` for a failing panic and `-- --nocapture` only when the
test intentionally emits diagnostic context.

## Review checklist

- Does the test fail before the fix?
- Does it assert an externally meaningful result or a durable invariant?
- Would a harmless internal refactor leave it green?
- Is the failure message specific enough to diagnose the regression?
- Does it cover the relevant negative case?
- Is an expensive corpus test kept out of the fast default tier and documented
  with an exact command?

Do not commit TODO-only test bodies, print-only probes, or tests whose only
assertion is that a derived `Clone`, `Eq` or field assignment works.
