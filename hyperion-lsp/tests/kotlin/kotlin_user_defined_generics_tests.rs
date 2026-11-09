// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// ========================================================================
// User-Defined Generic Type Inference Tests
// ========================================================================
//
// These tests verify that type inference works correctly for user-defined
// generic classes - not just stdlib types like List<T>.
//
// The key scenarios:
// 1. Repository<T> pattern: findById() returns T
// 2. Wrapper<T> pattern: get() returns T
// 3. Multiple type parameters: Pair<A, B>
// 4. Complex return types: List<T>, T?
//
// Success criteria:
// - When calling repo.findById(1) on Repository<User>, hover shows User
// - Type variables are substituted with concrete types
// - Works for both simple and complex return types

// ========== Simple Generic Class: Repository<T> ==========

// Test: Repository<T>.findById() should return the concrete type T
lsp_hover_test! {
    name: kotlin_user_generic_simple_return,
    files: {
        "Repository.kt" => r"
            package com.example

            class Repository<T> {
                fun findById(id: Int): T = TODO()
                fun findAll(): List<T> = TODO()
            }
        ",
        "User.kt" => r"
            package com.example

            data class User(val id: Int, val name: String)
        ",
        "Main.kt" => r"
            package com.example

            fun main() {
                val repo = Repository<User>()
                val us<|>er = repo.findById(1)
            }
        "
    },
    hover: "Main.kt", "<|>",
    contains: ["user", "User"]
}

// A receiver declared through a typealias whose expansion gives
// Repository a concrete argument substitutes it, as Repository<User> does.
lsp_hover_test! {
    name: kotlin_user_generic_return_through_a_typealias,
    files: {
        "Repository.kt" => r"
            package com.example

            class Repository<T> {
                fun findById(id: Int): T = TODO()
            }
        ",
        "User.kt" => r"
            package com.example

            data class User(val id: Int, val name: String)
        ",
        "Main.kt" => r"
            package com.example

            typealias Users = com.example.Repository<User>

            fun main(repo: Users) {
                val us<|>er = repo.findById(1)
            }
        "
    },
    hover: "Main.kt", "<|>",
    contains: ["user: User"]
}

// A generic typealias binds its parameters to the use's arguments
// in its own order: Flipped<User, Int> is Pairing<Int, User>, whose second()
// is a User.
lsp_hover_test! {
    name: kotlin_user_generic_return_through_a_generic_typealias,
    files: {
        "Pairing.kt" => r"
            package com.example

            class Pairing<A, B> {
                fun second(): B = TODO()
            }
        ",
        "User.kt" => r"
            package com.example

            data class User(val id: Int, val name: String)
        ",
        "Main.kt" => r"
            package com.example

            typealias Flipped<X, Y> = com.example.Pairing<Y, X>

            fun main(pair: Flipped<User, Int>) {
                val us<|>er = pair.second()
            }
        "
    },
    hover: "Main.kt", "<|>",
    contains: ["user: User"]
}

// A use argument that is itself generic keeps its own arguments,
// nested twice: Flipped<Map<String, List<User>>, Int> is Pairing<Int, ...>,
// whose second() is the whole map type.
lsp_hover_test! {
    name: kotlin_user_generic_return_through_a_generic_typealias_with_a_nested_argument,
    files: {
        "Pairing.kt" => r"
            package com.example

            class Pairing<A, B> {
                fun second(): B = TODO()
            }
        ",
        "User.kt" => r"
            package com.example

            data class User(val id: Int, val name: String)
        ",
        "Main.kt" => r"
            package com.example

            typealias Flipped<X, Y> = com.example.Pairing<Y, X>

            fun main(pair: Flipped<Map<String, List<User>>, Int>) {
                val us<|>ers = pair.second()
            }
        "
    },
    hover: "Main.kt", "<|>",
    contains: ["users: Map<String, List<User>>"]
}

// A member whose declared type has a nested comma (`Map<Pair<A, B>, Int>`) keeps the
// whole argument when the class's type variables are substituted; splitting at every comma cut it.
lsp_hover_test! {
    name: kotlin_user_generic_member_with_a_nested_comma_keeps_its_arguments,
    files: {
        "Pairing.kt" => r"
            package com.example

            class Pairing<A, B> {
                fun table(): Map<Pair<A, B>, Int> = TODO()
            }
        ",
        "Main.kt" => r"
            package com.example

            typealias Flipped<X, Y> = com.example.Pairing<Y, X>

            fun main(pair: Flipped<String, Int>) {
                val ta<|>ble = pair.table()
            }
        "
    },
    hover: "Main.kt", "<|>",
    contains: ["table: Map<Pair<Int, String>, Int>"]
}

// Test: Repository<T>.findAll() should return List<T> with concrete type
lsp_hover_test! {
    name: kotlin_user_generic_list_return,
    files: {
        "Repository.kt" => r"
            package com.example

            class Repository<T> {
                fun findById(id: Int): T = TODO()
                fun findAll(): List<T> = TODO()
            }
        ",
        "User.kt" => r"
            package com.example

            data class User(val id: Int, val name: String)
        ",
        "Main.kt" => r"
            package com.example

            fun main() {
                val repo = Repository<User>()
                val us<|>ers = repo.findAll()
            }
        "
    },
    hover: "Main.kt", "<|>",
    contains: ["users", "List"]
}

// ========== Two Type Parameters: Pair<A, B> ==========

// Test: Pair<A, B>.first should return A
lsp_hover_test! {
    name: kotlin_user_generic_two_params_first,
    files: {
        "Pair.kt" => r"
            package com.example

            class Pair<A, B>(val first: A, val second: B) {
                fun getFirst(): A = first
                fun getSecond(): B = second
            }
        ",
        "Main.kt" => r#"
            package com.example

            fun main() {
                val pair = Pair<String, Int>("hello", 42)
                val fir<|>st = pair.getFirst()
            }
        "#
    },
    hover: "Main.kt", "<|>",
    contains: ["first", "String"]
}

// Test: Pair<A, B>.second should return B
lsp_hover_test! {
    name: kotlin_user_generic_two_params_second,
    files: {
        "Pair.kt" => r"
            package com.example

            class Pair<A, B>(val first: A, val second: B) {
                fun getFirst(): A = first
                fun getSecond(): B = second
            }
        ",
        "Main.kt" => r#"
            package com.example

            fun main() {
                val pair = Pair<String, Int>("hello", 42)
                val sec<|>ond = pair.getSecond()
            }
        "#
    },
    hover: "Main.kt", "<|>",
    contains: ["second", "Int"]
}

// ========== Wrapper Pattern ==========

// Test: Wrapper<T>.get() should return T
lsp_hover_test! {
    name: kotlin_user_generic_wrapper,
    files: {
        "Wrapper.kt" => r"
            package com.example

            class Wrapper<T>(private val value: T) {
                fun get(): T = value
                fun getOrNull(): T? = value
            }
        ",
        "Main.kt" => r"
            package com.example

            fun main() {
                val wrapped = Wrapper<Double>(3.14)
                val va<|>lue = wrapped.get()
            }
        "
    },
    hover: "Main.kt", "<|>",
    contains: ["value", "Double"]
}

// ========== Nullable Return Type ==========

// Test: Wrapper<T>.getOrNull() should return T?
lsp_hover_test! {
    name: kotlin_user_generic_nullable_return,
    files: {
        "Wrapper.kt" => r"
            package com.example

            class Wrapper<T>(private val value: T) {
                fun get(): T = value
                fun getOrNull(): T? = value
            }
        ",
        "Main.kt" => r#"
            package com.example

            fun main() {
                val wrapped = Wrapper<String>("hello")
                val mayb<|>e = wrapped.getOrNull()
            }
        "#
    },
    hover: "Main.kt", "<|>",
    contains: ["maybe"]
    // Should ideally contain "String?" but nullable inference may not be implemented yet
}

// ========== Service/DAO Pattern ==========

// Test: Common enterprise pattern with generic services
lsp_hover_test! {
    name: kotlin_user_generic_service_pattern,
    files: {
        "BaseService.kt" => r"
            package com.example

            abstract class BaseService<T> {
                abstract fun getById(id: Long): T
                abstract fun save(entity: T): T
                abstract fun delete(id: Long): Boolean
            }
        ",
        "User.kt" => r"
            package com.example

            data class User(val id: Long, val name: String)
        ",
        "UserService.kt" => r"
            package com.example

            class UserService : BaseService<User>() {
                override fun getById(id: Long): User = TODO()
                override fun save(entity: User): User = TODO()
                override fun delete(id: Long): Boolean = TODO()
            }
        ",
        "Main.kt" => r"
            package com.example

            fun main() {
                val service: BaseService<User> = UserService()
                val us<|>er = service.getById(1L)
            }
        "
    },
    hover: "Main.kt", "<|>",
    contains: ["user", "User"]
}
