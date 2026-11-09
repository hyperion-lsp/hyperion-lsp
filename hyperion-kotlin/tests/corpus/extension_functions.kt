package corpus.extensions

// Extension functions on classes/generics, extension properties, nullable
// receivers, extension functions with receivers on generics.

fun String.isPalindrome(): Boolean {
    val cleaned = this.lowercase().filter { it.isLetterOrDigit() }
    return cleaned == cleaned.reversed()
}

fun String.shout(): String = this.uppercase() + "!"

val String.firstOrEmpty: Char
    get() = if (isNotEmpty()) this[0] else ' '

val String.wordCount: Int
    get() = trim().split(Regex("\\s+")).count { it.isNotBlank() }

fun List<Int>.sumSquares(): Int = this.sumOf { it * it }

fun <T> List<T>.second(): T? = if (size >= 2) this[1] else null

fun <T> List<T>.penultimate(): T? = if (size >= 2) this[size - 2] else null

var StringBuilder.lastChar: Char
    get() = this[length - 1]
    set(value) {
        this.setCharAt(length - 1, value)
    }

class Matrix(val rows: Int, val cols: Int) {
    val data = DoubleArray(rows * cols)

    operator fun get(row: Int, col: Int): Double = data[row * cols + col]
    operator fun set(row: Int, col: Int, value: Double) {
        data[row * cols + col] = value
    }
}

fun Matrix.trace(): Double {
    var sum = 0.0
    for (i in 0 until minOf(rows, cols)) {
        sum += this[i, i]
    }
    return sum
}

fun <T : Comparable<T>> Iterable<T>.isSorted(): Boolean {
    var previous: T? = null
    for (item in this) {
        if (previous != null && previous > item) return false
        previous = item
    }
    return true
}

fun <T> repeatTransform(value: T, times: Int, transform: (T) -> T): T {
    var result = value
    repeat(times) {
        result = transform(result)
    }
    return result
}

fun demoExtensions() {
    println("racecar".isPalindrome())
    println(listOf(1, 2, 3).sumSquares())
    val sb = StringBuilder("hi")
    sb.lastChar = '!'
    println(sb)
}
