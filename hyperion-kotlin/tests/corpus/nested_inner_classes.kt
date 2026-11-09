package corpus.nesting

// Nested classes (no reference to outer instance) vs inner classes
// (`inner`, capture outer `this`), local functions, deep nesting.
//
// Local *classes* (a `class` declared inside a function body) are a
// separate, currently-unsupported construct — see local_classes.kt on the
// known-failures ratchet — kept out of this file so it stays measured
// green.

class Outer(val name: String) {
    class Nested(val label: String) {
        fun describe(): String = "Nested: $label"
    }

    inner class Inner(val suffix: String) {
        fun describe(): String = "Inner of $name: $suffix"

        fun outerName(): String {
            return this@Outer.name
        }
    }

    fun makeInner(suffix: String): Inner = Inner(suffix)
}

class Tree {
    class Node(val value: Int) {
        var left: Node? = null
        var right: Node? = null

        fun sum(): Int {
            return value + (left?.sum() ?: 0) + (right?.sum() ?: 0)
        }
    }

    var root: Node? = null

    fun insert(value: Int) {
        root = insertInto(root, value)
    }

    private fun insertInto(node: Node?, value: Int): Node {
        if (node == null) return Node(value)
        if (value < node.value) {
            node.left = insertInto(node.left, value)
        } else {
            node.right = insertInto(node.right, value)
        }
        return node
    }
}

class Container<T> {
    inner class Iterator2(var index: Int = 0) {
        fun hasNext(): Boolean = index < items.size
        fun next(): T {
            val value = items[index]
            index += 1
            return value
        }
    }

    private val items = mutableListOf<T>()

    fun add(item: T) {
        items.add(item)
    }

    fun iterator(): Iterator2 = Iterator2()
}

class Graph {
    inner class Vertex(val id: Int) {
        val neighbors = mutableListOf<Vertex>()

        fun connectTo(other: Vertex) {
            neighbors.add(other)
            registerEdge(this, other)
        }
    }

    private val edges = mutableListOf<Pair<Vertex, Vertex>>()

    private fun registerEdge(a: Vertex, b: Vertex) {
        edges.add(a to b)
    }

    fun edgeCount(): Int = edges.size
}

fun demoLocalFunction(): Int {
    fun localHelper(x: Int): Int = x * x

    var total = 0
    for (i in 1..3) {
        total += localHelper(i)
    }
    return total
}

class DeepNesting {
    class Level1 {
        class Level2 {
            class Level3(val value: String)
        }
    }

    fun build(): DeepNesting.Level1.Level2.Level3 = Level1.Level2.Level3("deep")
}
