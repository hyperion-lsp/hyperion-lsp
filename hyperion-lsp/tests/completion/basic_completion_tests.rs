// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test 1: Basic completion - just check it works
lsp_completion_test! {
    name: test_basic_completion,
    files: {
        "Test.java" => r"
            class Test {
                void test() {
                    int count<|> = 0;
                }
            }
        "
    },
    complete: "Test.java", "<|>"
}

// Test 2: Completion with contains assertion
lsp_completion_test! {
    name: test_keyword_completion,
    files: {
        "Test.java" => r"
            class Test {
                void test() {
                    <|>
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["if", "for", "while", "return"]
}

// Test 3: Completion after '.' should show public String methods, not keywords or private methods
lsp_completion_test! {
    name: test_member_access_no_keywords,
    files: {
        "Test.java" => r#"
            class Test {
                void test() {
                    String s = "";
                    s.<|>
                }
            }
        "#
    },
    complete: "Test.java", "<|>",
    contains: ["length", "charAt", "substring", "equals"],  // Public instance methods
    not_contains: ["if", "for", "while", "coder", "isLatin1"]  // No keywords or private methods
}

// Test 4: Symbol completions
lsp_completion_test! {
    name: test_symbol_completion,
    files: {
        "Test.java" => r"
            class Test {
                private String name;
                private int age;

                void test() {
                    <|>
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["Test", "test"]
}

// Test 5: Local variable completion
lsp_completion_test! {
    name: test_local_variable_completion,
    files: {
        "Test.java" => r#"
            class Test {
                void test() {
                    int count = 0;
                    String message = "hello";
                    <|>
                }
            }
        "#
    },
    complete: "Test.java", "<|>",
    contains: ["count", "message"]
}

// Test 6: Parameter completion
lsp_completion_test! {
    name: test_parameter_completion,
    files: {
        "Test.java" => r"
            class Test {
                void test(String name, int age) {
                    <|>
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["name", "age"]
}

// Test 7: Field completion in method
lsp_completion_test! {
    name: test_field_completion_in_method,
    files: {
        "Test.java" => r"
            class Test {
                private String instanceField;

                void test() {
                    <|>
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["instanceField"]
}

// Test 8: Static method completion on class name (public methods only)
lsp_completion_test! {
    name: test_static_method_completion,
    files: {
        "Test.java" => r"
            class Test {
                void test() {
                    String.<|>
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["format", "join", "copyValueOf", "valueOf"],
    not_contains: ["checkBoundsBeginEnd", "decodeASCII", "serialVersionUID"]  // Private/package-private
}

// Test 9: Package-private visibility - same package (should see package-private members)
lsp_completion_test! {
    name: test_same_package_package_private,
    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {
                public String name;
                protected String email;
                String phone;  // package-private
                private String ssn;

                public String getName() { return name; }
                protected String getEmail() { return email; }
                String getPhone() { return phone; }  // package-private
                private String getSsn() { return ssn; }
            }
        ",
        "com/example/UserService.java" => r"
            package com.example;

            public class UserService {
                public void test(User user) {
                    user.<|>
                }
            }
        "
    },
    complete: "com/example/UserService.java", "<|>",
    contains: ["name", "email", "phone", "getName", "getEmail", "getPhone"],  // public, protected, package-private
    not_contains: ["ssn", "getSsn"]  // private members
}

// Test 10: Package-private visibility - different package (should NOT see package-private members)
lsp_completion_test! {
    name: test_different_package_package_private,
    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {
                public String name;
                protected String email;
                String phone;  // package-private
                private String ssn;

                public String getName() { return name; }
                protected String getEmail() { return email; }
                String getPhone() { return phone; }  // package-private
                private String getSsn() { return ssn; }
            }
        ",
        "com/other/Client.java" => r"
            package com.other;

            import com.example.User;

            public class Client {
                public void test(User user) {
                    user.<|>
                }
            }
        "
    },
    complete: "com/other/Client.java", "<|>",
    contains: ["name", "getName"],  // public only - protected not visible from different package without subclass
    not_contains: ["email", "getEmail", "phone", "getPhone", "ssn", "getSsn"]  // protected, package-private, and private
}

// Test 11: Package-private visibility - default package (no package declaration)
lsp_completion_test! {
    name: test_default_package_package_private,
    files: {
        "Helper.java" => r"
            public class Helper {
                public String publicField;
                String packageField;  // package-private
                private String privateField;

                public String getPublic() { return publicField; }
                String getPackage() { return packageField; }  // package-private
                private String getPrivate() { return privateField; }
            }
        ",
        "Main.java" => r"
            public class Main {
                public void test(Helper helper) {
                    helper.<|>
                }
            }
        "
    },
    complete: "Main.java", "<|>",
    contains: ["publicField", "packageField", "getPublic", "getPackage"],  // public and package-private (same default package)
    not_contains: ["privateField", "getPrivate"]  // private members
}
