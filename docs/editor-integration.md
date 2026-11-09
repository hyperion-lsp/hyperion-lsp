# Editor integration

Configure an LSP client to start `hyperion-lsp` over stdio from the project
root. Use `settings.gradle[.kts]`, `build.gradle[.kts]`, or `pom.xml` as
root markers. Scala support is limited to the capabilities in the root README.

Hyperion uses incremental document synchronization and UTF-16 LSP positions.
It publishes versioned diagnostics and reports long-running workspace and JDK
indexing through work-done progress. Reproduce compatibility failures with protocol traces that include document
versions, requests, and responses.

## Run, test and debug

Hyperion describes how to launch; the client executes it. Hyperion never
spawns the build or the program for a client that uses this contract (the
"▶ Run" code lens is a separate, in-VM path that ignores it). Clients should follow the request and response contract below.

### `workspace/executeCommand` `hyperion.resolveLaunch`

Listed in `executeCommandProvider.commands`. One argument object:

```json
{ "uri": "file:///abs/Main.java",
  "position": { "line": 12, "character": 4 },
  "target": "auto" }
```

`position` (UTF-16, 0-based) is optional. `target` is `"auto"` (default),
`"main"` or `"test"`.

- `auto`: a test method or test class under the position wins; then a `main`
  method under the position; then the enclosing test class; otherwise the
  file's first entry point, and only when there is none, its first test class.
- `main`: Java `public static void main(String[])` (also in interfaces, enums
  and records, and with varargs), the JEP 512 forms (instance `main`, no-arg
  `main`, non-public `main`; reported as `Extended` with a JDK 25 warning),
  Kotlin top-level `fun main()`/`fun main(args: Array<String>)` in the file
  facade (`<File>Kt`, or the name from `@file:JvmName`), and `@JvmStatic fun
  main(args)` in an `object` or `companion object` (the companion's outer
  class). Nested classes are launched by binary name (`pkg.Outer$Inner`).
- `test`: methods annotated `@Test`, `@ParameterizedTest`, `@RepeatedTest`,
  `@TestFactory`, `@TestTemplate` (JUnit 4/5, TestNG, `kotlin.test`), test
  classes (any class with such a method, or a class-level TestNG `@Test`) and
  `@Nested` classes (`pkg.Outer$Inner`).

The result is `null` when the file has no entry point and no test for the
requested target (and for tests in a project with no Gradle/Maven build:
nothing can run them). Otherwise:

```json
{
  "name": "Main (app)",
  "kind": "main",                    // "main" | "test"
  "language": "java",                // "java" | "kotlin"
  "projectRoot": "/abs/root",        // LSP workspace root
  "moduleDir": "/abs/root/app",      // owning Gradle/Maven module
  "buildTool": "gradle",             // "gradle" | "maven" | "none"
  "build": {                         // null when nothing needs building
    "argv": ["/abs/root/gradlew", ":app:classes", "--console=plain"],
    "cwd": "/abs/root"
  },
  "launch": {                        // DAP `launch` arguments, verbatim
    "mainClass": "com.example.Main",
    "classpath": "/abs/app/build/classes/java/main:...:/jar1.jar",
    "args": [], "jvmArgs": [],
    "cwd": "/abs/root/app", "projectRoot": "/abs/root", "env": {}
  },
  "test": {                          // present iff kind == "test"
    "className": "com.example.FooTest",
    "methodName": "handlesEmpty",    // null: whole class
    "gradleTask": ":app:test",       // null for Maven
    "argv": ["/abs/root/gradlew", ":app:cleanTest", ":app:test", "--tests",
             "com.example.FooTest.handlesEmpty", "--console=plain"],
    "debugArgv": [".../gradlew", ":app:cleanTest", ":app:test", "--tests",
                  "com.example.FooTest.handlesEmpty", "--debug-jvm", "--console=plain"],
    "cwd": "/abs/root",
    "reportsDir": "/abs/root/app/build/test-results/test"
  },
  "warnings": ["classpath incomplete: 3 dependencies not in the local cache (e.g. org.acme:lib:1.0)"]
}
```

Rules:

- `classpath` uses the platform path separator and lists the module's output
  directories (Gradle `build/classes/java|kotlin/<set>` and
  `build/resources/<set>`; Maven `target/classes` / `target/test-classes`),
  then the outputs and resources of depended-on workspace modules, then the
  resolved runtime dependency jars. Test source sets put the module's `main`
  outputs after their own. Output directories are listed whether or not they
  exist yet: `build` creates them.
- `build.argv` prefers `./gradlew` only when `gradle/wrapper/gradle-wrapper.jar`
  exists (else `gradle` on `PATH`) and runs `:<path>:classes` (or
  `:<path>:<set>Classes`); the Gradle project path comes from
  `settings.gradle[.kts]` (`include`, `project(':x').projectDir`) and falls
  back to the directory layout. Maven uses `./mvnw` when `.mvn/wrapper` exists,
  else `mvn`, with `-q [-pl <module> -am] compile|test-compile`, run from the
  topmost reactor POM. With no build tool a Java file gets
  `javac -d <module>/out/production -sourcepath <source root> <file>` (jars in
  `<module>/lib` join the classpath); Kotlin gets `null` plus a warning.
  Tests need no separate build: `build` is `null` and `test.argv` builds.
- Gradle test argv starts with `:<path>:cleanTest` so an unchanged test is run
  again instead of being skipped as up to date. Maven test argv is
  `mvn -Dtest=<Class>#<method> [-pl <module> -am
  -Dsurefire.failIfNoSpecifiedTests=false -DfailIfNoTests=false] test`;
  `debugArgv` adds `-Dmaven.surefire.debug`. Both print
  `Listening for transport dt_socket at address: 5005` and wait for a debugger.
- For `kind: "test"` the `launch` object is informational (the test classpath);
  tests run through `test.argv`/`test.debugArgv`, not DAP `launch`.
- The command never fails for an incomplete model: it returns a best-effort
  classpath plus `warnings` (model still loading, dependencies not in the local
  cache, per-module data missing, instance `main` needing JDK 25+).
- `hyperion.runConfigurations` still returns IntelliJ configurations;
  Application entries now also carry `classpath`, `cwd` (the configured working
  directory, else the module directory), `projectRoot`, `moduleDir`,
  `buildTool`, `build` and `warnings`, resolved the same way from the source
  file of the main class.

### Client behaviour

- Run (no debugger) and Debug are both driven from this result. Run spawns
  `java <jvmArgs> -cp <classpath> <mainClass> <args>` in `launch.cwd` with
  `launch.env`; Debug sends DAP `launch` with the `launch` object.
- If `build` is non-null, run it first, asynchronously, stream its output to
  the same console, and abort on a non-zero exit.
- Tests: Run spawns `test.argv`; Debug spawns `test.debugArgv`, waits for the
  `Listening for transport dt_socket at address: N` line on stdout or stderr,
  then DAP `attach`es to `127.0.0.1:N`. JUnit XML in `test.reportsDir` carries
  the results.

### Debug adapter (`hyperion-lsp dap`)

- `launch` arguments: `mainClass`, `classpath`, `args`, `jvmArgs`, `cwd`,
  `projectRoot`, `env`. `attach`: `host`, `port`, `projectRoot`.
- Failed launches are never silent: when the JVM dies before the debugger
  connects, the failed `launch` response and an `output` event (stderr) carry
  what it printed. A bad main class is reported by the JVM after the VM
  resumes: stderr `output`, `exited` with code 1, `terminated`.
- End of session: remaining output, `exited` (`exitCode`, launch mode only),
  then exactly one `terminated`. `terminate` stops the debuggee;
  `disconnect` ends the session.
- Breakpoints bind by class `SourceFile` attribute and line table (Kotlin
  facades, nested/inner/anonymous/local classes, lambdas), including classes
  loaded later; a line without code slides to the next line with code inside
  the same type. `verified: false` means not bound yet (the client gets a
  `breakpoint` event when it binds).
- Evaluate (`repl`, `watch`, conditions): Java-like expressions with method
  calls (run on the stopped thread only, 5 s limit), arithmetic, comparison,
  logic, `?:`, string concatenation, `this`, fields, indexing and static
  members of loaded classes. `hover` never runs code.
- The adapter logs to `$LOG_DIR/hyperion-dap.log` (default: the system temp
  directory); `HYPERION_DEBUG=1` adds debug detail.

## Refactoring and file operations

Capabilities the client should advertise, and what Hyperion does with them:

| Client capability | Effect |
|---|---|
| `workspace.workspaceEdit.documentChanges` | Required for versioned edits: workspace rename, safe delete, move. |
| `workspace.workspaceEdit.resourceOperations` `rename` | Rename of a public class also renames its file; package rename renames directories; "Move to package" moves the file. Without it those refactorings are refused with a message. |
| `resourceOperations` `create` / `delete` | Extract interface writes `I<Class>.java` next to the class (otherwise it is a package-private type in the same file); safe delete of the last type in a file deletes the file. |
| `workspace.fileOperations.willRename` / `didRename` | `workspace/willRenameFiles` returns the edits (package declarations, imports, references; class rename for a renamed public class file) the client applies *before* moving the path. Directories are expanded to their Java/Kotlin files. |
| `workspace.didChangeWatchedFiles.dynamicRegistration` | Hyperion registers `**/*.java`, `**/*.kt`, `**/*.kts` and build files. Its own OS watcher stays the owner of change detection; client events are a fast path into the same idempotent reconciler (an already published disk revision is a no-op). |

Edits that touch a moved file address its *old* URI and precede the `RenameFile`, so open-document version guards stay valid. Edits to open buffers are not written to disk by the server; the client saves them.

## Troubleshooting

1. Start `hyperion-lsp` from the intended project root.
2. Send `didOpen` before `didChange`, with increasing document versions.
3. Encode positions as UTF-16 line and character pairs.
4. Reject stale diagnostics by document version.
5. Respond to dynamic capability registrations and work-done progress requests.
6. Install matching JDK and dependency source archives for source navigation.
