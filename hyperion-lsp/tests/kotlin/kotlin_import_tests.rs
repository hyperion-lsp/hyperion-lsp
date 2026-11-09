// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Import navigation tests for Kotlin
//
// These tests verify that goto-definition works for import statements:
// - Navigate FROM import statement TO imported symbol
// - Support explicit imports (import X)
// - Support wildcard imports (import X.*)
// - Support aliased imports (import X as Y)

lsp_test_suite! {
    name: "Kotlin Import - Navigate from Import Statement",

    test navigate_to_explicit_class_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.model.Us<|>er

                fun main() {
                    val user = User()
                }
            ",
            "com/example/model/User.kt" => r#"
                package com.example.model

                class User {
                    val name: String = ""
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test navigate_to_explicit_function_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.utils.help<|>er

                fun main() {
                    helper()
                }
            ",
            "com/example/utils/Utils.kt" => r#"
                package com.example.utils

                fun helper() {
                    println("Helper")
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "Utils.kt"
    }

    test navigate_to_explicit_property_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.constants.MAX_V<|>ALUE

                fun main() {
                    println(MAX_VALUE)
                }
            ",
            "com/example/constants/Constants.kt" => r"
                package com.example.constants

                const val MAX_VALUE = 100
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Constants.kt"
    }
}

lsp_test_suite! {
    name: "Kotlin Import - Resolve Via Explicit Import",

    test resolve_class_via_explicit_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.model.User

                fun main() {
                    val user: Us<|>er = User()
                }
            ",
            "com/example/model/User.kt" => r#"
                package com.example.model

                class User {
                    val name: String = ""
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test resolve_function_via_explicit_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.utils.helper

                fun main() {
                    help<|>er()
                }
            ",
            "com/example/utils/Utils.kt" => r#"
                package com.example.utils

                fun helper() {
                    println("Helper")
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "Utils.kt"
    }

    test resolve_property_via_explicit_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.constants.MAX_VALUE

                fun main() {
                    println(MAX_V<|>ALUE)
                }
            ",
            "com/example/constants/Constants.kt" => r"
                package com.example.constants

                const val MAX_VALUE = 100
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Constants.kt"
    }
}

lsp_test_suite! {
    name: "Kotlin Import - Wildcard Imports",

    test resolve_class_via_wildcard_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.model.*

                fun main() {
                    val user: Us<|>er = User()
                }
            ",
            "com/example/model/User.kt" => r#"
                package com.example.model

                class User {
                    val name: String = ""
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test resolve_function_via_wildcard_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.utils.*

                fun main() {
                    help<|>er()
                }
            ",
            "com/example/utils/Utils.kt" => r#"
                package com.example.utils

                fun helper() {
                    println("Helper")
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "Utils.kt"
    }

    test resolve_multiple_via_wildcard_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.model.*

                fun main() {
                    val user = Us<|>er()
                    val product = Product()
                }
            ",
            "com/example/model/User.kt" => r#"
                package com.example.model

                class User {
                    val name: String = ""
                }
            "#,
            "com/example/model/Product.kt" => r#"
                package com.example.model

                class Product {
                    val title: String = ""
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }
}

goto_test! {
    ambiguous_wildcard_type_imports_do_not_choose_the_first_file {
        file "use/Main.kt" r"
            package use

            import alpha.*
            import beta.*

            val value: Us<|>er? = null
        ";
        file "alpha/User.kt" "package alpha\nclass User";
        file "beta/User.kt" "package beta\nclass User";
        target "use/Main.kt" "<|>";
        expect none;
    }
}

goto_test! {
    ambiguous_wildcard_function_imports_do_not_choose_the_first_file {
        file "use/Main.kt" r"
            package use

            import alpha.*
            import beta.*

            fun run() = pi<|>ck()
        ";
        file "alpha/Functions.kt" "package alpha\nfun pick(): Int = 1";
        file "beta/Functions.kt" "package beta\nfun pick(): Int = 2";
        target "use/Main.kt" "<|>";
        expect none;
    }
}

goto_test! {
    ambiguous_wildcard_extension_imports_do_not_choose_the_first_file {
        file "use/Main.kt" r#"
            package use

            import alpha.*
            import beta.*

            fun run() = "value".ta<|>g()
        "#;
        file "alpha/Extensions.kt" "package alpha\nfun String.tag(): Int = 1";
        file "beta/Extensions.kt" "package beta\nfun String.tag(): Int = 2";
        target "use/Main.kt" "<|>";
        expect none;
    }
}

goto_test! {
    aliased_extension_import_uses_the_imported_callable_identity {
        file "use/Main.kt" r#"
            package use

            import alpha.tag as label

            fun run() = "value".la<|>bel()
        "#;
        file "alpha/Extensions.kt" "package alpha\nfun String.«declaration»tag(): Int = 1";
        target "use/Main.kt" "<|>";
        expect "alpha/Extensions.kt" "declaration";
    }
}

lsp_test_suite! {
    name: "Kotlin Import - Aliased Imports",

    test resolve_class_via_aliased_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.model.User as U

                fun main() {
                    val user: <|>U = U()
                }
            ",
            "com/example/model/User.kt" => r#"
                package com.example.model

                class User {
                    val name: String = ""
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test resolve_function_via_aliased_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.utils.helper as h

                fun main() {
                    <|>h()
                }
            ",
            "com/example/utils/Utils.kt" => r#"
                package com.example.utils

                fun helper() {
                    println("Helper")
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "Utils.kt"
    }

    test navigate_from_aliased_import {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.model.User as <|>U

                fun main() {
                    val user = U()
                }
            ",
            "com/example/model/User.kt" => r#"
                package com.example.model

                class User {
                    val name: String = ""
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }
}

lsp_test_suite! {
    name: "Kotlin Import - Complex Import Scenarios",

    test multiple_explicit_imports {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.model.User
                import com.example.model.Product
                import com.example.utils.helper

                fun main() {
                    val user = Us<|>er()
                    val product = Product()
                    helper()
                }
            ",
            "com/example/model/User.kt" => r#"
                package com.example.model

                class User {
                    val name: String = ""
                }
            "#,
            "com/example/model/Product.kt" => r#"
                package com.example.model

                class Product {
                    val title: String = ""
                }
            "#,
            "com/example/utils/Utils.kt" => r#"
                package com.example.utils

                fun helper() {
                    println("Helper")
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test mixed_import_styles {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.model.*
                import com.example.utils.helper as h
                import com.example.constants.MAX_VALUE

                fun main() {
                    val user = Us<|>er()
                    h()
                    println(MAX_VALUE)
                }
            ",
            "com/example/model/User.kt" => r#"
                package com.example.model

                class User {
                    val name: String = ""
                }
            "#,
            "com/example/utils/Utils.kt" => r#"
                package com.example.utils

                fun helper() {
                    println("Helper")
                }
            "#,
            "com/example/constants/Constants.kt" => r"
                package com.example.constants

                const val MAX_VALUE = 100
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test same_package_no_import_needed {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val user = Us<|>er()
                }
            ",
            "com/example/User.kt" => r#"
                package com.example

                class User {
                    val name: String = ""
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }
}
