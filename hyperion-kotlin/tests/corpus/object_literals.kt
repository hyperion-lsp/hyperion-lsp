package corpus.objectliterals

// Anonymous object expressions (object literals): implementing interfaces,
// extending classes, capturing locals, used as arguments/return values.

interface ClickListener {
    fun onClick(x: Int, y: Int)
}

fun registerListener(register: (ClickListener) -> Unit) {
    register(object : ClickListener {
        override fun onClick(x: Int, y: Int) {
            println("Clicked at $x,$y")
        }
    })
}

abstract class Task(val name: String) {
    abstract fun run(): Int
}

fun makeTask(label: String, multiplier: Int): Task {
    return object : Task(label) {
        override fun run(): Int = multiplier * 2
    }
}

interface Comparator2<T> {
    fun compare(a: T, b: T): Int
}

fun reverseComparator(): Comparator2<Int> = object : Comparator2<Int> {
    override fun compare(a: Int, b: Int): Int = b - a
}

fun sortDescending(items: MutableList<Int>) {
    val comparator = object : Comparator2<Int> {
        override fun compare(a: Int, b: Int): Int = b.compareTo(a)
    }
    items.sortWith(Comparator { a, b -> comparator.compare(a, b) })
}

interface Watcher {
    fun onChanged(value: Int)
    fun onReset()
}

class Observable(var value: Int) {
    private val watchers = mutableListOf<Watcher>()

    fun watch(watcher: Watcher) {
        watchers.add(watcher)
    }

    fun update(newValue: Int) {
        value = newValue
        for (w in watchers) w.onChanged(newValue)
    }
}

fun demoAnonymousWatcher(observable: Observable) {
    var lastSeen = 0
    observable.watch(object : Watcher {
        override fun onChanged(value: Int) {
            lastSeen = value
            println("Changed to $value, was $lastSeen")
        }

        override fun onReset() {
            lastSeen = 0
        }
    })
}

fun makeCounterState(): Any {
    return object {
        var count = 0

        fun increment() {
            count += 1
        }
    }
}

fun demoObjectLiterals() {
    registerListener { listener -> listener.onClick(1, 2) }
    val task = makeTask("double", 5)
    println(task.run())
    println(reverseComparator().compare(3, 5))
}
