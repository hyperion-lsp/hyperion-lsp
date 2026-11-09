package corpus.localclasses

// KNOWN PARSER GAP: a `class` declaration inside a function body (a local
// class) is not recognized by the block-statement parser at all —
// "Unexpected CLASS_KW while parsing block statement; skipping token" is
// reported on the `class` keyword itself, and the rest of the enclosing
// function body is corrupted by the ensuing single-token error recovery.
// Local *functions* (`fun` inside a function body) already work fine —
// see nested_inner_classes.kt's `demoLocalFunction`, kept off this
// ratchet — this gap is specific to local classes. Local classes are a
// less common but legal and occasionally-used Kotlin feature (e.g. a
// small helper type scoped to a single function). Kept on the
// known-failures ratchet in mass_parse_test.rs pending a block-statement
// parser fix.

fun demoLocalClass(): Int {
    class LocalCounter(var value: Int) {
        fun bump(): Int {
            value += 1
            return value
        }
    }

    val counter = LocalCounter(0)
    counter.bump()
    counter.bump()
    return counter.value
}
