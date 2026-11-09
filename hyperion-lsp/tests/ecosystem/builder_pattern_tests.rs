// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive Builder pattern tests
// Testing: Lombok @Builder, manual builders, fluent interfaces, method chaining
lsp_test_suite! {
    name: "Builder Pattern",

    // Lombok @Builder basic
    test lombok_builder_basic {
        files: {
            "com/example/User.java" => r"
                package com.example;
                import lombok.Builder;
                @Builder
                public class User {
                    private String name;
                    private int age;
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;
                public class Main {
                    public void test() {
                        User user = User.buil<|>der()
                            .name("John")
                            .age(30)
                            .build();
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Lombok @Builder method chaining
    test lombok_builder_chaining {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                public class Main {
                    public void test() {
                        User user = User.builder()
                            .na<|>me("John")
                            .age(30)
                            .build();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                import lombok.Builder;
                @Builder
                public class User {
                    private String name;
                    private int age;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Manual builder pattern
    test manual_builder {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                public class Main {
                    public void test() {
                        User user = new User.Buil<|>der()
                            .setName("John")
                            .setAge(30)
                            .build();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;
                    private int age;

                    public static class Builder {
                        private String name;
                        private int age;

                        public Builder setName(String name) {
                            this.name = name;
                            return this;
                        }

                        public Builder setAge(int age) {
                            this.age = age;
                            return this;
                        }

                        public User build() {
                            User user = new User();
                            user.name = this.name;
                            user.age = this.age;
                            return user;
                        }
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Builder method navigation
    test builder_method_nav {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                public class Main {
                    public void test() {
                        User user = new User.Builder()
                            .setNa<|>me("John")
                            .setAge(30)
                            .build();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    public static class Builder {
                        public Builder setName(String name) { return this; }
                        public Builder setAge(int age) { return this; }
                        public User build() { return new User(); }
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Fluent interface
    test fluent_interface {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                public class Main {
                    public void test() {
                        QueryBuilder query = new QueryBuilder()
                            .sel<|>ect("name", "age")
                            .from("users")
                            .where("age > 18");
                    }
                }
            "#,
            "com/example/QueryBuilder.java" => r"
                package com.example;
                public class QueryBuilder {
                    public QueryBuilder select(String... columns) { return this; }
                    public QueryBuilder from(String table) { return this; }
                    public QueryBuilder where(String condition) { return this; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "QueryBuilder.java"
    }

    // Complex builder chain
    test complex_builder_chain {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                public class Main {
                    public void test() {
                        Config config = Config.builder()
                            .host("localhost")
                            .port(8080)
                            .ssl(true)
                            .timeout(30)
                            .retries(3)
                            .buil<|>d();
                    }
                }
            "#,
            "com/example/Config.java" => r"
                package com.example;
                import lombok.Builder;
                @Builder
                public class Config {
                    private String host;
                    private int port;
                    private boolean ssl;
                    private int timeout;
                    private int retries;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Config.java"
    }
}
