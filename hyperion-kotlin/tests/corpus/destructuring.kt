package corpus.destructuring

// Destructuring declarations: data classes, Pair/Map.Entry, in for-loops,
// in lambda parameters, with underscores for unused components.

data class Coordinates(val lat: Double, val lon: Double)

fun formatCoordinates(coords: Coordinates): String {
    val (lat, lon) = coords
    return "($lat, $lon)"
}

fun ignoreSecond(coords: Coordinates): Double {
    val (lat, _) = coords
    return lat
}

fun destructurePair(pair: Pair<String, Int>): String {
    val (key, value) = pair
    return "$key=$value"
}

fun iterateMap(map: Map<String, Int>) {
    for ((key, value) in map) {
        println("$key -> $value")
    }
}

fun iterateIndexed(items: List<String>) {
    for ((index, value) in items.withIndex()) {
        println("$index: $value")
    }
}

fun sumEntries(map: Map<String, Int>): Int {
    return map.entries.sumOf { (_, value) -> value }
}

data class Triple3(val a: Int, val b: Int, val c: Int)

fun sumTriple(triple: Triple3): Int {
    val (a, b, c) = triple
    return a + b + c
}

fun destructureInLambda(pairs: List<Pair<Int, String>>): List<String> {
    return pairs.map { (num, label) -> "$label=$num" }
}

class Vector2(val x: Double, val y: Double) {
    operator fun component1(): Double = x
    operator fun component2(): Double = y
}

fun addVectors(a: Vector2, b: Vector2): Vector2 {
    val (ax, ay) = a
    val (bx, by) = b
    return Vector2(ax + bx, ay + by)
}

fun nestedDestructuring(items: List<Pair<Coordinates, String>>): List<String> {
    return items.map { (coords, label) ->
        val (lat, lon) = coords
        "$label at $lat,$lon"
    }
}
