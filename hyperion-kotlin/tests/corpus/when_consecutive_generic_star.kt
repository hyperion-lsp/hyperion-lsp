package corpus.whenedge

// KNOWN PARSER GAP: two back-to-back `is GenericType<*> -> ...` branches
// in the same `when` corrupt the parse of the second branch (and
// everything after it), regardless of which generic types are involved or
// their order. A single `is List<*> -> ...` branch parses cleanly on its
// own (see the (isolated) `matchType` example in when_expressions.kt,
// which deliberately keeps only one generic-star branch to stay off this
// ratchet), but as soon as a second such branch immediately follows a
// first one, the parser reports "Expected ARROW" at the second branch's
// arrow. Minimal repro below has only two branches and already fails.
// Kept on the known-failures ratchet in mass_parse_test.rs pending a fix
// (likely in how the `is` type-check pattern's trailing `>` interacts
// with the next branch's leading token).

fun matchCollectionKind(value: Any?): String = when (value) {
    is List<*> -> "list"
    is Map<*, *> -> "map"
    else -> "other"
}

fun matchArrayOrList(value: Any?): String = when (value) {
    is List<*> -> "list"
    is Array<*> -> "array"
    else -> "other"
}
