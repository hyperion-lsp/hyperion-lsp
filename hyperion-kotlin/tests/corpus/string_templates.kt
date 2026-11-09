package corpus.strings

// String templates: simple, expression, nested, multiline/raw strings.

fun greet(name: String, age: Int): String {
    return "Hello, $name! You are $age years old."
}

fun describeList(items: List<Int>): String {
    return "The list has ${items.size} items, first is ${items.firstOrNull() ?: "none"}"
}

fun nestedTemplate(user: Map<String, String>): String {
    return "User: ${user["name"] ?: "unknown"} (${if (user.containsKey("email")) "has email" else "no email"})"
}

fun rawStringExample(): String {
    return """
        Line one
        Line two with a $variable-looking text
        Line three
    """.trimIndent()
}

const val variable = "not-a-template"

fun regexLikeRawString(): String {
    return """\d+-\d+-\d+"""
}

fun escapedCharacters(): String {
    return "Tab:\tNewline:\\nQuote:\"Backslash:\\\\"
}

fun multilineWithInterpolation(name: String): String {
    return """
        {
          "name": "$name",
          "computed": "${name.uppercase()}"
        }
    """.trimIndent()
}

fun complexInterpolation(a: Int, b: Int): String {
    return "Sum is ${a + b}, product is ${a * b}, and ${if (a > b) "$a > $b" else "$a <= $b"}"
}

fun templateWithFunctionCall(items: List<String>): String {
    return "Joined: ${items.joinToString(", ") { it.uppercase() }}"
}

fun dollarSignLiteral(): String {
    return "Price: \$${19.99}"
}

fun main() {
    println(greet("Ada", 36))
    println(describeList(listOf(1, 2, 3)))
    println(complexInterpolation(3, 4))
    println(dollarSignLiteral())
}
