// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

lsp_test_suite! {
    name: "Record Navigation and Synthetic Methods",

    // Test 1: Navigate to record class
    test record_type_reference {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Us<|>er user = new User("John", "john@example.com");
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public record User(String name, String email) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 2: Record component accessor (NO "get" prefix)
    test record_accessor_name {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User("John", "john@example.com");
                        String name = user.na<|>me();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public record User(String name, String email) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 3: Record component accessor - second field
    test record_accessor_email {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User("John", "john@example.com");
                        String email = user.em<|>ail();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public record User(String name, String email) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 4: Record constructor navigation
    test record_constructor {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new Us<|>er("John", "john@example.com");
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public record User(String name, String email) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 5: Record method chaining
    test record_method_chain {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        String upperName = user.na<|>me().toUpperCase();
                    }

                    private User user;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public record User(String name, String email) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 6: Record with custom methods
    test record_custom_method {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User("John", "john@example.com");
                        String display = user.getDisplayN<|>ame();
                    }
                }
            "#,
            "com/example/User.java" => r#"
                package com.example;

                public record User(String name, String email) {
                    public String getDisplayName() {
                        return name + " <" + email + ">";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 7: Record with validation (compact constructor)
    test record_compact_constructor {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Point point = new Po<|>int(10, 20);
                    }
                }
            ",
            "com/example/Point.java" => r#"
                package com.example;

                public record Point(int x, int y) {
                    public Point {
                        if (x < 0 || y < 0) {
                            throw new IllegalArgumentException("Coordinates must be positive");
                        }
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Point.java"
    }

    // Test 8: Record with multiple types
    test record_mixed_types {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Person person = new Person("John", 30, true);
                        int age = person.a<|>ge();
                    }
                }
            "#,
            "com/example/Person.java" => r"
                package com.example;

                public record Person(String name, int age, boolean active) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "Person.java"
    }

    // Test 9: Record in return position
    test record_return_type {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public User getUser() {
                        return new User("John", "john@example.com");
                    }

                    public void test() {
                        String name = getUser().na<|>me();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public record User(String name, String email) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 10: Nested record access
    test record_nested_access {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Profile profile = new Profile(new User("John", "john@example.com"), "Bio");
                        String email = profile.user().em<|>ail();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public record User(String name, String email) {}
            ",
            "com/example/Profile.java" => r"
                package com.example;

                public record Profile(User user, String bio) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 11: Record toString() method (auto-generated)
    test record_tostring {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User("John", "john@example.com");
                        String str = user.toStr<|>ing();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public record User(String name, String email) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 12: Record equals() method (auto-generated)
    test record_equals {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user1 = new User("John", "john@example.com");
                        User user2 = new User("John", "john@example.com");
                        boolean same = user1.equ<|>als(user2);
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public record User(String name, String email) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 13: Record hashCode() method (auto-generated)
    test record_hashcode {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User("John", "john@example.com");
                        int hash = user.hashC<|>ode();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public record User(String name, String email) {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}
