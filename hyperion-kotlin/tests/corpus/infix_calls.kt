package corpus.infixcalls

// Identifier-based infix function calls in expression position. This file
// covers standard-library idioms (`to`, bitwise functions, ranges) and a
// custom infix declaration, including chaining and use inside calls/when.

infix fun Int.scaledBy(factor: Int): Int = this * factor

fun pairExamples(): List<Pair<String, Int>> = listOf(
    "one" to 1,
    "two" to 2,
    "three" to 3,
)

fun chainedPairs(): Pair<Pair<Int, Int>, Int> = 1 to 2 to 3

fun bitAndRangeExamples(limit: Int): List<Int> {
    val shifted = 1 shl 2
    val masked = shifted and 7
    val stepped = (0 until limit step 2).toList()
    val descending = (limit downTo 0 step 2).toList()
    val custom = 3 scaledBy 4
    return stepped + descending + listOf(masked, custom)
}

fun infixInLargerExpressions(value: Int, limit: Int): String {
    val pair = consume(value to limit)
    return when (value shl 1) {
        in 0 until limit -> "small:$pair"
        else -> "large"
    }
}

fun <T> consume(value: T): T = value
