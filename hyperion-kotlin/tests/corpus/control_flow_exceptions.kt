package corpus.control

// Exceptions: try/catch/finally as statement and expression, multiple catch
// clauses, custom exceptions, labeled returns from lambdas, if/else as
// expressions.

class ValidationException(message: String) : Exception(message)
class NotFoundException(message: String, val code: Int) : RuntimeException(message)

fun parseAge(input: String): Int {
    return try {
        val value = input.toInt()
        if (value < 0) throw ValidationException("age cannot be negative")
        value
    } catch (e: NumberFormatException) {
        throw ValidationException("not a number: $input")
    }
}

fun readSafely(action: () -> String): String {
    return try {
        action()
    } catch (e: NotFoundException) {
        "not found (${e.code})"
    } catch (e: ValidationException) {
        "invalid: ${e.message}"
    } catch (e: Exception) {
        "error: ${e.message}"
    } finally {
        println("read attempt finished")
    }
}

fun divideChecked(a: Int, b: Int): Int {
    if (b == 0) {
        throw ArithmeticException("division by zero")
    }
    return a / b
}

fun maxOfExpr(a: Int, b: Int): Int = if (a > b) a else b

fun classify2(n: Int): String {
    val label = if (n < 0) {
        "negative"
    } else if (n == 0) {
        "zero"
    } else {
        "positive"
    }
    return label
}

fun findFirstMatching(numbers: List<Int>, predicate: (Int) -> Boolean): Int? {
    numbers.forEach {
        if (predicate(it)) return it
    }
    return null
}

fun findFirstLabeled(numbers: List<Int>, predicate: (Int) -> Boolean): Int? {
    numbers.forEach label@{
        if (!predicate(it)) return@label
        return it
    }
    return null
}

fun sumPositive(numbers: List<Int>): Int {
    var total = 0
    numbers.forEach {
        if (it < 0) return@forEach
        total += it
    }
    return total
}

fun tryWithResources(): String {
    val resource = AutoCloseable { println("closed") }
    return resource.use {
        "used"
    }
}

fun rethrowWrapped(action: () -> Unit) {
    try {
        action()
    } catch (e: Exception) {
        throw RuntimeException("wrapped: ${e.message}", e)
    }
}

fun assertions(value: Int) {
    check(value >= 0) { "value must be non-negative" }
    require(value < 1000) { "value too large" }
    assert(value != 42) { "unlucky number" }
}
