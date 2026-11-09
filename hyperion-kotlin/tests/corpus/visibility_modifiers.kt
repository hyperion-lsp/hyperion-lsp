package corpus.visibility

// Visibility modifiers on declarations, and (deliberately) on primary
// constructor value parameters.
//
// KNOWN PARSER GAP: `parse_parameter` (hyperion-kotlin/src/parser.rs) only
// consumes `vararg`/`noinline`/`crossinline` modifiers before `val`/`var` in
// a parameter list. It does not accept `private`/`protected`/`public`/
// `internal`/`override` there, even though `class Foo(private val x: Int)`
// and `class Foo(override val x: Int) : Base` are among the most common
// idioms in real Kotlin code. This file is intentionally kept on the
// known-failures ratchet in mass_parse_test.rs until that is fixed in the
// parser (out of scope for this harness/README task).

interface Named {
    val name: String
}

class OverridingHolder(override val name: String) : Named

class PublicHolder(val visible: Int)

class PrivateHolder(private val hidden: Int) {
    fun reveal(): Int = hidden
}

class ProtectedBase(protected val label: String)

class InternalHolder(internal val shared: String)

internal class TopLevelInternal(val value: Int)

private class TopLevelPrivate(val value: Int)

public class ExplicitPublic(val value: Int)

class MixedVisibilityMembers {
    public val everyone: Int = 1
    private val onlyMe: Int = 2
    protected open val subclasses: Int = 3
    internal val sameModule: Int = 4

    private fun helper(): Int = onlyMe + everyone

    protected open fun overridable(): Int = subclasses

    internal fun moduleApi(): Int = helper() + sameModule
}

class MultiVisibilityConstructor(
    private val a: Int,
    protected val b: Int,
    internal val c: Int,
    val d: Int,
)
