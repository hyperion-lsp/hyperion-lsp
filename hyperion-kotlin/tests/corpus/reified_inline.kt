package corpus.reified

// Inline functions with reified type parameters.

inline fun <reified T> isInstance(value: Any?): Boolean = value is T

inline fun <reified T> castOrNull(value: Any?): T? = value as? T

inline fun <reified T> arrayOfNulls2(size: Int): Array<T?> = arrayOfNulls(size)

inline fun <reified T, reified U> bothTypesMatch(a: Any?, b: Any?): Boolean {
    return a is T && b is U
}

inline fun <reified T> filterIsInstance2(items: List<Any?>): List<T> {
    val result = mutableListOf<T>()
    for (item in items) {
        if (item is T) {
            result.add(item)
        }
    }
    return result
}

// KNOWN PARSER GAP: the `X::class` class-literal expression (here on a
// reified type parameter, but the same failure occurs for `Int::class`
// etc.) is not parsed — "Expected IDENT, found EOF/DOT". Plain callable
// references without `class` (e.g. `String::length`) parse fine; this is
// specific to the `::class` literal form. Kept on the known-failures
// ratchet in mass_parse_test.rs.
inline fun <reified T> typeName(): String = T::class.simpleName ?: "Unknown"

inline fun <T, reified R> mapAndCheck(items: List<T>, transform: (T) -> R): List<R> {
    return items.map { transform(it) }
}

inline fun <reified T : Throwable> runCatchingSpecific(block: () -> Unit): T? {
    return try {
        block()
        null
    } catch (e: Throwable) {
        e as? T
    }
}

fun demoReified() {
    val values: List<Any?> = listOf(1, "two", 3.0, "four")
    val strings = filterIsInstance2<String>(values)
    println(strings)
    println(isInstance<String>("hello"))
    println(typeName<Int>())
}
