// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for switch expressions (Java 14+)
// Priority 1: Medium-High priority - modern Java feature becoming standard

lsp_test_suite! {
    name: "Switch Expression Navigation",

    test nested_switch_in_arm_keeps_outer_var_navigation {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                class Main {
                    void run(int mode) {
                        var message = switch (mode) {
                            case 0 -> {
                                int retryCount = switch (mode) {
                                    default -> { yield 3; }
                                };
                                yield "ready";
                            }
                            default -> "waiting";
                        };
                        message.toUpperCa<|>se();
                    }
                }
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public final class String {
                    public String toUpperCase() { return this; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    test switch_yielded_local_keeps_outer_var_navigation {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                class Main {
                    void run(int mode) {
                        var message = switch (mode) {
                            case 0 -> {
                                var status = "ready";
                                yield status;
                            }
                            default -> "waiting";
                        };
                        message.toUpperCa<|>se();
                    }
                }
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public final class String {
                    public String toUpperCase() { return this; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    test switch_expression_method_calls {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;

                public class Main {
                    public Set<String> convert(Set<String> input) {
                        return switch (input.size()) {
                            case 0 -> Collections.emptyS<|>et();
                            case 1 -> Set.of(input.iterator().next());
                            default -> new HashSet<>(input);
                        };
                    }
                }
            ",
            "java/util/Collections.java" => r"
                package java.util;
                public class Collections {
                    public static <T> Set<T> emptySet() {
                        return null;
                    }
                }
            ",
            "java/util/Set.java" => r"
                package java.util;
                public interface Set<E> {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "Collections.java"
    }

    test switch_expression_multiple_cases {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public String getMessage(int code) {
                        return switch (code) {
                            case 0, 1, 2 -> "Low";
                            case 3, 4, 5 -> "Medium";
                            case 6, 7, 8, 9 -> "High";
                            default -> "Unknown";
                        };
                    }

                    public void test() {
                        String msg = getMessage(5);
                        msg.toUpperCa<|>se();
                    }
                }
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public final class String {
                    public String toUpperCase() { return this; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    test switch_expression_yield {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public User getUser(String type) {
                        return switch (type) {
                            case "admin" -> {
                                User user = new User();
                                user.setRole("ADMIN");
                                yield user;
                            }
                            case "guest" -> {
                                yield new User();
                            }
                            default -> new User();
                        };
                    }

                    public void test() {
                        User user = getUser("admin");
                        user.getRol<|>e();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String role;
                    public void setRole(String role) { this.role = role; }
                    public String getRole() { return role; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test switch_expression_enum {
        files: {
            "com/example/Day.java" => r"
                package com.example;
                public enum Day {
                    MONDAY, TUESDAY, WEDNESDAY, THURSDAY, FRIDAY, SATURDAY, SUNDAY
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public int getWorkHours(Day day) {
                        return switch (day) {
                            case MONDAY, TUESDAY, WEDNESDAY, THURSDAY, FRIDAY -> 8;
                            case SATURDAY, SUNDAY -> 0;
                        };
                    }

                    public void test() {
                        int hours = getWorkHours(Day.MONDAY);
                        Day.MONDAY.ordina<|>l();
                    }
                }
            ",
            "java/lang/Enum.java" => r"
                package java.lang;
                public abstract class Enum<E extends Enum<E>> {
                    public final int ordinal() { return 0; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Enum.java"
    }

    test switch_expression_with_objects {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public String describe(Object obj) {
                        return switch (obj) {
                            case Integer i -> "Integer: " + i;
                            case String s -> "String: " + s.lengt<|>h();
                            case null -> "null";
                            default -> "Unknown";
                        };
                    }
                }
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public final class String {
                    public int length() { return 0; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    test switch_expression_nested {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public String process(int x, int y) {
                        return switch (x) {
                            case 0 -> switch (y) {
                                case 0 -> "Origin";
                                default -> "Y-axis";
                            };
                            default -> switch (y) {
                                case 0 -> "X-axis";
                                default -> "Quadrant";
                            };
                        };
                    }

                    public void test() {
                        String result = process(0, 0);
                        result.toUpperCa<|>se();
                    }
                }
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public final class String {
                    public String toUpperCase() { return this; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    test switch_expression_variable_assignment {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(int code) {
                        var user = switch (code) {
                            case 1 -> new User("admin");
                            case 2 -> new User("guest");
                            default -> new User("anonymous");
                        };
                        user.getN<|>ame();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;
                    public User(String name) { this.name = name; }
                    public String getName() { return name; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test switch_expression_complex_type {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.*;

                public class Main {
                    public List<String> getValues(int type) {
                        return switch (type) {
                            case 1 -> List.o<|>f("a", "b");
                            case 2 -> Arrays.asList("c", "d");
                            default -> Collections.emptyList();
                        };
                    }
                }
            "#,
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> {
                    static <E> List<E> of(E... elements) { return null; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "List.java"
    }

    // The receiver is parenthesised because a switch expression is not a
    // Primary (JLS 15.8), so `switch (..) { .. }.toUpperCase()` is a syntax
    // error -- javac 26 rejects it with "';' expected" at the `.`. This test
    // asserted navigation through that illegal form and so could never have
    // passed. The contract it means to pin -- a method invoked on a switch
    // expression resolves against the switch expression's result type (JLS
    // 15.28), here `java.lang.String` -- is unchanged and now written in
    // compilable Java.
    test switch_expression_method_chaining {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(int code) {
                        String result = (switch (code) {
                            case 1 -> "hello";
                            case 2 -> "world";
                            default -> "unknown";
                        }).toUpperC<|>ase();
                    }
                }
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public final class String {
                    public String toUpperCase() { return this; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    test switch_expression_guarded_pattern {
        files: {
            "com/example/Shape.java" => r"
                package com.example;
                public interface Shape {
                    double area();
                }
            ",
            "com/example/Circle.java" => r"
                package com.example;
                public record Circle(double radius) implements Shape {
                    public double area() { return Math.PI * radius * radius; }
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public String describe(Shape shape) {
                        return switch (shape) {
                            case Circle c when c.radius() > 10 -> "Large circle";
                            case Circle c -> "Small circle: " + c.radiu<|>s();
                            default -> "Unknown shape";
                        };
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Circle.java"
    }
}
