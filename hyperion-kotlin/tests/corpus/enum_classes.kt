package corpus.enums

// Enum classes: plain, with constructor args, with bodies/overrides, interface impls.

enum class Direction {
    NORTH, EAST, SOUTH, WEST
}

fun opposite(direction: Direction): Direction = when (direction) {
    Direction.NORTH -> Direction.SOUTH
    Direction.SOUTH -> Direction.NORTH
    Direction.EAST -> Direction.WEST
    Direction.WEST -> Direction.EAST
}

enum class HttpStatus(val code: Int, val reason: String) {
    OK(200, "OK"),
    NOT_FOUND(404, "Not Found"),
    SERVER_ERROR(500, "Internal Server Error");

    fun isError(): Boolean = code >= 400
}

interface Describable {
    fun describe(): String
}

enum class Planet(val massKg: Double, val radiusM: Double) : Describable {
    MERCURY(3.303e+23, 2.4397e6),
    VENUS(4.869e+24, 6.0518e6),
    EARTH(5.976e+24, 6.37814e6) {
        override fun describe(): String = "Home"
    };

    fun surfaceGravity(): Double {
        val g = 6.67300E-11
        return g * massKg / (radiusM * radiusM)
    }

    override fun describe(): String = "Planet with mass $massKg"
}

enum class Operation {
    PLUS {
        override fun apply(a: Int, b: Int) = a + b
    },
    MINUS {
        override fun apply(a: Int, b: Int) = a - b
    },
    TIMES {
        override fun apply(a: Int, b: Int) = a * b
    };

    abstract fun apply(a: Int, b: Int): Int
}

fun computeAll(a: Int, b: Int): List<Int> {
    return Operation.values().map { it.apply(a, b) }
}

fun statusFor(code: Int): HttpStatus {
    return HttpStatus.entries.firstOrNull { it.code == code } ?: HttpStatus.SERVER_ERROR
}
