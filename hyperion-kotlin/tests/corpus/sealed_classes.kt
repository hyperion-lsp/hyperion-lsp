package corpus.sealed

// Sealed classes/interfaces with exhaustive `when`.

sealed class Result<out T> {
    data class Success<T>(val value: T) : Result<T>()
    data class Failure(val message: String, val cause: Throwable? = null) : Result<Nothing>()
    object Loading : Result<Nothing>()
}

fun <T> describe(result: Result<T>): String = when (result) {
    is Result.Success -> "Success: ${result.value}"
    is Result.Failure -> "Failure: ${result.message}"
    Result.Loading -> "Loading..."
}

sealed interface NetworkState {
    object Connected : NetworkState
    object Disconnected : NetworkState
    data class Error(val exception: Exception) : NetworkState
    data class Reconnecting(val attempt: Int) : NetworkState
}

fun handleState(state: NetworkState): String = when (state) {
    is NetworkState.Connected -> "online"
    is NetworkState.Disconnected -> "offline"
    is NetworkState.Error -> "error: ${state.exception.message}"
    is NetworkState.Reconnecting -> "retrying (#${state.attempt})"
}

sealed class Shape {
    class Circle(val radius: Double) : Shape()
    class Square(val side: Double) : Shape()
    class Triangle(val base: Double, val height: Double) : Shape()
}

fun area(shape: Shape): Double = when (shape) {
    is Shape.Circle -> Math.PI * shape.radius * shape.radius
    is Shape.Square -> shape.side * shape.side
    is Shape.Triangle -> 0.5 * shape.base * shape.height
}

sealed class Expr {
    data class Num(val value: Int) : Expr()
    data class Add(val left: Expr, val right: Expr) : Expr()
    data class Mul(val left: Expr, val right: Expr) : Expr()
    data class Neg(val operand: Expr) : Expr()
}

fun eval(expr: Expr): Int = when (expr) {
    is Expr.Num -> expr.value
    is Expr.Add -> eval(expr.left) + eval(expr.right)
    is Expr.Mul -> eval(expr.left) * eval(expr.right)
    is Expr.Neg -> -eval(expr.operand)
}

fun sampleExpr(): Expr = Expr.Add(Expr.Num(1), Expr.Mul(Expr.Num(2), Expr.Neg(Expr.Num(3))))
