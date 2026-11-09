package corpus.delegation

import kotlin.properties.Delegates

// Property delegation (by lazy, custom delegates, observable) and class
// delegation (`by` on interfaces).

val expensiveValue: Int by lazy {
    println("Computing expensive value")
    42
}

class LazyHolder {
    val computed: String by lazy {
        "computed-once"
    }
}

class Account {
    var balance: Int by Delegates.observable(0) { _, old, new ->
        println("Balance changed from $old to $new")
    }

    var limit: Int by Delegates.vetoable(1000) { _, _, new ->
        new <= 10000
    }
}

class Cache<K, V>(val loader: (K) -> V) {
    private val map = mutableMapOf<K, V>()

    operator fun getValue(thisRef: Any?, property: Any?): Map<K, V> = map

    fun get(key: K): V = map.getOrPut(key) { loader(key) }
}

class StringDelegate {
    private var backing: String = ""

    operator fun getValue(thisRef: Any?, property: kotlin.reflect.KProperty<*>): String {
        return backing
    }

    operator fun setValue(thisRef: Any?, property: kotlin.reflect.KProperty<*>, value: String) {
        backing = value.trim()
    }
}

class Form {
    var username: String by StringDelegate()
    var password: String by StringDelegate()
}

interface Renderer {
    fun render(): String
}

class DefaultRenderer : Renderer {
    override fun render(): String = "default"
}

class WidgetRenderer(val delegate: Renderer = DefaultRenderer()) : Renderer by delegate {
    fun renderWithPrefix(prefix: String): String = "$prefix:${render()}"
}

interface Logger {
    fun log(message: String)
}

class ConsoleLogger : Logger {
    override fun log(message: String) {
        println("[LOG] $message")
    }
}

class Service(logger: Logger = ConsoleLogger()) : Logger by logger {
    fun doWork() {
        log("doing work")
    }
}

fun demoDelegation() {
    println(expensiveValue)
    val account = Account()
    account.balance = 100
    val form = Form()
    form.username = "  admin  "
    println(form.username)
}
