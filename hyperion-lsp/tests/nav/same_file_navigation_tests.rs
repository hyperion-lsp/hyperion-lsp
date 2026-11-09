// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for goto definition within the same file

lsp_test_suite! {
    name: "Same File Variable Navigation",

    test local_variable_to_declaration {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        String message = "hello";
                        System.out.println(mes<|>sage);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test parameter_reference {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void greet(String name) {
                        System.out.println("Hello, " + na<|>me);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test field_reference_in_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    private String message;

                    public void printMessage() {
                        System.out.println(mes<|>sage);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }
}

lsp_test_suite! {
    name: "Same File Method Navigation",

    test method_call_to_definition {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        hel<|>per();
                    }

                    private void helper() {
                        System.out.println("Helper");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    // Bug reproduction: method call with String parameter
    test method_call_with_string_param {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        String url = "http://example.com";
                        validate<|>ImageMediaUrl(url);
                    }

                    private void validateImageMediaUrl(String url) {
                        System.out.println(url);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test this_method_call {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        this.hel<|>per();
                    }

                    private void helper() {
                        System.out.println("Helper");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test constructor_call {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    private String value;

                    public Main(String value) {
                        this.value = value;
                    }

                    public void test() {
                        Main m = new Ma<|>in("test");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }
}

lsp_test_suite! {
    name: "Same File Type Navigation",

    test class_reference_to_declaration {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Mai<|>n other = new Main();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test inner_class_reference {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Hel<|>per helper = new Helper();
                    }

                    static class Helper {
                        public void help() {
                            System.out.println("Helping");
                        }
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }
}

lsp_test_suite! {
    name: "Same File Field Navigation",

    test field_access_to_declaration {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    private int count;

                    public void increment() {
                        cou<|>nt++;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test this_field_access {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    private int count;

                    public void increment() {
                        this.cou<|>nt++;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test static_field_access {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    private static final String VERSION = "1.0";

                    public void printVersion() {
                        System.out.println(VER<|>SION);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test static_private_field_cursor_position {
        files: {
            "com/example/Config.java" => r#"
                package com.example;

                public class Config {
                    private static String <|1>DEFAULT_VALUE = "default";

                    public static String getDefault() {
                        return DEFAULT_V<|2>ALUE;
                    }
                }
            "#
        },
        nav: "com/example/Config.java", "<|2>" => "<|1>"
    }
}

lsp_test_suite! {
    name: "Same File Complex Scenarios",

    test chained_method_calls {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public Main getThis() {
                        return this;
                    }

                    public void print() {
                        System.out.println("Main");
                    }

                    public void test() {
                        getThis().pri<|>nt();
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test variable_with_same_type_as_class {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Main main = new Main();
                        main.hel<|>per();
                    }

                    private void helper() {
                        System.out.println("Helper");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test method_overloading {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void process(String s) {
                        System.out.println(s);
                    }

                    public void process(int i) {
                        System.out.println(i);
                    }

                    public void test() {
                        proc<|>ess("hello");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }
}

lsp_test_suite! {
    name: "Try-With-Resources Navigation",

    test try_with_resources_single_resource {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.io.BufferedReader;

                public class Main {
                    public void test() throws Exception {
                        try (BufferedReader reader = getReader()) {
                            String line = rea<|>der.readLine();
                        }
                    }

                    private BufferedReader getReader() { return null; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test try_with_resources_multiple_resources {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.sql.Connection;
                import java.sql.PreparedStatement;

                public class Main {
                    public void test(Connection connection) throws Exception {
                        try (PreparedStatement statement = connection.prepareStatement("SELECT 1")) {
                            stat<|>ement.setString(1, "test");
                        }
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test try_with_resources_nested {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.sql.Connection;
                import java.sql.PreparedStatement;
                import java.sql.ResultSet;

                public class Main {
                    public void test(Connection connection) throws Exception {
                        try (Connection conn = getConnection()) {
                            try (PreparedStatement statement = conn.prepareStatement("SELECT 1")) {
                                try (ResultSet rs = statement.executeQuery()) {
                                    while (r<|>s.next()) {
                                        rs.getString("id");
                                    }
                                }
                            }
                        }
                    }

                    private Connection getConnection() { return null; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }
}
