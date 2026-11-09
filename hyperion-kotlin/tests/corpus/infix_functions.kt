package corpus.infix

// Infix function declarations and usage, chained infix calls.

infix fun Int.pow(exponent: Int): Long {
    var result = 1L
    repeat(exponent) { result *= this }
    return result
}

infix fun String.repeatedTimes(count: Int): String = this.repeat(count)

class Range3(val start: Int, val endInclusive: Int)

infix fun Int.upTo(end: Int): Range3 = Range3(this, end)

data class Vec(val x: Int, val y: Int)

infix fun Vec.dot(other: Vec): Int = x * other.x + y * other.y

infix fun Vec.cross(other: Vec): Int = x * other.y - y * other.x

class Money2(val cents: Long) {
    infix fun plus(other: Money2): Money2 = Money2(cents + other.cents)
}

infix fun <T> T.pairedWith(other: T): Pair<T, T> = this to other

infix fun Boolean.implies(other: Boolean): Boolean = !this || other

fun demoInfix() {
    val result = 2 pow 10
    println(result)

    val repeated = "ab" repeatedTimes 3
    println(repeated)

    val range = 1 upTo 10
    println("${range.start}..${range.endInclusive}")

    val a = Vec(1, 2)
    val b = Vec(3, 4)
    println(a dot b)
    println(a cross b)

    val money = Money2(100) plus Money2(200)
    println(money.cents)

    val pair = "left" pairedWith "right"
    println(pair)

    println(true implies false)

    val combined = 1 to 2
    println(combined)
}

infix fun Int.coerceInRangeOf(other: IntRange): Int = this.coerceIn(other)
