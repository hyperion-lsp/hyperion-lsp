package corpus.nullablereceiver

// KNOWN PARSER GAP: an extension declaration whose receiver type is
// immediately followed by the member-access dot with no space, e.g.
// `fun Int?.orZero()`, is mis-lexed: the lexer greedily merges the `?` of
// the nullable receiver type and the following `.` into a single
// SAFE_CALL (`?.`) token regardless of syntactic context, which is correct
// inside an expression (`x?.foo()`) but wrong here, where `?` closes the
// receiver type and `.` is the plain extension-declaration separator.
// `fun Int?.orZero()` is an extremely common idiom (nullable extension
// receivers, e.g. stdlib's own `CharSequence?.isNullOrBlank()`). Kept on
// the known-failures ratchet in mass_parse_test.rs pending a lexer/parser
// fix that special-cases this position.

fun Int?.orZero(): Int = this ?: 0

fun Any?.describeNullability(): String = if (this == null) "is null" else "is not null"

fun String?.orEmpty2(): String = this ?: ""

fun List<Int>?.sumOrZero(): Int = this?.sum() ?: 0
