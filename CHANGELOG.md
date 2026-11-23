# Changelog

## 0.1.2

### Changed

- A library source opened through go-to-definition is a file named after its
  class (`…/ConcurrentHashMap.java`), not after a 64-character digest, so the
  editor's tab, status line and reference list are readable.
- Server logs older than a week are removed from the log directory at
  startup. Every start used to leave `hyperion-lsp-<pid>.log` behind for good.

### Fixed

- **Hover showed popups with nothing in them.** White space, keywords, empty
  lines and string literals answered "Symbol", and a call that could not be
  resolved answered "Identifier / Method call" followed by its own name. These
  now have no hover. Declarations still describe themselves.
- **Hover on a method called on a lambda parameter** (`user -> user.getName()`)
  found nothing when the parameter had no written type, and a use of such a
  parameter showed `<unknown> user`. Both now use the type inferred from the
  functional interface.

- Library sources opened through go-to-definition (an entry of a `-sources`
  jar or the JDK's `src.zip`, or a stub rendered from a class file) are no
  longer analysed. They used to fill the editor's problem list with warnings
  such as "Parameter 'value' is never used" for code the user cannot change.

## 0.1.1

### Fixed

- **Find references returned nothing for overloaded methods.** A call whose
  argument types could not all be inferred (a method call or a concatenation
  as an argument is enough) made the whole query fail whenever the method had
  another overload with the same number of parameters, and the editor showed
  "no references". References and call hierarchy now list such calls; exact
  queries used by refactorings still refuse them. A call with a different
  number of arguments than the overload no longer counts as ambiguous at all.
- **Find references on a variable passed as an argument** dropped the
  variable's declaration and every use that was not itself a call argument.
- Find references answers "still indexing, try again" instead of an empty
  list when the index is busy.
- **Inlay hints: unqualified calls had no parameter names.** `label("tea", 3)`
  inside the class that declares `label` got no hints; only `Shop.label(...)`
  and calls through a variable did.
- Inlay hints no longer show `arg0:`, `arg1:` for library methods compiled
  without parameter names (most of the JDK). Curated names are used where
  known, otherwise the argument is left alone.
- An inlay-hint range ending past the last line (how several editors say "the
  whole file" when the file has no trailing newline) returned no hints. Window
  ranges are now clamped to the document.

- Workspace discovery and source reads stay stable across file updates.
- Maven profiles are resolved with per-POM activation and override rules.
- Java `switch` expressions take their type from every correctly scoped
  `yield`.

### Performance

- Find references and the usage pass of indexing looked up the members of a
  JDK or library class by scanning every declaration of that library for each
  call (about 340,000 for the JDK). Members of library classes are now cached
  like those of workspace classes. On Apache Kafka (6,300 Java files), from a
  cold start, indexing finishes in 28 s instead of 3 min, and reference
  searches that took 8-37 s take 0.6-7 s (the slowest being a method named
  `close`, which over 1,300 files mention).
- The parallel reference scan no longer measures and frees cache entries
  while holding its cache lock.
- Inlay hints for a whole file built the file's symbol table again for every
  method call, re-read the line table for every hint and re-opened the JDK
  module archive for every call into the JDK. A 9,500-line file went from
  6.9 s to 1.4 s, while producing 875 hints instead of 486 (the unqualified
  calls above); for the same 486 hints it took 0.4 s.

## 0.1.0 — initial public release

- Java and Kotlin language server with completion, hover, navigation, rename,
  diagnostics, and semantic highlighting; Kotlin support remains partial.
- Gradle and Maven project modeling with offline dependency-cache resolution
  and an optional build-tool classpath query.
- JVM launch descriptions and a DAP/JDWP debugger for Java and Kotlin.
- Java formatting, refactoring support, and experimental security diagnostics.
- Prebuilt binaries for Linux and macOS (GitHub Releases, shell installer,
  Homebrew tap), source-build instructions, a VS Code extension and a Neovim
  plugin.
