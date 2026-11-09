package corpus.lambdas

// Lambdas, trailing lambdas, lambdas with receivers, higher-order functions.
//
// KNOWN PARSER GAP: `HtmlBuilder.() -> Unit` below hits the same
// receiver-lambda-type dot-duplication bug documented in dsl_builders.kt
// (zero reported errors, but the round-trip is lossy: the receiver dot is
// emitted twice). Kept on the known-failures ratchet in
// mass_parse_test.rs.

fun main() {
    val numbers = listOf(1, 2, 3, 4, 5)

    val doubled = numbers.map { it * 2 }
    val filtered = numbers.filter { n -> n > 2 }
    val sum = numbers.fold(0) { acc, n -> acc + n }
    val indexed = numbers.mapIndexed { index, n -> "$index:$n" }

    val callback: (String) -> Unit = { message ->
        println("Callback: $message")
    }
    callback("hi")

    numbers.forEach {
        println(it)
    }

    val grouped = numbers.groupBy({ it % 2 == 0 }, { it * 10 })
    println(grouped)

    println(doubled)
    println(filtered)
    println(sum)
    println(indexed)
}

fun higherOrder(block: () -> Unit) {
    block()
}

fun withResult(compute: () -> Int): Int = compute()

fun repeatAction(times: Int, action: (Int) -> Unit) {
    for (i in 0 until times) {
        action(i)
    }
}

inline fun <T> Collection<T>.myFilter(predicate: (T) -> Boolean): List<T> {
    val result = mutableListOf<T>()
    for (item in this) {
        if (predicate(item)) {
            result.add(item)
        }
    }
    return result
}

class HtmlBuilder {
    private val sb = StringBuilder()

    fun tag(name: String, block: HtmlBuilder.() -> Unit) {
        sb.append("<$name>")
        this.block()
        sb.append("</$name>")
    }

    fun text(value: String) {
        sb.append(value)
    }

    override fun toString(): String = sb.toString()
}

fun buildHtml(block: HtmlBuilder.() -> Unit): String {
    val builder = HtmlBuilder()
    builder.block()
    return builder.toString()
}

fun demoReceiverLambda(): String = buildHtml {
    tag("div") {
        tag("span") {
            text("hello")
        }
    }
}

fun sumWithLastLambda(a: Int, b: Int, combine: (Int, Int) -> Int): Int {
    return combine(a, b)
}

fun demoTrailingLambdaChain() {
    val values = listOf(5, 3, 8, 1)
    val result = values
        .filter { it > 2 }
        .map { it * it }
        .sortedByDescending { it }
        .joinToString(separator = ", ") { "[$it]" }
    println(result)
}
