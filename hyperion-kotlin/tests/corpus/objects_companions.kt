package corpus.objects

// Object declarations, companion objects, object expressions (covered further
// in object_literals.kt), named companions.

object Singleton {
    var counter = 0

    fun increment(): Int {
        counter += 1
        return counter
    }
}

class Factory private constructor(val label: String) {
    companion object {
        const val DEFAULT_LABEL = "default"

        fun create(): Factory = Factory(DEFAULT_LABEL)

        fun createWithLabel(label: String): Factory = Factory(label)
    }

    fun label(): String = label
}

class Database private constructor() {
    companion object Holder {
        private var instance: Database? = null

        fun getInstance(): Database {
            return instance ?: Database().also { instance = it }
        }
    }
}

interface JsonSerializable {
    fun toJson(): String

    companion object {
        fun empty(): JsonSerializable = object : JsonSerializable {
            override fun toJson(): String = "{}"
        }
    }
}

class Config(val values: Map<String, String>) {
    companion object {
        @JvmStatic
        fun fromEnv(): Config = Config(mapOf("env" to "test"))

        operator fun invoke(vararg pairs: Pair<String, String>): Config = Config(pairs.toMap())
    }
}

fun buildConfig(): Config = Config("a" to "1", "b" to "2")

object PathUtils {
    private const val SEPARATOR = "/"

    fun join(vararg parts: String): String = parts.joinToString(SEPARATOR)
}

object EventBus {
    private val listeners = mutableListOf<(String) -> Unit>()

    fun subscribe(listener: (String) -> Unit) {
        listeners.add(listener)
    }

    fun publish(event: String) {
        for (listener in listeners) {
            listener(event)
        }
    }
}
