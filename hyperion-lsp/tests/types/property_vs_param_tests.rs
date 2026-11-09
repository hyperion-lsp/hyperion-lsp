// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for Property vs Constructor Parameter Navigation
//
// Problem: When a class has both a field and a constructor parameter with the same name,
// unqualified access (e.g., `this.name`) should navigate to the field, not the parameter.
//
// Example:
// ```java
// public class User {
//     private String name;           // Field
//     public User(String name) {     // Parameter - same name!
//         this.name = name;
//     }
// }
// ```
//
// Unqualified access like `this.name` should go to the field, not the parameter.
// This is the correct Java semantics.

lsp_test_suite! {
    name: "Property vs Constructor Parameter Navigation",

    // CORE BUG: Unqualified field access with this.
    // `this.name` should navigate to field, not parameter
    test this_qualified_access_to_field {
        files: {
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;

                    public User(String name) {
                        this.na<|>me = name;
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    // Verify we can navigate to the parameter when explicitly needed
    test parameter_access_in_assignment {
        files: {
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;

                    public User(String name) {
                        this.name = na<|>me;
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    // Field with getter/setter methods
    test this_access_to_getter {
        files: {
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;

                    public String getName() {
                        return na<|>me;
                    }

                    public User(String name) {
                        this.name = name;
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    // Method assignment uses parameter, not field
    test method_assignment_uses_parameter {
        files: {
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;

                    public void setName(String name) {
                        this.name = na<|>me;
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    // Accessing field through this in method
    test method_access_field_through_this {
        files: {
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;

                    public String getName() {
                        return this.na<|>me;
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    // Complex case: multiple fields with shadowing
    test multiple_fields_with_same_parameter_names {
        files: {
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;
                    private int age;

                    public User(String name, int age) {
                        this.na<|>me = name;
                        this.age = age;
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    // Cross-file: accessing property of other class
    test cross_file_property_access {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test(User user) {
                        String n = user.na<|>me;
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    public String name;

                    public User(String name) {
                        this.name = name;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Inheritance case: accessing inherited field
    test inherited_field_access {
        files: {
            "com/example/Person.java" => r"
                package com.example;
                public class Person {
                    protected String name;

                    public Person(String name) {
                        this.name = name;
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;
                public class User extends Person {
                    public User(String name) {
                        super(name);
                        String n = this.na<|>me;
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "Person.java"
    }

    // Field access in nested method
    test field_access_in_nested_code_block {
        files: {
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;

                    public User(String name) {
                        if (name != null) {
                            this.name = na<|>me;
                        }
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    // Field access in initializer
    test field_access_in_initializer_block {
        files: {
            "com/example/User.java" => r#"
                package com.example;
                public class User {
                    private String name = "default";

                    public void update(String name) {
                        this.na<|>me = name;
                    }
                }
            "#
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    // Avoid unqualified name confusion - should prefer field
    test unqualified_name_in_method_prefers_field {
        files: {
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;

                    public void setName(String name) {
                        // Unqualified 'name' in method should find parameter first
                        System.out.println(na<|>me);
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    // Field in conditional with parameter of same name
    test field_in_conditional_shadowed_by_parameter {
        files: {
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;

                    public boolean matches(String name) {
                        return this.na<|>me.equals(name);
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }
}
