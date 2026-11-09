package corpus.when_expr

// `when` with subject, without subject (boolean guards), semicolon-separated
// entries, bound subject (`when (val ...)`), range/type checks.

fun describe(obj: Any): String = when (obj) {
    is String -> "String of length ${obj.length}"
    is Int -> "Integer value $obj"
    in 1..10 -> "Small number"
    !is Number -> "Not a number"
    else -> "Unknown"
}

fun classify(n: Int): String = when {
    n < 0 -> "negative"
    n == 0 -> "zero"
    n < 10 -> "single digit"
    n < 100 -> "double digit"
    else -> "large"
}

fun classifySemicolons(n: Int): String {
    val result = when (n) { 1 -> "one"; 2 -> "two"; 3 -> "three"; else -> "many" }
    return result
}

fun describeBound(supplier: () -> Any): String = when (val r = supplier()) {
    is String -> "string: $r"
    is Int -> "int: $r"
    else -> "other: $r"
}

fun describeBoundTyped(supplier: () -> Int): String {
    return when (val n: Int = supplier()) {
        0 -> "zero"
        in 1..9 -> "digit $n"
        else -> "big $n"
    }
}

sealed class Token {
    object Plus : Token()
    object Minus : Token()
    data class Number(val value: Int) : Token()
}

fun tokenPriority(token: Token): Int = when (token) {
    Token.Plus, Token.Minus -> 1
    is Token.Number -> 0
}

fun weekdayName(day: Int): String = when (day) {
    1, 7 -> "Weekend"
    in 2..6 -> "Weekday"
    else -> throw IllegalArgumentException("invalid day: $day")
}

fun matchType(value: Any?): String = when (value) {
    is List<*> -> "list of ${value.size}"
    null -> "null"
    else -> value.toString()
}

fun guardedWhen(x: Int, y: Int): String = when {
    x > 0 && y > 0 -> "both positive"
    x < 0 && y < 0 -> "both negative"
    x == 0 || y == 0 -> "one is zero"
    else -> "mixed signs"
}

fun main() {
    println(describe(42))
    println(classify(-5))
    println(classifySemicolons(2))
    println(weekdayName(3))
}
