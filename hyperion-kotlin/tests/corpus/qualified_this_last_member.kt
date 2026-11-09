package corpus.qualifiedthis

// KNOWN PARSER GAP: a labeled/qualified `this@Label` expression, used as
// the body of an expression-bodied function (`fun f() = this@Label...`)
// that happens to be the *last* member in its enclosing class body,
// causes the parser to overrun into the enclosing class's closing brace
// and report "Expected R_BRACE, found EOF" at end-of-file — i.e. one `}`
// too few gets consumed/matched somewhere in `this@Label` handling.
// Every other shape works fine and is used elsewhere in the corpus
// without issue: `this@Label` alone at top level, `this@Label` in a
// *block*-bodied function (`{ return this@Label.x }`), and an
// expression-bodied `this@Label` function that is *not* the last member
// (see nested_inner_classes.kt's `Inner.outerName`, rewritten to a block
// body specifically to stay off this ratchet). Kept on the
// known-failures ratchet in mass_parse_test.rs pending a fix to
// qualified-`this` expression parsing.

class Outer2(val name: String) {
    inner class Inner {
        fun outerName(): String = this@Outer2.name
    }
}
