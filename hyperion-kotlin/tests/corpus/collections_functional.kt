package corpus.collections

// Functional-style collection processing: chains, sequences, varargs,
// spread operator, named + default arguments.

data class Employee(val name: String, val department: String, val salary: Double)

fun averageSalaryByDepartment(employees: List<Employee>): Map<String, Double> {
    return employees
        .groupBy { it.department }
        .mapValues { (_, group) -> group.map { it.salary }.average() }
}

fun topEarners(employees: List<Employee>, count: Int = 3): List<Employee> {
    return employees.sortedByDescending { it.salary }.take(count)
}

fun namesJoined(employees: List<Employee>, separator: String = ", ", prefix: String = ""): String {
    return employees.joinToString(separator = separator, prefix = prefix) { it.name }
}

fun sumOfVarargs(vararg numbers: Int): Int {
    var total = 0
    for (n in numbers) total += n
    return total
}

fun spreadDemo(): Int {
    val values = intArrayOf(1, 2, 3, 4)
    return sumOfVarargs(*values, 5, 6)
}

fun buildList2(vararg items: String): List<String> = listOf(*items)

fun sequenceDemo(limit: Int): List<Int> {
    return generateSequence(1) { it + 1 }
        .filter { it % 2 == 0 }
        .map { it * it }
        .take(limit)
        .toList()
}

fun partitionByThreshold(numbers: List<Int>, threshold: Int): Pair<List<Int>, List<Int>> {
    return numbers.partition { it >= threshold }
}

fun flattenNested(nested: List<List<Int>>): List<Int> = nested.flatten()

fun flatMapExample(words: List<String>): List<Char> = words.flatMap { it.toList() }

fun reduceExample(numbers: List<Int>): Int = numbers.reduce { acc, n -> acc + n }

fun zipWithNext(numbers: List<Int>): List<Pair<Int, Int>> = numbers.zipWithNext()

fun associateExample(employees: List<Employee>): Map<String, Employee> {
    return employees.associateBy { it.name }
}

fun chunkedExample(numbers: List<Int>, size: Int): List<List<Int>> = numbers.chunked(size)

fun windowedExample(numbers: List<Int>, size: Int): List<List<Int>> = numbers.windowed(size)

fun namedArgumentsDemo(a: Int, b: Int = 10, c: Int = 20): Int {
    return a + b + c
}

fun callWithNamedArgs(): Int {
    return namedArgumentsDemo(a = 1, c = 5, b = 2)
}

fun demoCollections() {
    val employees = listOf(
        Employee("Ann", "Eng", 90000.0),
        Employee("Bo", "Eng", 95000.0),
        Employee("Cy", "Sales", 70000.0),
    )
    println(averageSalaryByDepartment(employees))
    println(topEarners(employees, 2))
    println(namesJoined(employees))
    println(sequenceDemo(5))
}
