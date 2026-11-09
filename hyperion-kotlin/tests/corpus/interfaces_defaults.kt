package corpus.interfaces

// Interfaces with default method bodies, default properties, multiple
// inheritance resolution, functional interfaces.

interface Greeter {
    val greeting: String
        get() = "Hello"

    fun greet(name: String): String = "$greeting, $name!"
}

interface Loud {
    fun shout(text: String): String = text.uppercase() + "!!!"
}

class FriendlyGreeter : Greeter, Loud {
    override val greeting: String = "Hi there"

    fun loudGreet(name: String): String = shout(greet(name))
}

interface Repository<T> {
    fun findById(id: Long): T?
    fun save(item: T): T
    fun deleteById(id: Long): Boolean {
        return findById(id) != null
    }
}

class InMemoryRepository<T> : Repository<T> {
    private val items = mutableMapOf<Long, T>()
    private var nextId = 0L

    override fun findById(id: Long): T? = items[id]

    override fun save(item: T): T {
        items[nextId] = item
        nextId += 1
        return item
    }
}

interface Comparable2<T> {
    fun compareTo2(other: T): Int
}

interface Validator<T> {
    fun validate(value: T): List<String>

    fun isValid(value: T): Boolean = validate(value).isEmpty()
}

class NonEmptyStringValidator : Validator<String> {
    override fun validate(value: String): List<String> {
        return if (value.isBlank()) listOf("must not be blank") else emptyList()
    }
}

interface Transformer<T, R> {
    fun transform(value: T): R
}

fun applyTransform(values: List<Int>, transformer: Transformer<Int, String>): List<String> {
    return values.map { transformer.transform(it) }
}

interface Animal {
    fun sound(): String
}

abstract class AbstractAnimal(val name: String) : Animal {
    override fun toString(): String = "$name says ${sound()}"
}

class Dog(name: String) : AbstractAnimal(name) {
    override fun sound(): String = "Woof"
}
