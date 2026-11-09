package corpus.typealiases

// Type aliases: simple, generic, function-shaped, nested generics.

typealias UserId = Long

typealias UserMap = Map<UserId, String>

typealias Handler<T> = (T) -> Unit

typealias Predicate<T> = (T) -> Boolean

typealias BiFunction<A, B, R> = (A, B) -> R

typealias StringPredicate = Predicate<String>

typealias NestedMap = Map<String, List<Pair<Int, String>>>

typealias AsyncCallback<T> = (T, Throwable?) -> Unit

class EventEmitter {
    private val handlers = mutableListOf<Handler<String>>()

    fun on(handler: Handler<String>) {
        handlers.add(handler)
    }

    fun emit(event: String) {
        for (h in handlers) {
            h(event)
        }
    }
}

fun filterBy(items: List<String>, predicate: StringPredicate): List<String> {
    return items.filter(predicate)
}

fun combine(a: Int, b: Int, op: BiFunction<Int, Int, Int>): Int = op(a, b)

fun buildUserMap(pairs: List<kotlin.Pair<UserId, String>>): UserMap {
    return pairs.toMap()
}

fun describeNested(map: NestedMap): Int = map.values.sumOf { it.size }

fun withCallback(id: UserId, callback: AsyncCallback<String>) {
    callback("user-$id", null)
}

fun demoTypealiases() {
    val emitter = EventEmitter()
    emitter.on { msg -> println("Got: $msg") }
    emitter.emit("hello")

    val isLong: StringPredicate = { it.length > 5 }
    println(filterBy(listOf("hi", "hello world"), isLong))
}
