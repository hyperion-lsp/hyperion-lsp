package corpus.data

// Data classes: components, copy, defaults, nullable fields, nested data.

data class User(
    val id: Long,
    val name: String,
    val email: String?,
    var active: Boolean = true,
)

data class Address(val street: String, val city: String, val zip: String = "00000")

data class Order(
    val id: Long,
    val user: User,
    val address: Address,
    val items: List<String> = emptyList(),
    val total: Double = 0.0,
)

fun renamePlaceholder(user: User, newName: String): User {
    return user.copy(name = newName)
}

fun describeUser(user: User): String {
    val (id, name, email) = user
    return "User #$id: $name <${email ?: "no email"}>"
}

data class Pair3<A, B, C>(val first: A, val second: B, val third: C)

fun swapFirstTwo(triple: Pair3<Int, Int, String>): Pair3<Int, Int, String> {
    return triple.copy(first = triple.second, second = triple.first)
}

data class Money(val cents: Long, val currency: String = "USD") {
    operator fun plus(other: Money): Money {
        require(currency == other.currency) { "currency mismatch" }
        return Money(cents + other.cents, currency)
    }

    fun format(): String = "%.2f %s".format(cents / 100.0, currency)
}

fun totalOf(orders: List<Order>): Double = orders.sumOf { it.total }

fun sampleOrder(): Order {
    val user = User(1, "Charlie", "charlie@example.com")
    val addr = Address("Main St 1", "Springfield")
    return Order(id = 42, user = user, address = addr, items = listOf("book", "pen"), total = 19.99)
}
