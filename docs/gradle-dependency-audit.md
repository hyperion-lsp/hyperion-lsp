# Offline Gradle dependency modeling

`GradleParser` builds the static model and records unsupported syntax.
`GradleResolver` resolves installed artifacts and POM edges without downloading
missing dependencies. `GradleClasspathBuilder` preserves compile, runtime, API,
and test visibility. `GradleIntegration` aggregates module classpaths and stages
archive/source-provider publication under refresh tickets. An incomplete model
cannot authorize execution or an authoritative external index.

Each resolution owns its graph queue, results, artifact lookups, and parsed-POM
cache. Traversal is breadth-first and bounded at 64 transitive levels and 16,384
visited/pending contexts, with scope and exclusion context retained. Cycles and
shared dependencies do not discard graph edges. Metadata is re-read for later
resolutions so projects and refreshes remain independent.

Artifact coordinates are validated before constructing paths. POMs stay with
the cache that supplied the binary; ambiguous checksum variants fail rather
than depending on directory order. Installed versions alone do not establish
which dynamic version Gradle selected. `GRADLE_USER_HOME` selects the cache for
both resolution and bytecode sandboxes.

## Limits and troubleshooting

This is a static resolver, not Gradle's full resolution engine. Computed build
logic, unsupported variants, unavailable versions, incomplete BOM/POM metadata,
conflicts, missing artifacts, and traversal limits can leave a model incomplete.
The separate wrapper dependency-report API rejects unsupported/project entries
rather than returning a silently partial classpath.

The LSP combines module, syntax, plugin, VM, and dependency outcomes into a
bounded summary, with detailed causes in logs. Skipped candidate archives are
reported separately from publication failures. Refresh clears prior reasons.
For missing libraries, build the project once to populate local caches or use
`hyperion.buildToolClasspath` as documented in the root README. That opt-in
executes the project's build scripts. Source navigation requires the matching
sources archive; bytecode metadata alone supports hover and completion.

## Regression coverage

`hyperion-build/tests/gradle_dependency_resolution_chain_tests.rs` covers chains,
cycles, scopes, refreshes, invalid coordinates, ambiguous artifacts, version
guesses, shared graphs, and incomplete metadata:

```sh
nice -n 10 cargo test -p hyperion-build --test gradle_dependency_resolution_chain_tests -j 4
```

See [Gradle dependency caching](https://docs.gradle.org/current/userguide/dependency_caching.html)
and [dependency declarations](https://docs.gradle.org/current/userguide/declaring_dependencies_basics.html)
for the upstream concepts. Static modeling does not guarantee parity with every
Gradle plugin or resolution strategy.
