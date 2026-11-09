# Java reference conversions

Reference conversion and overload selection share canonical declaration types.
Cast and construction expressions resolve nominal components through imports,
packages, and enclosing types. Array casts retain dimensions; intersection
casts retain every bound. Same-simple-name types in different packages remain
distinct.

Lexical type parameters retain their bounds, including finite references for
recursive bounds. Generic-supertype projection preserves actual type arguments
and applies invariance and wildcard rules. Raw supertypes retain Java's
unchecked-conversion behavior; a diamond constructor cannot change fixed
interface arguments.

Assignment and invocation use the shared array relation: reference covariance,
remaining dimensions, and widening to `Object`, `Cloneable`, or `Serializable`.
Primitive arrays cannot widen or box their individual elements.

Regression cases in `hyperion-lsp/tests/diagnostics/type_checking_tests.rs` and
`hyperion-lsp/tests/hover/overload_type_resolution_tests.rs` cover casts,
construction, bounds, generic supertypes, arrays, negative conversions, exact
navigation positions, and rendered overload signatures. Run the affected groups:

```sh
nice -n 10 cargo test -p hyperion-lsp --test diagnostics type_checking_tests -j 4
nice -n 10 cargo test -p hyperion-lsp --test hover overload_type_resolution_tests -j 4
```

For protocol validation, open a valid cast selecting a subtype overload, check
its definition and hover, change the cast to an incompatible type without
saving, then restore it and close/reopen the document. Check versioned semantic
diagnostics after indexing completes; a syntax-only publication does not prove
semantic analysis has completed. Java's conversion rules and `javac` provide
independent checks within the supported language scope.
