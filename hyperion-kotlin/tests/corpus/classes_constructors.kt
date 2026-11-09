package corpus.classes

// Basic classes, primary + secondary constructors, init blocks, visibility.

class Point(val x: Int, val y: Int) {
    fun distanceTo(other: Point): Double {
        val dx = (x - other.x).toDouble()
        val dy = (y - other.y).toDouble()
        return Math.sqrt(dx * dx + dy * dy)
    }
}

class Person {
    var name: String
    var age: Int
    private var internalId: Long = 0

    init {
        name = "Unknown"
        age = 0
        internalId = System.nanoTime()
    }

    constructor(name: String) : this() {
        this.name = name
    }

    constructor(name: String, age: Int) : this(name) {
        this.age = age
    }

    constructor() {
        name = "Anonymous"
        age = -1
    }

    fun describe(): String = "$name is $age years old (id=$internalId)"
}

open class Shape(val label: String) {
    init {
        println("Creating shape: $label")
    }

    open fun area(): Double = 0.0
}

class Circle(val radius: Double) : Shape("circle") {
    override fun area(): Double = Math.PI * radius * radius
}

class Rectangle(val width: Double, val height: Double) : Shape("rectangle") {
    override fun area(): Double = width * height

    init {
        require(width > 0.0) { "width must be positive" }
        require(height > 0.0) { "height must be positive" }
    }
}

private class Counter(start: Int = 0) {
    private var value = start

    fun increment(step: Int = 1): Int {
        value += step
        return value
    }

    fun current(): Int = value
}

internal open class Base {
    open val kind: String get() = "base"
}

class Derived : Base() {
    override val kind: String = "derived"
}

fun buildPeople(): List<Person> {
    val a = Person("Alice", 30)
    val b = Person("Bob")
    val c = Person()
    return listOf(a, b, c)
}
