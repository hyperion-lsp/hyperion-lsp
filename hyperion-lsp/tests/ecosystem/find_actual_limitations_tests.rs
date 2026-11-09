// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests to discover the actual limitations of hyperion-lsp
// Based on what we've learned, let's find what DOESN'T work

// These tests explore complex scenarios to find limitations

lsp_test_suite! {
    name: "Complex Scenarios That Might Work",

    // Test 1: Method chaining - does it work across multiple calls?
    test method_chain_two_calls {
        files: {
            "Test.java" => r#"
                package test;

                class StringBuilder {
                    StringBuilder append(String s) { return this; }
                    public String toString() { return ""; }
                }

                class Test {
                    void test() {
                        StringBuilder sb = new StringBuilder();
                        String result = sb.append("hello").toStr<|>ing();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 2: Field access on a typed variable
    test field_access_on_variable {
        files: {
            "Test.java" => r"
                package test;

                class User {
                    String name;
                }

                class Test {
                    void test() {
                        User user = new User();
                        String n = user.na<|>me;
                    }
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 3: Return type inference from method call
    test return_type_inference {
        files: {
            "Test.java" => r#"
                package test;

                class Factory {
                    User createUser() {
                        return new User();
                    }
                }

                class User {
                    String getName() { return ""; }
                }

                class Test {
                    void test() {
                        Factory factory = new Factory();
                        User user = factory.createUser();
                        String name = user.getN<|>ame();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 4: Chaining method call on return value
    test chain_on_method_return {
        files: {
            "Test.java" => r#"
                package test;

                class Factory {
                    User createUser() {
                        return new User();
                    }
                }

                class User {
                    String getName() { return ""; }
                }

                class Test {
                    void test() {
                        Factory factory = new Factory();
                        String name = factory.createUser().getN<|>ame();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 5: Cross-file type inference
    test cross_file_type_inference {
        files: {
            "test/User.java" => r#"
                package test;

                public class User {
                    public String getName() { return ""; }
                }
            "#,
            "test/Test.java" => r"
                package test;

                class Test {
                    void test() {
                        User user = new User();
                        String name = user.getN<|>ame();
                    }
                }
            "
        },
        nav: "test/Test.java", "<|>" => "User.java"
    }

    // Test 6: Parameter type resolution
    test parameter_method_call {
        files: {
            "Test.java" => r#"
                package test;

                class User {
                    String getName() { return ""; }
                }

                class Test {
                    void process(User user) {
                        String name = user.getN<|>ame();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 7: Field type resolution
    test field_method_call {
        files: {
            "Test.java" => r#"
                package test;

                class User {
                    String getName() { return ""; }
                }

                class Test {
                    User user = new User();

                    void test() {
                        String name = user.getN<|>ame();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 8: Method call on this
    test method_call_on_this {
        files: {
            "Test.java" => r"
                package test;

                class Test {
                    User getUser() {
                        return new User();
                    }

                    void test() {
                        User user = this.getU<|>ser();
                    }
                }

                class User {}
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 9: Generic type instantiation
    test generic_list_method {
        files: {
            "Test.java" => r"
                package test;

                import java.util.List;
                import java.util.ArrayList;

                class Test {
                    void test() {
                        List<String> list = new ArrayList<>();
                        int size = list.si<|>ze();
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;

                public interface List<E> {
                    int size();
                }
            ",
            "java/util/ArrayList.java" => r"
                package java.util;

                public class ArrayList<E> implements List<E> {
                    public int size() { return 0; }
                }
            "
        },
        nav: "Test.java", "<|>" => "List.java"
    }

    // Test 10: Builder pattern
    test builder_pattern_chain {
        files: {
            "Test.java" => r#"
                package test;

                class Builder {
                    Builder setName(String name) { return this; }
                    Builder setAge(int age) { return this; }
                    User build() { return new User(); }
                }

                class User {}

                class Test {
                    void test() {
                        User user = new Builder()
                            .setName("John")
                            .setAge(30)
                            .bu<|>ild();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }
}

// Tests for scenarios that might NOT work

lsp_goto_none_test! {
    name: lambda_method_inference,
    files: {
        "Test.java" => r"
            package test;

            interface Callback {
                void onResult(String result);
            }

            class Test {
                void process(Callback callback) {}

                void test() {
                    process(result -> result.length<|>());
                }
            }
        "
    },
    nav: "Test.java", "<|>"
}

lsp_goto_none_test! {
    name: method_reference_inference,
    files: {
        "Test.java" => r"
            package test;

            import java.util.List;

            class Test {
                void test(List<String> list) {
                    list.forEach(System.out::print<|>ln);
                }
            }
        "
    },
    nav: "Test.java", "<|>"
}
