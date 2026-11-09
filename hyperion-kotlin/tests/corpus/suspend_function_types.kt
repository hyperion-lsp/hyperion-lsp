package corpus.suspendtypes

// KNOWN PARSER GAP: `suspend` as a modifier prefixing a function *type*
// (`suspend () -> T`, as opposed to `suspend fun` on a declaration, which
// works fine) is not recognized by `parse_type_ref`/`parse_parameter`. In
// parameter position this produces a hard, reported parse error (the
// bogus partial type consumes just the `suspend` identifier and leaves
// `() -> T` dangling, so the enclosing parameter list never finds its
// closing paren). This is a mainstream, load-bearing coroutines idiom
// (`block: suspend () -> Unit`, `noinline action: suspend (T) -> R`, the
// exact shape used by kotlinx.coroutines' own `launch`/`async`/`Flow`
// APIs). Kept on the known-failures ratchet in mass_parse_test.rs pending
// a `SUSPEND_KW` arm in the type parser.

fun higherOrderSuspend(block: suspend () -> Unit) {
}

fun higherOrderSuspendWithArg(block: suspend (Int) -> Boolean) {
}

class SuspendCallbackHolder(val callback: suspend (String) -> Unit)
