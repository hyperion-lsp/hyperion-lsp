package corpus.coroutines

// Suspend functions, coroutine-style builders, structured concurrency shapes.
// (No real kotlinx.coroutines dependency — these mimic the shapes without
// requiring the library, so the file is self-contained.)
//
// Note: higher-order parameters below intentionally use plain function
// types (`() -> T`) rather than `suspend () -> T`. The latter is a real,
// separately-tracked parser gap in parameter position — see
// suspend_function_types.kt on the known-failures ratchet — kept out of
// this file so the rest of the suspend/coroutine surface stays measured
// green.

suspend fun delay(millis: Long) {
    // placeholder suspend function
}

suspend fun fetchUser(id: Long): String {
    delay(100)
    return "user-$id"
}

suspend fun fetchAll(ids: List<Long>): List<String> {
    val results = mutableListOf<String>()
    for (id in ids) {
        results.add(fetchUser(id))
    }
    return results
}

suspend fun withTimeoutFallback(timeoutMs: Long, block: () -> String): String {
    return try {
        block()
    } catch (e: Exception) {
        "fallback"
    }
}

class Deferred2<T>(val value: T) {
    suspend fun await(): T = value
}

interface Scope {
    fun <T> async(block: () -> T): Deferred2<T>
    fun launch(block: () -> Unit)
}

class SimpleScope : Scope {
    override fun <T> async(block: () -> T): Deferred2<T> {
        return Deferred2(block())
    }

    override fun launch(block: () -> Unit) {
        block()
    }
}

suspend fun processData(scope: Scope) {
    val deferredResult = scope.async {
        "queued"
    }
    val user = deferredResult.await()
    println(user)
}

suspend fun retryWithBackoff(attempts: Int, block: (Int) -> Boolean): Boolean {
    var attempt = 0
    while (attempt < attempts) {
        if (block(attempt)) return true
        attempt += 1
        delay(attempt * 100L)
    }
    return false
}

class Flow2<T>(val producer: ((T) -> Unit) -> Unit) {
    fun collect(consumer: (T) -> Unit) {
        producer(consumer)
    }
}

fun numbersFlow(): Flow2<Int> = Flow2 { emit ->
    for (i in 1..5) {
        emit(i)
    }
}

fun sumFlow(flow: Flow2<Int>): Int {
    var total = 0
    flow.collect { value -> total += value }
    return total
}

suspend inline fun <T> runSuspendCatching(crossinline block: () -> T): Result<T> {
    return try {
        Result.success(block())
    } catch (e: Throwable) {
        Result.failure(e)
    }
}
