// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Lombok annotation hover support
//!
//! Lombok generates fields and methods at compile time through annotation processing.
//! These tests verify that the LSP can provide hover information for Lombok-generated
//! symbols even though they don't exist in the source AST.

// Test 1: @Slf4j annotation generates org.slf4j.Logger log field
lsp_hover_test! {
    name: slf4j_logger_hover,

    files: {
        "com/example/Service.java" => r#"
            package com.example;
            import lombok.extern.slf4j.Slf4j;

            @Slf4j
            public class Service {
                public void execute() {
                    lo<|>g.info("test");
                }
            }
        "#
    },

    hover: "com/example/Service.java", "<|>",
    contains: ["org.slf4j.Logger", "log", "Slf4j"]
}

// Test 2: @Log4j2 annotation generates org.apache.logging.log4j.Logger log field
lsp_hover_test! {
    name: log4j2_logger_hover,

    files: {
        "com/example/Controller.java" => r#"
            package com.example;
            import lombok.extern.log4j.Log4j2;

            @Log4j2
            public class Controller {
                public void handle() {
                    lo<|>g.debug("processing");
                }
            }
        "#
    },

    hover: "com/example/Controller.java", "<|>",
    contains: ["org.apache.logging.log4j.Logger", "log", "Log4j2"]
}

// Test 3: @CommonsLog annotation generates org.apache.commons.logging.Log log field
lsp_hover_test! {
    name: commons_log_hover,

    files: {
        "com/example/Manager.java" => r#"
            package com.example;
            import lombok.extern.apachecommons.CommonsLog;

            @CommonsLog
            public class Manager {
                public void process() {
                    lo<|>g.warn("warning message");
                }
            }
        "#
    },

    hover: "com/example/Manager.java", "<|>",
    contains: ["org.apache.commons.logging.Log", "log", "CommonsLog"]
}

// Test 4: @Log annotation generates java.util.logging.Logger log field
lsp_hover_test! {
    name: java_util_logging_hover,

    files: {
        "com/example/Processor.java" => r#"
            package com.example;
            import lombok.extern.java.Log;

            @Log
            public class Processor {
                public void run() {
                    lo<|>g.fine("detail");
                }
            }
        "#
    },

    hover: "com/example/Processor.java", "<|>",
    contains: ["java.util.logging.Logger", "log"]
}

// Test 5: @XSlf4j annotation generates org.slf4j.ext.XLogger log field
lsp_hover_test! {
    name: xslf4j_logger_hover,

    files: {
        "com/example/AdvancedService.java" => r"
            package com.example;
            import lombok.extern.slf4j.XSlf4j;

            @XSlf4j
            public class AdvancedService {
                public void execute() {
                    lo<|>g.entry();
                }
            }
        "
    },

    hover: "com/example/AdvancedService.java", "<|>",
    contains: ["org.slf4j.ext.XLogger", "log", "XSlf4j"]
}

// Test 6: @Slf4j in nested class
lsp_hover_test! {
    name: slf4j_in_nested_class,

    files: {
        "com/example/Outer.java" => r#"
            package com.example;
            import lombok.extern.slf4j.Slf4j;

            public class Outer {
                @Slf4j
                public static class Inner {
                    public void test() {
                        lo<|>g.info("nested");
                    }
                }
            }
        "#
    },

    hover: "com/example/Outer.java", "<|>",
    contains: ["org.slf4j.Logger", "log"]
}

// Test 7: Fully qualified annotation name
lsp_hover_test! {
    name: fully_qualified_annotation,

    files: {
        "com/example/FullyQualified.java" => r#"
            package com.example;

            @lombok.extern.slf4j.Slf4j
            public class FullyQualified {
                public void test() {
                    lo<|>g.info("test");
                }
            }
        "#
    },

    hover: "com/example/FullyQualified.java", "<|>",
    contains: ["org.slf4j.Logger", "log"]
}

// Test 8: Static modifier shown in hover
lsp_hover_test! {
    name: static_modifier_in_hover,

    files: {
        "com/example/StaticCheck.java" => r#"
            package com.example;
            import lombok.extern.slf4j.Slf4j;

            @Slf4j
            public class StaticCheck {
                public void test() {
                    lo<|>g.info("test");
                }
            }
        "#
    },

    hover: "com/example/StaticCheck.java", "<|>",
    contains: ["static", "org.slf4j.Logger", "log"]
}
