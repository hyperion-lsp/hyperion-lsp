// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test multiple cross-file symbols
lsp_completion_test! {
    name: test_multiple_cross_file_symbols,
    files: {
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                public void test() {
                    User<|>
                }
            }
        ",
        "com/example/UserService.java" => r"
            package com.example;

            public class UserService {
            }
        ",
        "com/example/UserRepository.java" => r"
            package com.example;

            public class UserRepository {
            }
        ",
        "com/example/User.java" => r"
            package com.example;

            public class User {
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["User", "UserService", "UserRepository"]
}

// Test method return type completion - calling someMethod() and pressing dot
// should show members of the return type
lsp_completion_test! {
    name: test_method_return_type_dot_completion,
    files: {
        "com/example/Main.java" => r"
            package com.example;

            import com.example.model.User;

            public class Main {
                private User getUser() {
                    return new User();
                }

                public void test() {
                    getUser().<|>
                }
            }
        ",
        "com/example/model/User.java" => r"
            package com.example.model;

            public class User {
                private String name;
                private String email;

                public String getName() { return name; }
                public String getEmail() { return email; }
                public void setName(String name) { this.name = name; }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["getName", "getEmail", "setName"],
    not_contains: ["name", "email"]
}

// Test inherited method return type completion
// Main extends UserService which defines getUser() returning User.
// Typing getUser(). in Main should resolve via the parent class hierarchy.
lsp_completion_test! {
    name: test_inherited_method_return_type_dot_completion,
    files: {
        "com/example/Main.java" => r"
            package com.example;
            import com.example.model.UserService;
            public class Main extends UserService {
                public void test() {
                    getUser().<|>
                }
            }
        ",
        "com/example/model/UserService.java" => r"
            package com.example.model;
            public class UserService {
                public User getUser() { return new User(); }
            }
        ",
        "com/example/model/User.java" => r#"
            package com.example.model;
            public class User {
                public String getName() { return ""; }
            }
        "#
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["getName"]
}

// Test inherited method return type when only the defining class imports the type.
// Main.java does NOT import Profile. Only UserService.java does. The return type
// of getProfile() should still resolve via UserService's imports.
lsp_completion_test! {
    name: test_inherited_method_return_type_uses_defining_file_imports,
    files: {
        "com/example/Main.java" => r"
            package com.example;
            import com.example.service.UserService;
            public class Main extends UserService {
                public void test() {
                    getProfile().<|>
                }
            }
        ",
        "com/example/service/UserService.java" => r"
            package com.example.service;
            import com.example.model.Profile;
            public class UserService {
                public Profile getProfile() { return new Profile(); }
            }
        ",
        "com/example/model/Profile.java" => r#"
            package com.example.model;
            public class Profile {
                public String getBio() { return ""; }
                public String getAvatar() { return ""; }
            }
        "#
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["getBio", "getAvatar"]
}

// Test overloaded method resolution by argument count.
// getUser() (0 args) returns User, getUser(int) returns Profile.
// Completing after getUser(). should show User's members (getName), not Profile's.
lsp_completion_test! {
    name: test_overloaded_method_resolution_by_arg_count,
    files: {
        "com/example/Main.java" => r"
            package com.example;

            import com.example.model.User;

            public class Main {
                private User getUser() { return new User(); }
                private com.example.model.Profile getUser(int id) { return null; }

                public void test() {
                    getUser().<|>
                }
            }
        ",
        "com/example/model/User.java" => r#"
            package com.example.model;

            public class User {
                public String getName() { return ""; }
            }
        "#,
        "com/example/model/Profile.java" => r#"
            package com.example.model;

            public class Profile {
                public String getBio() { return ""; }
            }
        "#
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["getName"]
}

// The return type of an inherited method is recovered through the
// canonical effective-method election, which is transitive through
// superinterfaces. The previous `get_parent_class` loop walked the superclass
// chain only, so an interface `default` method's return type was never found.
lsp_completion_test! {
    name: inherited_default_method_return_type_drives_chained_completion,
    files: {
        "com/example/Greeter.java" => r#"
            package com.example;
            public interface Greeter {
                default String greeting() {
                    return "hello";
                }
            }
        "#,
        "com/example/Console.java" => r"
            package com.example;
            public class Console implements Greeter {
                void run() {
                    greeting().<|>
                }
            }
        "
    },
    complete: "com/example/Console.java", "<|>",
    contains: ["length", "isEmpty"]
}
