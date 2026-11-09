// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

lsp_test_suite! {
    name: "Enum Navigation and Synthetic Methods",

    // Test 1: Navigate to enum constant
    test enum_constant_reference {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Color color = Color.R<|>ED;
                    }
                }
            ",
            "com/example/Color.java" => r"
                package com.example;

                public enum Color {
                    RED, GREEN, BLUE
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    // Test 2: Navigate from constant to enum itself
    test enum_type_reference {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Col<|>or color = Color.RED;
                    }
                }
            ",
            "com/example/Color.java" => r"
                package com.example;

                public enum Color {
                    RED, GREEN, BLUE
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    // Test 3: Enum.values() static method
    test enum_values_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Color[] colors = Color.val<|>ues();
                    }
                }
            ",
            "com/example/Color.java" => r"
                package com.example;

                public enum Color {
                    RED, GREEN, BLUE
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    // Test 4: Enum.valueOf(String) static method
    test enum_valueof_method {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Color color = Color.value<|>Of("RED");
                    }
                }
            "#,
            "com/example/Color.java" => r"
                package com.example;

                public enum Color {
                    RED, GREEN, BLUE
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    // Test 5: Enum instance method - name()
    test enum_name_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Color color = Color.RED;
                        String name = color.na<|>me();
                    }
                }
            ",
            "com/example/Color.java" => r"
                package com.example;

                public enum Color {
                    RED, GREEN, BLUE
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    // Test 6: Enum instance method - ordinal()
    test enum_ordinal_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Color color = Color.RED;
                        int index = color.ordin<|>al();
                    }
                }
            ",
            "com/example/Color.java" => r"
                package com.example;

                public enum Color {
                    RED, GREEN, BLUE
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    // Test 7: Enum with custom fields and methods
    test enum_custom_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Color color = Color.RED;
                        String hex = color.toH<|>ex();
                    }
                }
            ",
            "com/example/Color.java" => r#"
                package com.example;

                public enum Color {
                    RED, GREEN, BLUE;

                    public String toHex() {
                        return "hex";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    // Test 8: Enum with getter via custom field
    test enum_field_getter {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Status status = Status.ACTIVE;
                        int code = status.getCo<|>de();
                    }
                }
            ",
            "com/example/Status.java" => r"
                package com.example;

                public enum Status {
                    ACTIVE, INACTIVE;

                    public int getCode() {
                        return 1;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Status.java"
    }

    // Test 9: Chain enum methods
    test enum_method_chain {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        String name = Color.RED.na<|>me().toLowerCase();
                    }
                }
            ",
            "com/example/Color.java" => r"
                package com.example;

                public enum Color {
                    RED, GREEN, BLUE
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    // Test 10: Multiple enum constants
    test enum_multiple_constants {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        if (status == Status.ACT<|>IVE) {
                            // do something
                        }
                    }

                    private Status status;
                }
            ",
            "com/example/Status.java" => r"
                package com.example;

                public enum Status {
                    ACTIVE,
                    INACTIVE,
                    PENDING,
                    ARCHIVED
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Status.java"
    }
}
