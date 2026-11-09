package corpus.funinterfaces

// KNOWN PARSER GAP: `fun interface` (functional/SAM interface) declarations
// are not recognized at all — the top-level declaration dispatcher only
// expects `fun` to start a plain function declaration and has no lookahead
// for a following `interface` keyword, so it fails immediately on the
// simplest possible case (`fun interface Runner { fun run() }`). `fun
// interface` is the standard modern-Kotlin way to declare a SAM type for
// lambda conversion (stable since Kotlin 1.4) and is common in real
// codebases. Kept on the known-failures ratchet in mass_parse_test.rs
// pending a top-level-declaration parser fix.

fun interface Runner {
    fun run()
}

fun interface Transformer<T, R> {
    fun transform(value: T): R
}

fun interface Predicate2 {
    fun test(value: Int): Boolean
}

fun runIfPositive(value: Int, predicate: Predicate2, runner: Runner) {
    if (predicate.test(value)) {
        runner.run()
    }
}
