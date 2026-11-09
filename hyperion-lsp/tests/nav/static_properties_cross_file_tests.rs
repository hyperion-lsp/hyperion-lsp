// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive static property access tests across files
// Tests for fully qualified access to static properties
// These tests validate the fix for goto-definition on qualified static properties
lsp_test_suite! {
    name: "Static Properties Cross-File",

    // Static property with fully qualified access
    test qualified_static_property {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        String value = com.example.config.AppConfig.DEFAULT_TIMEOUT<|>;
                    }
                }
            ",
            "com/example/config/AppConfig.java" => r"
                package com.example.config;
                public class AppConfig {
                    public static final int DEFAULT_TIMEOUT = 30000;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "AppConfig.java"
    }

    // Static property with nested class
    test nested_class_static_property {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        int max = Config.Limits.MAX_CONNECTIONS<|>;
                    }
                }
            ",
            "com/example/Config.java" => r"
                package com.example;
                public class Config {
                    public static class Limits {
                        public static final int MAX_CONNECTIONS = 100;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Config.java"
    }

    // Static property from different package
    test qualified_package_static_property {
        files: {
            "com/app/Main.java" => r"
                package com.app;
                import com.app.constants.SystemConstants;
                public class Main {
                    public void test() {
                        String version = com.app.constants.SystemConstants.APP_VERSION<|>;
                    }
                }
            ",
            "com/app/constants/SystemConstants.java" => r#"
                package com.app.constants;
                public class SystemConstants {
                    public static final String APP_VERSION = "1.0.0";
                }
            "#
        },
        nav: "com/app/Main.java", "<|>" => "SystemConstants.java"
    }

    // Static property - simple class name (not fully qualified)
    test simple_class_static_property {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        int timeout = Config.TIMEOUT<|>;
                    }
                }
            ",
            "com/example/Config.java" => r"
                package com.example;
                public class Config {
                    public static final int TIMEOUT = 5000;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Config.java"
    }

    // Static property - deeply nested packages
    test deeply_nested_static_property {
        files: {
            "org/example/app/Main.java" => r"
                package org.example.app;
                public class Main {
                    public void test() {
                        String key = org.example.config.db.DatabaseConfig.DB_KEY<|>;
                    }
                }
            ",
            "org/example/config/db/DatabaseConfig.java" => r#"
                package org.example.config.db;
                public class DatabaseConfig {
                    public static final String DB_KEY = "mydb";
                }
            "#
        },
        nav: "org/example/app/Main.java", "<|>" => "DatabaseConfig.java"
    }
}
