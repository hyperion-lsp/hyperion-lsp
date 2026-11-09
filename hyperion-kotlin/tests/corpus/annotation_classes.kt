package corpus.annotations

// Annotation classes, use-site targets, annotated declarations.

annotation class Author(val name: String)

@Target(AnnotationTarget.FUNCTION, AnnotationTarget.PROPERTY)
@Retention(AnnotationRetention.RUNTIME)
annotation class Benchmark(val warmupIterations: Int = 1, val measuredIterations: Int = 10)

@Repeatable
annotation class Tag(val value: String)

annotation class ReplaceWith(val expression: String)

annotation class Deprecated2(val message: String, val replaceWith: ReplaceWith = ReplaceWith(""))

@Author("Adrian")
class LegacyParser {
    @field:JvmField
    var cachedResult: String? = null

    @get:JvmName("getRawValue")
    val rawValue: Int = 42

    @param:Suppress("UNUSED_PARAMETER")
    fun configure(@Suppress("UNUSED_PARAMETER") verbose: Boolean) {
        // no-op
    }

    @Benchmark(warmupIterations = 3, measuredIterations = 100)
    fun parse(input: String): Int = input.length

    @Tag("slow")
    @Tag("integration")
    fun slowIntegrationCheck(): Boolean = true
}

@Suppress("unused", "UNCHECKED_CAST")
fun <T> unsafeCast(value: Any?): T {
    @Suppress("UNCHECKED_CAST")
    return value as T
}

// KNOWN PARSER GAP: an annotation argument that is itself a call to
// another annotation's constructor (nested annotation values, the exact
// shape of the real `@Deprecated(message = ..., replaceWith =
// ReplaceWith(...))`) is not parsed — the annotation-argument-value
// parser only accepts literals/qualified-names/`::class`, not a full call
// expression. Kept on the known-failures ratchet in mass_parse_test.rs.
@Deprecated2("use newApi() instead", ReplaceWith("newApi()"))
fun oldApi(): Int = 0

fun newApi(): Int = 1

class AnnotatedParams(
    @Author("team-a") val ownerA: String,
    @field:Transient val transientField: Int = 0,
)
