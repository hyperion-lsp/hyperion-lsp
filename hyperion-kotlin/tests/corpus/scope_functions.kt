package corpus.scope

// Scope functions: let, run, with, apply, also — chained and nested.

class Config {
    var host: String = "localhost"
    var port: Int = 8080
    var timeoutMs: Long = 5000
}

fun buildConfig(): Config {
    return Config().apply {
        host = "example.com"
        port = 443
        timeoutMs = 3000
    }
}

fun printAndReturn(value: String): String {
    return value.also {
        println("Processing: $it")
    }
}

fun describeConfig(config: Config): String {
    return with(config) {
        "$host:$port (timeout=${timeoutMs}ms)"
    }
}

fun parseOrNull(input: String?): Int? {
    return input?.let {
        it.trim().toIntOrNull()
    }
}

fun chained(): String {
    return "  Hello World  "
        .trim()
        .let { it.lowercase() }
        .run { replace(" ", "-") }
        .also { println("Result: $it") }
}

class Connection(val name: String) {
    fun open(): Connection = this.also { println("Opening $name") }
    fun close() {
        println("Closing $name")
    }

    fun <T> use(block: (Connection) -> T): T {
        try {
            return block(this)
        } finally {
            close()
        }
    }
}

fun demoNestedScopes() {
    val connection = Connection("db").open()
    val result = connection.use { conn ->
        conn.run {
            "used connection"
        }
    }
    println(result)

    val config = buildConfig().apply {
        port = port + 1
    }.let { cfg ->
        describeConfig(cfg)
    }
    println(config)
}

data class Box3(var content: String?)

fun updateBoxSafely(box: Box3, newValue: String?) {
    box.content = newValue?.let { it.trim() } ?: run {
        println("no value provided")
        "default"
    }
}
