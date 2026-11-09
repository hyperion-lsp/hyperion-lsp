package corpus.properties

// Custom property getters/setters, backing fields, lateinit, const, top-level
// properties, computed properties.

class Temperature(celsiusInitial: Double) {
    var celsius: Double = celsiusInitial
        set(value) {
            field = value
        }

    val fahrenheit: Double
        get() = celsius * 9.0 / 5.0 + 32.0

    var fahrenheitSettable: Double
        get() = celsius * 9.0 / 5.0 + 32.0
        set(value) {
            celsius = (value - 32.0) * 5.0 / 9.0
        }
}

class Person3 {
    var name: String = ""
        get() = field.uppercase()
        set(value) {
            field = value.trim()
        }

    val age: Int = 0

    lateinit var address: String

    val fullName: String
        get() = "$name (age $age)"

    fun assignAddress(value: String) {
        address = value
    }
}

val topLevel = "global"
var mutableGlobal: Int = 42

const val CONSTANT = 100

const val MAX_RETRIES: Int = 3

class Circle2(val radius: Double) {
    val area: Double
        get() = Math.PI * radius * radius

    val circumference: Double get() = 2 * Math.PI * radius
}

class Rectangle2(var width: Double, var height: Double) {
    var area: Double = width * height
        private set

    fun resize(newWidth: Double, newHeight: Double) {
        width = newWidth
        height = newHeight
        area = width * height
    }
}

class LazyPair {
    val expensive: Int by lazy { computeExpensive() }

    private fun computeExpensive(): Int {
        return (1..100).sum()
    }
}

abstract class Shape3 {
    abstract val perimeter: Double
    open val description: String
        get() = "Shape with perimeter $perimeter"
}

class Square3(val side: Double) : Shape3() {
    override val perimeter: Double
        get() = side * 4
}

interface HasId {
    val id: Long
}

class Entity(val name: String, providedId: Long) : HasId {
    override val id: Long = providedId
}
