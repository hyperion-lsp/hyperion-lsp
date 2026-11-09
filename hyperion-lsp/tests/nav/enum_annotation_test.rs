// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

lsp_test_suite! {
    name: "Enum with Annotations",

    test enum_deprecated_constant {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Status status = Status.ACTIVITY_T<|>IME;
                    }
                }
            ",
            "com/example/Status.java" => r"
                package com.example;

                public enum Status {
                    @Deprecated
                    ACTIVITY_TIME,

                    SUPPORT_GROUP_INVITED,
                    SUPPORT_GROUP_NOT_INVITED
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Status.java"
    }

    test enum_multiple_annotations {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Color color = Color.R<|>ED;
                    }
                }
            ",
            "com/example/Color.java" => r#"
                package com.example;

                public enum Color {
                    @Deprecated
                    @SuppressWarnings("unused")
                    RED,

                    GREEN,

                    @Deprecated
                    BLUE
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    test enum_with_method_and_annotated_constants {
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
                    ACTIVE,

                    @Deprecated
                    INACTIVE;

                    public int getCode() {
                        return 1;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Status.java"
    }
}
