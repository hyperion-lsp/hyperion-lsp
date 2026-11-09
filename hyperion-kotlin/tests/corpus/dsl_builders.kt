package corpus.dsl

// DSL-style builders: receiver lambdas, infix helpers, nested builder blocks.
//
// KNOWN PARSER GAP: a receiver-lambda function type (`Type.() -> R`, used
// pervasively below and essential to any real Kotlin DSL) is parsed with
// zero reported errors, but the tree is *not* lossless: `parse_type_ref`'s
// backtrack-to-the-dot logic (hyperion-kotlin/src/parser.rs, around the
// "Check for receiver type in function type" block) resets `self.pos` to
// re-read the dot but does not undo the token the first `bump()` already
// pushed into the tree, so the `.` is emitted twice (e.g. `Body.() ->
// Unit` round-trips as `Body..() -> Unit`). This is a "misleading 0
// errors" class bug — see tests/parser_termination.rs's docstring for the
// same class of prior bug — caught here only because mass_parse_test.rs's
// corpus check additionally verifies the parse is lossless, not just
// error-free. Kept on the known-failures ratchet in mass_parse_test.rs.

class Html {
    private val children = mutableListOf<String>()

    fun body(block: Body.() -> Unit) {
        val body = Body()
        body.block()
        children.add(body.render())
    }

    fun render(): String = "<html>${children.joinToString("")}</html>"
}

class Body {
    private val children = mutableListOf<String>()

    fun h1(text: String) {
        children.add("<h1>$text</h1>")
    }

    fun p(block: (() -> String)? = null) {
        children.add("<p>${block?.invoke() ?: ""}</p>")
    }

    fun ul(block: Ul.() -> Unit) {
        val ul = Ul()
        ul.block()
        children.add(ul.render())
    }

    fun render(): String = "<body>${children.joinToString("")}</body>"
}

class Ul {
    private val items = mutableListOf<String>()

    fun li(text: String) {
        items.add("<li>$text</li>")
    }

    fun render(): String = "<ul>${items.joinToString("")}</ul>"
}

fun html(block: Html.() -> Unit): String {
    val root = Html()
    root.block()
    return root.render()
}

fun demoHtmlDsl(): String = html {
    body {
        h1("Title")
        ul {
            li("one")
            li("two")
            li("three")
        }
    }
}

class RouteBuilder {
    private val routes = mutableMapOf<String, () -> String>()

    infix fun String.handledBy(handler: () -> String) {
        routes[this] = handler
    }

    fun build(): Map<String, () -> String> = routes
}

fun routes(block: RouteBuilder.() -> Unit): Map<String, () -> String> {
    val builder = RouteBuilder()
    builder.block()
    return builder.build()
}

fun demoRoutesDsl(): Map<String, () -> String> = routes {
    "/" handledBy { "home" }
    "/about" handledBy { "about page" }
}

class QueryBuilder {
    private val conditions = mutableListOf<String>()
    private var table = ""

    fun from(name: String): QueryBuilder {
        table = name
        return this
    }

    fun andWhere(condition: String): QueryBuilder {
        conditions.add(condition)
        return this
    }

    fun build(): String {
        val whereClause = if (conditions.isEmpty()) "" else " WHERE ${conditions.joinToString(" AND ")}"
        return "SELECT * FROM $table$whereClause"
    }
}

fun demoQueryDsl(): String {
    return QueryBuilder()
        .from("users")
        .andWhere("age > 18")
        .andWhere("active = true")
        .build()
}
