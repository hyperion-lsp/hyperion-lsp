package corpus.generics

// Generics: declaration-site variance, use-site variance (in/out projections),
// bounded type parameters, multiple bounds with `where`.

class Box<T>(var value: T) {
    fun get(): T = value
    fun set(newValue: T) {
        value = newValue
    }
}

class ReadOnlyBox<out T>(val value: T) {
    fun get(): T = value
}

class WriteOnlyBox<in T> {
    private var value: Any? = null

    fun set(newValue: T) {
        value = newValue
    }
}

interface Producer<out T> {
    fun produce(): T
}

interface Consumer<in T> {
    fun consume(value: T)
}

fun copyItems(from: Array<Any>, to: Array<Any>) {
    for (i in from.indices) {
        to[i] = from[i]
    }
}

fun fillWithNulls(destination: Array<String?>) {
    for (i in destination.indices) {
        destination[i] = null
    }
}

fun <T : Comparable<T>> maxOf3(a: T, b: T, c: T): T {
    var result = a
    if (b > result) result = b
    if (c > result) result = c
    return result
}

fun <T> firstOrDefault(list: List<T>, default: T): T {
    return if (list.isNotEmpty()) list[0] else default
}

fun <K, V> zipToMap(keys: List<K>, values: List<V>): Map<K, V> {
    return keys.zip(values).toMap()
}

fun <T> copyList(source: List<T>, destination: MutableList<T>) {
    for (item in source) {
        destination.add(item)
    }
}

class Pair2<A, B>(val first: A, val second: B) {
    fun <C> withThird(third: C): Triple<A, B, C> = Triple(first, second, third)
}

fun <T> processWhere(items: List<T>): List<String> where T : Comparable<T>, T : CharSequence {
    return items.sorted().map { it.toString() }
}

class NumericBox<T> where T : Number, T : Comparable<T> {
    var value: T? = null

    fun isPositive(): Boolean = (value?.toDouble() ?: 0.0) > 0.0
}

fun <T> List<T>.secondOrNull(): T? = if (size >= 2) this[1] else null

fun main() {
    val box = Box(42)
    box.set(box.get() + 1)
    val readOnly: ReadOnlyBox<Number> = ReadOnlyBox(box.get())
    println(readOnly.get())
}
