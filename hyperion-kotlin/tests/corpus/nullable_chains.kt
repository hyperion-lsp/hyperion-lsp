package corpus.nullability

// Nullable chains: safe calls, elvis, not-null assertion, safe casts,
// combined chains.

data class Address2(val city: String?, val zip: String?)
data class Profile(val address: Address2?)
data class User2(val profile: Profile?, val nickname: String?)

fun cityOf(user: User2?): String {
    return user?.profile?.address?.city ?: "Unknown city"
}

fun zipOrDefault(user: User2?): String {
    val zip = user?.profile?.address?.zip
    return zip ?: "00000"
}

fun forceNonNull(user: User2): String {
    return user.nickname!!
}

fun forceChain(user: User2): Int {
    return user.profile!!.address!!.city!!.length
}

fun safeCastExample(value: Any): String? {
    return value as? String
}

fun safeCastChain(value: Any?): Int {
    val text = value as? String ?: return -1
    return text.length
}

fun combinedSafeCallLet(user: User2?) {
    user?.nickname?.let {
        println("Nickname: $it")
    }
}

fun elvisWithThrow(user: User2?): Profile {
    return user?.profile ?: throw IllegalStateException("no profile")
}

fun elvisWithReturn(user: User2?): String {
    val nickname = user?.nickname ?: return "no user"
    return nickname.uppercase()
}

fun nullableFunctionCall(callback: (() -> String)?): String {
    return callback?.invoke() ?: "no callback"
}

fun safeIndexAccess(list: List<Int>?, index: Int): Int? {
    return list?.getOrNull(index)
}

fun chainedSafeCallsWithMethodCalls(user: User2?): String {
    return user?.profile?.address?.city?.trim()?.uppercase() ?: "N/A"
}

fun nullableGenericChain(map: Map<String, List<Int>?>?, key: String): Int {
    return map?.get(key)?.firstOrNull() ?: 0
}

fun main() {
    val user = User2(Profile(Address2("Oslo", "0150")), "kt")
    println(cityOf(user))
    println(cityOf(null))
    println(zipOrDefault(user))
    println(forceNonNull(user))
}
