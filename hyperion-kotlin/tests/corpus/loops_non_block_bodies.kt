package corpus.loops

// do-while, while, for loops, including non-block (single statement) bodies.

fun countDown(start: Int) {
    var i = start
    do {
        println(i)
        i -= 1
    } while (i > 0)
}

fun sumUntilNonBlock(limit: Int): Int {
    var i = 0
    var sum = 0
    while (i < limit) sum += i++
    return sum
}

fun findFirstEven(numbers: List<Int>): Int? {
    var index = 0
    while (index < numbers.size && numbers[index] % 2 != 0) index++
    return numbers.getOrNull(index)
}

fun printAll(items: List<String>) {
    for (item in items) println(item)
}

fun printRangeNonBlock() {
    for (i in 1..5) print(i)
    println()
}

fun retryUntilSuccess(maxAttempts: Int, action: (Int) -> Boolean): Boolean {
    var attempt = 0
    var succeeded = false
    do {
        attempt += 1
        succeeded = action(attempt)
    } while (!succeeded && attempt < maxAttempts)
    return succeeded
}

fun nestedLoops(): List<Pair<Int, Int>> {
    val result = mutableListOf<Pair<Int, Int>>()
    for (i in 0 until 3) {
        for (j in 0 until 3) {
            if (i == j) continue
            result.add(i to j)
        }
    }
    return result
}

fun stepAndDownTo() {
    for (i in 10 downTo 1 step 2) println(i)
    for (i in 0..20 step 5) println(i)
}

fun whileTrueBreak(): Int {
    var counter = 0
    while (true) {
        counter += 1
        if (counter >= 10) break
    }
    return counter
}

fun labeledLoops(): Int {
    var total = 0
    outer@ for (i in 1..5) {
        for (j in 1..5) {
            if (j > i) continue@outer
            if (i * j > 20) break@outer
            total += i * j
        }
    }
    return total
}

fun singleStatementIfInLoop(items: List<Int>) {
    for (item in items) if (item < 0) println("negative: $item") else println("ok: $item")
}
