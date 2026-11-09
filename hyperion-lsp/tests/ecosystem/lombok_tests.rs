// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

lsp_test_suite! {
    name: "Lombok Annotation Support",

    test getter_annotation_field_level {
        files: {
            "com/example/User.java" => r"
                package com.example;
                import lombok.Getter;

                public class User {
                    @Getter
                    private String name;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(User user) {
                        user.getNa<|>me();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test setter_annotation_field_level {
        files: {
            "com/example/User.java" => r"
                package com.example;
                import lombok.Setter;

                public class User {
                    @Setter
                    private String email;
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(User user) {
                        user.setEma<|>il("test@example.com");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test data_annotation_class_level {
        files: {
            "com/example/Person.java" => r"
                package com.example;
                import lombok.Data;

                @Data
                public class Person {
                    private String firstName;
                    private String lastName;
                    private int age;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(Person p) {
                        p.getFirstNa<|>me();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Person.java"
    }

    test data_annotation_setter {
        files: {
            "com/example/Person.java" => r"
                package com.example;
                import lombok.Data;

                @Data
                public class Person {
                    private String name;
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(Person p) {
                        p.setNa<|>me("John");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Person.java"
    }

    test builder_annotation {
        files: {
            "com/example/User.java" => r"
                package com.example;
                import lombok.Builder;

                @Builder
                public class User {
                    private String username;
                    private String email;
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = User.buil<|>der()
                            .username("john")
                            .email("john@example.com")
                            .build();
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test all_args_constructor {
        files: {
            "com/example/Product.java" => r"
                package com.example;
                import lombok.AllArgsConstructor;

                @AllArgsConstructor
                public class Product {
                    private String name;
                    private double price;
                    private int quantity;
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Product p = new Produ<|>ct("Widget", 9.99, 100);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Product.java"
    }

    test value_annotation_immutable {
        files: {
            "com/example/Point.java" => r"
                package com.example;
                import lombok.Value;

                @Value
                public class Point {
                    int x;
                    int y;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(Point p) {
                        int x = p.get<|>X();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Point.java"
    }

    test required_args_constructor {
        files: {
            "com/example/Service.java" => r"
                package com.example;
                import lombok.RequiredArgsConstructor;
                import lombok.NonNull;

                @RequiredArgsConstructor
                public class Service {
                    @NonNull
                    private final String endpoint;
                    private String optional;
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Service s = new Servi<|>ce("http://api.example.com");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Service.java"
    }
}

lsp_test_suite! {
    name: "Lombok Method Chaining",

    test builder_chain {
        files: {
            "com/example/Config.java" => r"
                package com.example;
                import lombok.Builder;

                @Builder
                public class Config {
                    private String host;
                    private int port;
                    private boolean ssl;
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Config config = Config.builder()
                            .ho<|>st("localhost")
                            .port(8080)
                            .ssl(true)
                            .build();
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Config.java"
    }

    test data_with_chain {
        files: {
            "com/example/User.java" => r"
                package com.example;
                import lombok.Data;
                import lombok.experimental.Accessors;

                @Data
                @Accessors(chain = true)
                public class User {
                    private String name;
                    private String email;
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User()
                            .setName("John")
                            .setEm<|>ail("john@example.com");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

lsp_test_suite! {
    name: "Mixed Lombok and Manual Methods",

    test getter_with_manual_setter {
        files: {
            "com/example/Account.java" => r"
                package com.example;
                import lombok.Getter;

                public class Account {
                    @Getter
                    private String username;

                    public void setUsername(String username) {
                        this.username = username.toLowerCase();
                    }
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(Account account) {
                        String name = account.getUser<|>name();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Account.java"
    }

    test manual_setter_with_getter {
        files: {
            "com/example/Account.java" => r"
                package com.example;
                import lombok.Getter;

                public class Account {
                    @Getter
                    private String email;

                    public void setEmail(String email) {
                        this.email = email;
                    }
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(Account account) {
                        account.setEm<|>ail("test@example.com");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Account.java"
    }
}
