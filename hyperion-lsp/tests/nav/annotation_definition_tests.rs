// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for annotation type (@interface) definitions and usage
// Priority 1: Critical gap - annotations are ubiquitous in modern Java

use crate::lsp_test_framework::LspTest;

lsp_test_suite! {
    name: "Annotation Type Navigation",

    test navigate_to_annotation_definition {
        files: {
            "com/example/CustomAnnotation.java" => r#"
                package com.example;
                import java.lang.annotation.*;

                @Target(ElementType.METHOD)
                @Retention(RetentionPolicy.RUNTIME)
                public @interface CustomAnnotation {
                    String value() default "";
                    int priority() default 0;
                }
            "#,
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    @CustomAnnot<|>ation(value = "test", priority = 1)
                    public void test() {}
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "CustomAnnotation.java"
    }

    test navigate_to_meta_annotation {
        files: {
            "com/example/CustomAnnotation.java" => r"
                package com.example;
                import java.lang.annotation.*;

                @Targ<|>et(ElementType.METHOD)
                @Retention(RetentionPolicy.RUNTIME)
                public @interface CustomAnnotation {
                }
            ",
            "java/lang/annotation/Target.java" => r"
                package java.lang.annotation;
                public @interface Target {
                    ElementType[] value();
                }
            ",
            "java/lang/annotation/ElementType.java" => r"
                package java.lang.annotation;
                public enum ElementType {
                    METHOD, FIELD, TYPE
                }
            "
        },
        nav: "com/example/CustomAnnotation.java", "<|>" => "Target.java"
    }

    test navigate_to_retention_annotation {
        files: {
            "com/example/CustomAnnotation.java" => r"
                package com.example;
                import java.lang.annotation.*;

                @Target(ElementType.METHOD)
                @Retenti<|>on(RetentionPolicy.RUNTIME)
                public @interface CustomAnnotation {
                }
            ",
            "java/lang/annotation/Retention.java" => r"
                package java.lang.annotation;
                public @interface Retention {
                    RetentionPolicy value();
                }
            ",
            "java/lang/annotation/RetentionPolicy.java" => r"
                package java.lang.annotation;
                public enum RetentionPolicy {
                    SOURCE, CLASS, RUNTIME
                }
            "
        },
        nav: "com/example/CustomAnnotation.java", "<|>" => "Retention.java"
    }

    test annotation_with_class_parameter {
        files: {
            "com/example/UseCase.java" => r"
                package com.example;
                public @interface UseCase {
                    Class<?> value();
                }
            ",
            "com/example/Service.java" => r"
                package com.example;
                public class Service {}
            ",
            "com/example/Main.java" => r"
                package com.example;

                @UseCase(Servic<|>e.class)
                public class Main {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "Service.java"
    }

    test annotation_with_string_array_parameter {
        files: {
            "com/example/Tags.java" => r"
                package com.example;
                public @interface Tags {
                    String[] value();
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                @Ta<|>gs({"unit", "integration"})
                public class Main {}
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Tags.java"
    }

    test annotation_with_enum_parameter {
        files: {
            "com/example/Level.java" => r"
                package com.example;
                public enum Level {
                    LOW, MEDIUM, HIGH
                }
            ",
            "com/example/Priority.java" => r"
                package com.example;
                public @interface Priority {
                    Level value();
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                @Priority(Lev<|>el.HIGH)
                public class Main {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "Level.java"
    }

    test nested_annotation_parameter {
        files: {
            "com/example/Inner.java" => r"
                package com.example;
                public @interface Inner {
                    String value();
                }
            ",
            "com/example/Outer.java" => r"
                package com.example;
                public @interface Outer {
                    Inner[] value();
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                @Outer(@Inn<|>er("test"))
                public class Main {}
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Inner.java"
    }

    test annotation_on_field {
        files: {
            "com/example/NotNull.java" => r"
                package com.example;
                public @interface NotNull {}
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    @NotN<|>ull
                    private String name;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "NotNull.java"
    }

    test annotation_on_parameter {
        files: {
            "com/example/Validated.java" => r"
                package com.example;
                public @interface Validated {}
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(@Validat<|>ed String input) {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Validated.java"
    }

    test annotation_on_local_variable {
        files: {
            "com/example/SuppressWarnings.java" => r"
                package com.example;
                public @interface SuppressWarnings {
                    String[] value();
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        @SuppressWarnin<|>gs("unchecked")
                        Object obj = new Object();
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "SuppressWarnings.java"
    }

    test annotation_with_multiple_attributes {
        files: {
            "com/example/TestInfo.java" => r#"
                package com.example;
                public @interface TestInfo {
                    String author();
                    String date();
                    String description() default "";
                }
            "#,
            "com/example/Main.java" => r#"
                package com.example;

                @TestIn<|>fo(
                    author = "John",
                    date = "2024-01-01",
                    description = "Test case"
                )
                public class Main {}
            "#
        },
        nav: "com/example/Main.java", "<|>" => "TestInfo.java"
    }

    test marker_annotation {
        files: {
            "com/example/Deprecated.java" => r"
                package com.example;
                public @interface Deprecated {
                    // Marker annotation with no parameters
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                @Deprecat<|>ed
                public class Main {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "Deprecated.java"
    }
}

#[test]
fn ambiguous_unimported_annotation_does_not_elect_a_workspace_match() {
    let mut test = LspTest::new();
    test.add_file(
        "alpha/Marker.java",
        "package alpha; public @interface Marker {}",
    );
    test.add_file(
        "beta/Marker.java",
        "package beta; public @interface Marker {}",
    );
    test.add_file(
        "consumer/Main.java",
        "package consumer; @Mar<|>ker public class Main {}",
    );

    let result = test.goto_definition("consumer/Main.java", "<|>");
    assert!(
        matches!(result, Ok(None)),
        "an unbound annotation name must remain unresolved: {result:?}"
    );
}
