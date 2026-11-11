# Real-project audit, November 2026

This pass exercised project discovery, Maven profile selection and modern Java
switch expressions. These are bounded workflow checks, not a claim of general
Gradle/Maven or Java/Kotlin conformance.

## Reproduced problems and repairs

### Checkout location and changing files

A checkout at `~/build/checkout` produced an empty default source scan because
build-directory exclusions examined the absolute path. The same problem affected
`target`, `out`, `bin`, `dist` and `node_modules` ancestors. Startup discovery now
applies exclusions relative to the project boundary. Directory-create/rename
reconciliation carries that boundary explicitly, so a notification for
`checkout/build/classes` cannot bypass exclusions by making it the scan root.

The native watcher and client file notifications also disagreed with startup:
they admitted `bin`, `dist` and `node_modules` sources. Both now use the scanner's
exclusion rules. Generated-source exemptions are relative to the workspace too;
an ancestor named `build/generated` no longer exempts unrelated build outputs.

The real server-process check opens a checkout under `build/generated`, queries
symbols without opening a document, edits a source on disk, and restarts the
server. It requires the same single source identity throughout, with no output
duplicates. This uncovered a second defect: reading source content after cache
restore/eviction attempted an authoritative index write and panicked in debug
builds. Exact byte-cache refills now retain revision/path/hash validation without
requiring a semantic publication transaction. Tests cover stale disk bytes and
concurrent publications as well as successful refills.

### Maven profiles

Profiles without activation and profiles declaring `activeByDefault=false` were
applied automatically. A default development profile also remained active when
another profile in the same POM matched, and multiple activation conditions
overwrote one another instead of being combined. These errors changed dependency
versions, classpaths and properties for ordinary multi-environment projects.

Implicit activation is now separate from the default fallback, selected per POM.
All modeled activation conditions must match. Unsupported conditions remain
inactive and preserve model-completeness blockers. Parent POMs contribute their
modeled content while propagating structural limitations to children.
Profile dependencies replace matching artifact variants before dependency
management supplies omitted attributes; appending them previously allowed the
base dependency to win instead. Selection and override behavior were compared
with Maven 3.9.11's active-profile and effective-POM output. Incomplete JDK range
conditions such as `[` also now return false instead of panicking while a POM
is being edited.

### Java switch expressions

Only the first `yield` in a block contributed to inference, while yields in
nested switch expressions could contaminate the enclosing result. Thus a String
result could become Object and an int/long result could become int. Resolver and
expression consumers also disagreed when one branch was unresolved.

A shared result-expression traversal now respects yield ownership and includes
every result. Unknown branches remain unknown. Switch-arm local declarations are
processed before inferring the enclosing `var`, with sibling scopes isolated.
Semantic and LSP navigation regressions cover arrow and colon forms, nested
switch statements/expressions, local classes, throw arms and unresolved results.
Representative String and long results were checked with `javac` and `javap`.

### Reliable regression results

The broad test run exposed shared JDK-cache contamination in the LSP harness.
Cold indexing cached the fixture's elected symbols, so a fixture declaring its
own `java.lang.String` could remove real String members from subsequent tests.
The harness now extracts the shared JDK snapshot in an independent index, and
the test-cache namespace retires previously contaminated snapshots. A subprocess
contract checks fixture shadowing and actual cold, memory and disk cache paths.
Production's separate canonical refresh snapshots already avoid this defect.

## Repeating validation

The final affected-crate run passed all **6,077 tests**, with 10 opt-in heavy
tests skipped. Workspace-wide clippy and formatting, repository gates, and all
10 watcher tests with real filesystem watching enabled also passed. The final
run includes the real server startup/edit/restart and JDK-cache regressions.

```sh
cargo nextest run -p hyperion-build -p hyperion-workspace -p hyperion-semantics -p hyperion-lsp --locked --no-fail-fast -j 2
HYPERION_FS_WATCH_TEST=1 cargo nextest run -p hyperion-lsp --lib -E 'test(file_watcher::tests::)' -j 2
cargo clippy --workspace --all-targets --locked -j 2 --no-deps -- -D warnings
cargo fmt --all -- --check
bash scripts/check-headers.sh --all
bash scripts/check-test-bodies.sh
bash scripts/check-build-test-relevance.sh
```

Heavy ignored tests require the CLI option `--profile heavy --run-ignored all`;
`run-ignored` is not a supported nextest profile configuration key.

## Remaining coverage limits

Default scanning still treats package path segments named `build` or `target`
as outputs. Correctly distinguishing those from generated/build directories
requires source-root-aware classification shared by discovery and live updates.
Explicit Maven profile selection through `-P` or settings remains unmodeled.
Kotlin feature gaps and build-model approximations documented in the README
remain; this pass does not establish production readiness for those workflows.
