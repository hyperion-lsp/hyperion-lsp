package corpus.operators

// Operator overloading: arithmetic, comparison, indexing, invoke, unary,
// compound assignment, `in`, iterator.

data class Vector(val x: Double, val y: Double) {
    operator fun plus(other: Vector): Vector = Vector(x + other.x, y + other.y)
    operator fun minus(other: Vector): Vector = Vector(x - other.x, y - other.y)
    operator fun times(scalar: Double): Vector = Vector(x * scalar, y * scalar)
    operator fun unaryMinus(): Vector = Vector(-x, -y)
    operator fun get(index: Int): Double = if (index == 0) x else y
    operator fun compareTo(other: Vector): Int = (x * x + y * y).compareTo(other.x * other.x + other.y * other.y)
}

operator fun Double.times(vector: Vector): Vector = Vector(this * vector.x, this * vector.y)

class Accumulator(var total: Int = 0) {
    operator fun plusAssign(value: Int) {
        total += value
    }

    operator fun minusAssign(value: Int) {
        total -= value
    }
}

class Grid(val width: Int, val height: Int) {
    private val cells = IntArray(width * height)

    operator fun get(x: Int, y: Int): Int = cells[y * width + x]
    operator fun set(x: Int, y: Int, value: Int) {
        cells[y * width + x] = value
    }

    operator fun contains(value: Int): Boolean = cells.contains(value)
}

class Multiplier(val factor: Int) {
    operator fun invoke(value: Int): Int = value * factor
}

class Range2(val start: Int, val end: Int) : Iterable<Int> {
    override fun iterator(): Iterator<Int> = object : Iterator<Int> {
        var current = start
        override fun hasNext(): Boolean = current <= end
        override fun next(): Int = current++
    }

    operator fun contains(value: Int): Boolean = value in start..end
}

class Fraction(val numerator: Int, val denominator: Int) : Comparable<Fraction> {
    operator fun plus(other: Fraction): Fraction {
        return Fraction(
            numerator * other.denominator + other.numerator * denominator,
            denominator * other.denominator,
        )
    }

    override fun compareTo(other: Fraction): Int {
        return (numerator * other.denominator) - (other.numerator * denominator)
    }

    override fun equals(other: Any?): Boolean {
        if (other !is Fraction) return false
        return numerator * other.denominator == other.numerator * denominator
    }

    override fun hashCode(): Int = numerator * 31 + denominator
}

fun demoOperators() {
    val a = Vector(1.0, 2.0)
    val b = Vector(3.0, 4.0)
    val c = a + b - (-b)
    println(c[0])

    val acc = Accumulator()
    acc += 10
    acc -= 3
    println(acc.total)

    val doubleIt = Multiplier(2)
    println(doubleIt(21))

    for (i in Range2(1, 5)) {
        print(i)
    }
    println()

    println(3 in Range2(1, 5))
}
