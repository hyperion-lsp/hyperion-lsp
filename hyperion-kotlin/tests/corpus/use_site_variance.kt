package corpus.usesitevariance

// KNOWN PARSER GAP: use-site variance projections (`out`/`in` written
// inside a type-argument list, e.g. `Array<out Any>`) are not accepted at
// all — `parse_type_arguments` has no arm for `out_KW`/`in_KW` before a
// type argument, only `parse_type_parameters` (declaration-site variance,
// e.g. `class Box<out T>`) handles them. Use-site variance is a
// mainstream, textbook Kotlin feature (the canonical `Array<out T>`
// covariant-read / `Array<in T>` contravariant-write "PECS" pattern) and
// is distinct from the declaration-site variance already covered cleanly
// in generics_variance.kt. Kept on the known-failures ratchet in
// mass_parse_test.rs pending a `parse_type_arguments` fix.

fun copyItems(from: Array<out Any>, to: Array<Any>) {
    for (i in from.indices) {
        to[i] = from[i]
    }
}

fun fillWithNulls(destination: Array<in String?>) {
    for (i in destination.indices) {
        destination[i] = null
    }
}

fun <T> copyInto(source: List<T>, destination: MutableList<in T>) {
    for (item in source) {
        destination.add(item)
    }
}

fun <T> readAll(source: List<out T>): List<T> = source.toList()
