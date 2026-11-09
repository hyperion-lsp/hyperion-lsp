# Hyperion

**A Java and Kotlin language server, written in Rust.**

One native binary. It resolves Gradle and Maven projects *without running
Gradle*, holds Java and Kotlin in a single shared symbol index, and carries its
own JVM bytecode interpreter — which is how it reads Gradle plugin bytecode,
and how it runs your `main` method without starting a JVM.

> **Status: development release, v0.1.0.** Java is usable for day-to-day work.
> Kotlin is behind it and you will notice. There is no packaged distribution
> and no documented third-party production deployment yet.
> [What works today](#what-works-today) is deliberately blunt — read it before
> you install, and read [known rough edges](#known-rough-edges) before you
> recommend it to anyone else.

**Cost and licensing, up front:**

- Source-available under the [PolyForm Shield License 1.0.0](LICENSE).
- **Free for any use except building a competing product,** including
  commercial and production use at your job. No feature is paywalled, time-limited
  or disabled.
- Offering a product that competes with Hyperion (a rival language server,
  or a product built on Hyperion that competes with it) requires a separate
  commercial license. Read the `LICENSE` for the
  precise boundary.

---

## Why this exists

Editing Java outside IntelliJ means Eclipse JDT Language Server, and it is
Java-only. Add Kotlin to the build and you are running a second server with a
second index that does not know your Java code exists — so every Kotlin call
into a Java class in the same module is a guess.

Hyperion bets the fix is one process holding one index, with no JVM in the hot
path. The server is a native binary that reads `.class` files and JARs
directly. Java and Kotlin resolve against the same symbol table, so
cross-language navigation is a lookup rather than a heuristic. Formatter, debug
adapter and security scanner are in the same binary instead of being three more
things to install — all three Java-only today.

Whether the bet has paid off is what the rest of this page is for.

## The bytecode interpreter

`hyperion-bytecode` is the part of this repository that does not exist
elsewhere: a JVM bytecode interpreter in Rust that loads and executes real
`javac` output in-process, with no JVM.

It earns its keep twice. It reads Gradle plugin bytecode during project
resolution, which is what lets Hyperion model your build without ever executing
`gradlew`. And it backs the "▶ Run" code lens — `javac` still produces the
bytecode, but nothing spawns `java` to run it.

Correctness is checked differentially against a real JVM: a corpus of 33
programs — streams, records, lambdas, enums, generics, try-with-resources,
switch expressions — whose output must match byte for byte. That is a
smoke-level guarantee, not a conformance suite. The test runs in the normal
suite when `javac` and `java` are on `PATH`, and self-skips otherwise:

```bash
cargo test -p hyperion-bytecode --test jvm_differential_test -- --nocapture
```

It implements a useful subset of the JVM. It is not a conformant JVM and should
not run your production workload.

---

## What works today

"Yes" means implemented and covered by tests. It does not mean it matches
IntelliJ.

### Java

Java is the primary language, and the one the author uses daily.

| | |
|---|---|
| Goto-definition, find references, hover | Yes — cross-file, and into JDK and library sources |
| Completion | Yes, including member completion after `.` |
| Diagnostics | Parse errors, unresolved symbols, javac-style compile errors (missing methods/fields/constructors, wrong arguments, missing return, unreachable code, unreported checked exceptions, access, unimplemented abstract methods, `final` assignment, switch exhaustiveness, ...), nullability analysis, 12 inspections. An error is reported only when it is provable from a fully resolved model; anything uncertain (incomplete classpath, unresolved supertypes, generic inference) stays silent |
| Rename | Yes, symbol-aware and cross-file (Java and Kotlin), through imports and Javadoc `{@link}`/`@see`/`@throws` references. A public top-level class also renames its file, and a package segment renames the package (directories, declarations, imports); both use `documentChanges` resource operations, so the client must support them. `{@link #member}` mentions and KDoc references are not rewritten |
| Move / file operations | "Move to package" code action and `workspace/willRenameFiles` (files and directories moved or renamed in the client's explorer) fix package declarations, imports and qualified references in Java and Kotlin; renaming a public class file renames the class. `workspace/didChangeWatchedFiles` (registered dynamically) and `didCreate`/`didRename`/`didDelete` share one reconciler with the built-in file watcher |
| Document & workspace symbols, semantic tokens, inlay hints, signature help | Yes |
| Code actions | Quick fixes (import candidates ranked JDK-first, never JDK internals or members; "Add all missing imports"), source generation that respects Lombok and existing members, organize imports (sorts, groups and adds unambiguous missing imports), extract interface (own file when the client can create files), safe delete and move. Extract method/variable actions are present but disabled because safe edits are not implemented |
| Call hierarchy, folding, document links | Yes |
| Code lens | "▶ Run" above `public static void main` (Java only). It runs the class in Hyperion's in-process bytecode VM, not on a real JVM, and its `className` is the fully qualified binary name. Real JVM runs come from `hyperion.resolveLaunch` (below) |
| Run and test launch descriptions | `hyperion.resolveLaunch` resolves a file position to a ready-to-run description: Java static/instance `main`, Kotlin top-level `main` (`<File>Kt`/`@JvmName`), `@JvmStatic` in objects/companions, nested classes; JUnit 4/5, TestNG and `kotlin.test` tests (method, parameterized, `@Nested`); module directory, classpath (module outputs plus resolved dependency jars), a pre-launch build command and Gradle/Maven test commands. Hyperion only describes the launch: the client spawns the build and the JVM. Contract: [docs/editor-integration.md](docs/editor-integration.md) |
| Lombok | Partial — `@Data`, `@Getter`/`@Setter`, `@Builder`, the `*ArgsConstructor` family, `@Accessors` and the logger annotations (`@Slf4j`, `@Log4j2`, `@CommonsLog`, `@Log`, …, which declare `log`) produce synthetic members in the index. `@With`, `@SneakyThrows`, `val`/`var`, `lombok.config` and other annotations are not modelled |
| Debugging | DAP/JDWP launch and attach. Line and conditional breakpoints bind by `SourceFile` attribute and line table, so they work in Kotlin (top-level functions, classes, lambdas) and in Java nested, inner, anonymous and local classes, including classes that load later. Stepping, stacks, exception breakpoints, `exited` + one `terminated`, `terminate`. Variables show the runtime type with summaries (strings, boxed values, collection sizes and elements, maps, `toString()`); evaluate runs method calls in the debuggee plus arithmetic, comparison, concatenation, `this` and static members. Not supported: hit counts, logpoints, set-variable, hot code replace, expression side effects such as assignment, evaluating Kotlin-only syntax. It does not build your project: the client compiles first |
| Formatting | Partial — targets [palantir-java-format](https://github.com/palantir/palantir-java-format) style but is not byte-identical, so it will not satisfy a `spotlessCheck` pinned to palantir |
| Security scanning | Experimental — 25 taint-analysis check families (injection, deserialization, XXE, SSTI, zip-slip, JWT, access control, …) surfaced as diagnostics. No published false-positive rate; treat it as a hint generator, not a gate |
| Type hierarchy | Yes — dynamically registered for clients that advertise type-hierarchy registration support |

### Build systems

This is where the "no Gradle daemon" bet gets tested, so it is worth being
specific. Modelled today: **Gradle** (Groovy and Kotlin DSL), **version
catalogs** (`libs.versions.toml`, including alias normalisation), **`buildSrc`**,
**composite builds** (`includeBuild`), convention and binary plugins via
bytecode inspection, multi-module projects, **Maven** (including parent POMs and
BOM imports), generated sources from annotation processors/KAPT/KSP and other
build generators. Nested git repositories are indexed.

**Dependencies come from your local caches, not from the network.** Hyperion
reads `~/.gradle/caches/modules-2` and `~/.m2/repository`; it never downloads
anything and, by default, never runs Gradle or Maven. On a fresh clone the
caches are empty, so no library resolves until you build the project once
(`./gradlew build`, `mvn dependency:resolve`) or opt in as described below.

What the resolver models: effective POMs (parent chains, properties,
`dependencyManagement`, imported BOMs), Gradle Module Metadata (separate API and
runtime variants, platform constraints, `available-at` redirects), Spring's
`io.spring.dependency-management` BOM, Gradle's highest-version conflict rule,
Maven's nearest-wins rule, and the Kotlin plugin's implicit `kotlin-stdlib`.
Main and test source sets see their own JARs, and a module sees its project
dependencies' exported JARs. Approximations to know about: one dependency graph
per module (a test-only dependency can raise the version of a library the main
classpath also uses), no dependency substitution, locking, resolution
strategies or `strictly` rejections, and Maven profiles in *library* POMs are
ignored. Definition into a library uses its `-sources.jar` in the cache when
available; otherwise Hyperion generates signature-only Java stubs from the
compiled classes. These stubs provide navigation targets, without method bodies.

Dependencies that cannot be resolved (not in the cache, unreadable POM, no
version) are reported by exact coordinate — in the log, in a
`window/showMessage`, and in the progress message — and only those libraries are
missing; everything else is indexed. Rename is refused only when the build
*structure* is not understood (unparsed modules, unmodelled plugin
declarations), not because a library is missing.

**Opt-in: let Hyperion ask the build tool.** Set `hyperion.buildToolClasspath`
to `true` through the client's initialization options
(`{"hyperion": {"buildToolClasspath": true}}`), `workspace/didChangeConfiguration`,
or the environment variable `HYPERION_BUILD_TOOL_CLASSPATH=1`. Hyperion then runs
`./gradlew` (or `gradle`) with a temporary init script that prints every source
set's resolved classpath, or `mvn dependency:build-classpath` for Maven, and
uses that exact answer, downloading whatever is missing. **This executes the
project's build scripts and plugins: arbitrary code from the repository.** It is
off by default and cannot be turned on from a file inside the repository;
`hyperion.buildToolClasspath` is the only switch that makes Hyperion execute
the build tool (without it, Gradle scripts are only interpreted in a sandbox).
The `hyperion.reloadProject` command re-reads the model (and re-runs the build
tool when enabled). Edits to `build.gradle(.kts)`, `settings.gradle(.kts)`,
`gradle.properties`, `libs.versions.toml` and `pom.xml` refresh the model
automatically.

### Kotlin

Kotlin has real native implementations, not the Java path with a different file
extension: goto-definition, find references, hover, document symbols, semantic
tokens, rename, signature help, inlay hints, and call and type hierarchy are
all Kotlin-specific code.

**If Kotlin is your main language, Hyperion is not ready for you.** Member
completion now follows inferred lambda parameters and same-type `when` results,
including members declared in imported Java classes, but remains behind the Java
path overall. Diagnostics are parse errors, unresolved symbols, a Kotlin type checker
and 4 inspections against Java's 12 (plus its compile-error checks).

Not yet implemented for Kotlin: **formatting** (there is no Kotlin formatter at
all — `ktfmt` equivalence is not started), **nullability analysis**, **security
scanning**, **goto-implementation**, folding, code lens, document links and
code actions. Java-only providers explicitly decline Kotlin documents rather
than interpreting Kotlin syntax as Java.

### Groovy and Scala

Along for the ride, not first-class. Groovy exists mostly so Gradle works —
hover, goto-definition, Gradle DSL completion and workspace symbols, including
inside `build.gradle`. Scala is hover and goto-definition over tree-sitter
symbol extraction and name matching, with no scope, type or import resolution;
enough to jump between Java and Scala in a mixed build. Both languages provide
CST-based semantic highlighting for declarations, calls, parameters, literals,
keywords and comments. For real Scala, use
[Metals](https://scalameta.org/metals/).

### Known rough edges

- **Cold requests and memory use need measurement on your project.** No current
  comparative benchmark is shipped. See [`docs/performance.md`](docs/performance.md)
  for reproducible methods and measurement limits.
- **"▶ Run" runs `main` methods only, in Hyperion's bytecode VM.** Hyperion has
  no test runner: `hyperion.resolveLaunch` returns the Gradle/Maven test command
  for a client to spawn, and there is nothing to run a test with when the
  project has no build tool. Legacy `hyperion.runTest` commands from cached
  clients are rejected explicitly.
- **No packaged distribution.** No Homebrew tap, no Marketplace listing, no
  crates.io release. Build from source.

---

## Try it

Needs a current stable Rust toolchain (tested with Rust 1.99.1), a C toolchain
and platform development headers (for example, Linux build-essential or macOS
Xcode Command Line Tools). A JDK on `PATH` or via `JAVA_HOME` supplies runtime
standard-library support (the benchmarks use 21). This is a large workspace
and the first release build is not quick; measured Linux x86_64 binaries were
about 53 MB unstripped or 41 MB stripped. Size varies by toolchain and target.

```bash
git clone https://github.com/hyperion-lsp/hyperion-lsp.git
cd hyperion-ls
nice -n 10 cargo build --locked --release -p hyperion-lsp --bin hyperion-lsp -j 4
```

To install the built server locally, run `scripts/build-hyperion-ls.sh release`.
It installs to `$HOME/.local/bin`; set `HYPERION_BIN_DIR` to choose another
destination. Set `HYPERION_INSTALL_LEGACY_ALIAS=1` only if your configuration
requires the `hyperion-ls` alias. No extra linker is required; Linux users with
`lld` installed can opt in using the example in `.cargo/config.toml`.

The binary lands at `target/release/hyperion-lsp`. With no arguments it speaks
LSP over stdio; it also has subcommands:

```bash
hyperion-lsp                    # LSP server over stdio
hyperion-lsp format Foo.java    # format Java files in place (--check / --list do not write)
hyperion-lsp dap                # Debug Adapter Protocol server over stdio
```

### Neovim

[`editors/nvim`](editors/nvim) is a Neovim plugin (also published as
[`hyperion-lsp/hyperion.nvim`](https://github.com/hyperion-lsp/hyperion.nvim)).
It needs Neovim 0.11 or newer and adds the server's `vim.lsp` config, a
`:Hyperion` command (reload, run, debug, code lens, log), optional
[nvim-dap](https://github.com/mfussenegger/nvim-dap) debugging, and
`:checkhealth hyperion`. Put `hyperion-lsp` on your `PATH`, then with lazy.nvim:

```lua
{ 'hyperion-lsp/hyperion.nvim', opts = {} }  -- opts: server_path, extra_filetypes, ...
```

or with `vim.pack` (Neovim 0.12):

```lua
vim.pack.add({ 'https://github.com/hyperion-lsp/hyperion.nvim' })
vim.lsp.enable('hyperion')
```

See [`editors/nvim/README.md`](editors/nvim/README.md) for the options and
commands.

Without the plugin, Neovim 0.11 or newer needs only this configuration:

```lua
vim.lsp.config('hyperion', {
  cmd = { '/path/to/hyperion-lsp' },
  filetypes = { 'java', 'kotlin', 'groovy' },  -- add 'scala' only for the thin support described above
  root_markers = {
    { 'settings.gradle', 'settings.gradle.kts' },
    '.git',
    { 'pom.xml', 'build.gradle', 'build.gradle.kts' },
  },
})
vim.lsp.enable('hyperion')
```

### VS Code

The extension source is in [`editors/vscode`](editors/vscode). It is not on the
Marketplace. It requires VS Code 1.82 or newer and registers `.kt`/`.kts`
files as Kotlin; build it locally:

```bash
cd editors/vscode
npm install && npm run compile
npx @vscode/vsce package   # produces a .vsix you can install
```

Put `hyperion-lsp` on your `PATH`, or point `hyperion.serverPath` at it. The
extension also registers a `hyperion` debug adapter for launching and attaching
to Java processes.

### Operating it

- **First run** does a full workspace index plus background indexing of JDK
  bytecode from JMODs or `rt.jar`; `src.zip`, when present, supplies source
  navigation. Clients advertising `window.workDoneProgress` receive registered
  `$/progress` notifications. On a large multi-module project this takes a while.
- **Later runs** reuse persistent caches, and are substantially faster. The
  workspace cache is `<16-hex>-hyperion.bin` in the system temp directory
  (`$TMPDIR`, or `/tmp`). To force re-indexing, stop the server and delete only
  that workspace's cache file. Cache schema changes invalidate it automatically.
  The separate JDK cache lives in the platform cache directory under
  `hyperion/jdk-<major>/` (on Linux, `$XDG_CACHE_HOME/hyperion/jdk-<major>/`,
  normally `~/.cache/hyperion/jdk-<major>/`). Remove that directory with the
  server stopped to rebuild the JDK cache.
- **Logs** go to `$TMPDIR/hyperion-lsp-<pid>.log` (or the platform temp directory
  when `TMPDIR` is unset). `LOG_FILE` overrides the directory, not the filename;
  an unwritable location falls back to stderr. Set `HYPERION_DEBUG=1` for debug
  logging. Logs never go to stdout.

---

## Performance

**No current, verified comparison against jdtls exists, so this README makes no
comparative performance claim.** See [`docs/performance.md`](docs/performance.md)
for how to measure Hyperion on your own projects.

---

## Kotlin parser conformance

The table below is **generated, and a test enforces it**: `hyperion-kotlin`'s
`readme_matrix_test` re-derives it from the checked-in corpus and fails the
build if this README and the measurement disagree. That is the standard every
claim above is being moved toward.

Caveats worth stating: it measures the *parser*, and the corpus is
hand-written, so 37/37 means "the parser handles the constructs we wrote tests
for" — a floor, not a coverage figure, and not a statement about semantics.

<details>
<summary>Generated corpus results</summary>

<!-- KOTLIN_MATRIX_START -->

<!-- Generated by hyperion-kotlin/tests/readme_matrix_test.rs from hyperion-kotlin/tests/corpus/ and tests/support/mod.rs::KNOWN_FAILURES. Do not hand-edit this block: fix the parser or the corpus, update KNOWN_FAILURES, then run `cargo nextest run -p hyperion-kotlin --test readme_matrix_test` and paste its failure diff here. -->

Measured from `hyperion-kotlin/tests/corpus/` (37 original Kotlin files, 2483 lines) via `cargo nextest run -p hyperion-kotlin --test mass_parse_test`: **37/37 files parse cleanly** (zero errors, lossless round-trip); 0 known parser gaps tracked below as a ratchet (see `tests/support/mod.rs::KNOWN_FAILURES`).

| Corpus file | Coverage area | Status |
|---|---|---|
| `annotation_classes.kt` | Annotation Classes | ✅ Clean |
| `classes_constructors.kt` | Classes Constructors | ✅ Clean |
| `collections_functional.kt` | Collections Functional | ✅ Clean |
| `control_flow_exceptions.kt` | Control Flow Exceptions | ✅ Clean |
| `data_classes.kt` | Data Classes | ✅ Clean |
| `delegation.kt` | Delegation | ✅ Clean |
| `destructuring.kt` | Destructuring | ✅ Clean |
| `dsl_builders.kt` | Dsl Builders | ✅ Clean |
| `enum_classes.kt` | Enum Classes | ✅ Clean |
| `extension_functions.kt` | Extension Functions | ✅ Clean |
| `fun_interfaces.kt` | Fun Interfaces | ✅ Clean |
| `generics_variance.kt` | Generics Variance | ✅ Clean |
| `infix_calls.kt` | Infix Calls | ✅ Clean |
| `infix_functions.kt` | Infix Functions | ✅ Clean |
| `interfaces_defaults.kt` | Interfaces Defaults | ✅ Clean |
| `lambdas_and_receivers.kt` | Lambdas And Receivers | ✅ Clean |
| `local_classes.kt` | Local Classes | ✅ Clean |
| `loops_non_block_bodies.kt` | Loops Non Block Bodies | ✅ Clean |
| `nested_inner_classes.kt` | Nested Inner Classes | ✅ Clean |
| `nullable_chains.kt` | Nullable Chains | ✅ Clean |
| `nullable_receiver_extensions.kt` | Nullable Receiver Extensions | ✅ Clean |
| `object_literals.kt` | Object Literals | ✅ Clean |
| `objects_companions.kt` | Objects Companions | ✅ Clean |
| `operator_overloading.kt` | Operator Overloading | ✅ Clean |
| `properties_accessors.kt` | Properties Accessors | ✅ Clean |
| `qualified_this_last_member.kt` | Qualified This Last Member | ✅ Clean |
| `reified_inline.kt` | Reified Inline | ✅ Clean |
| `scope_functions.kt` | Scope Functions | ✅ Clean |
| `sealed_classes.kt` | Sealed Classes | ✅ Clean |
| `string_templates.kt` | String Templates | ✅ Clean |
| `suspend_coroutines.kt` | Suspend Coroutines | ✅ Clean |
| `suspend_function_types.kt` | Suspend Function Types | ✅ Clean |
| `typealiases.kt` | Typealiases | ✅ Clean |
| `use_site_variance.kt` | Use Site Variance | ✅ Clean |
| `visibility_modifiers.kt` | Visibility Modifiers | ✅ Clean |
| `when_consecutive_generic_star.kt` | When Consecutive Generic Star | ✅ Clean |
| `when_expressions.kt` | When Expressions | ✅ Clean |

<!-- KOTLIN_MATRIX_END -->

</details>

---

## Contributing

```bash
nice -n 10 cargo check -p hyperion-lsp --tests -j 4
nice -n 10 cargo clippy -p hyperion-lsp --all-targets -j 4
```

Run checks for the crates you change, one Cargo command at a time. The local
verification script requires Bash and Python 3.11 or newer.
`scripts/verify.sh` is the comprehensive local gate for headers, formatting,
linting, and tests. It runs packages sequentially with four build jobs.

For a quick local check, `scripts/verify.sh --fast` runs formatting, all-target
linting, relevance checks that reject empty Rust tests and `hyperion-build`
integration targets which never reference the library or a built binary, the
vendored tower-lsp transport contracts, and a small LSP contract suite covering
document updates, completion, navigation, rename, language gating, and
Kotlin-to-Java member completion. Run `scripts/lsp-contracts.sh` when only those
editor contracts are needed.

<details>
<summary>Crate map</summary>

| Crate | What it is |
|---|---|
| `hyperion-lsp` | The server: protocol, request handlers, project integration |
| `hyperion-bytecode` | JVM bytecode interpreter, including native JDK emulation |
| `hyperion-security` | Taint-analysis security scanner (sources, sinks, sanitizers, interprocedural) |
| `hyperion-semantics` | Symbol tables, scopes, type system, inference |
| `hyperion-workspace` | Cross-file indexing, symbol queries, Lombok synthesis |
| `hyperion-parser` | Java lexer and parser producing a lossless CST |
| `hyperion-build` | Gradle and Maven project models; dependency resolution |
| `hyperion-kotlin` / `hyperion-groovy` / `hyperion-scala` | Per-language frontends |
| `hyperion-fmt` | Java formatter (palantir-java-format style) |
| `hyperion-dap` / `hyperion-jdwp` | Debug adapter and JDWP client |
| `hyperion-eval` | Expression and snippet evaluation on top of the interpreter |
| `hyperion-types` / `hyperion-actions` / `hyperion-refactor` | Shared type contracts, code actions, and refactoring support |

Built on [rowan](https://github.com/rust-analyzer/rowan) for lossless syntax
trees, [salsa](https://github.com/salsa-rs/salsa) for incremental
recomputation, and [tower-lsp](https://github.com/ebkalderon/tower-lsp) for the
protocol layer.

</details>

Bug reports and pull requests are welcome. Contributions are covered by the
[CLA](CLA.md). Please confirm agreement in your first contribution. Changes
are recorded in [CHANGELOG.md](CHANGELOG.md).

Hyperion is written and maintained by Adrian Helvik.

---

## License

Hyperion is **source-available, not open source**. It is licensed under the
[PolyForm Shield License 1.0.0](LICENSE).

In plain terms: you may use, modify and redistribute Hyperion **for any
purpose, including commercial and production use**, at no cost. Individuals in
any editor, companies using it for their own development, and open source
projects integrating with it are all fine.

The one thing you may not do is provide a product that competes with
Hyperion: a rival language server or IDE tooling built from it, or a hosted
service that offers its functionality. The LICENSE text is the authority;
this paragraph is a summary.

For commercial licensing, open an issue or contact the author via GitHub.
