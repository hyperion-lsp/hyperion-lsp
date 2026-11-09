// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for `this.field` navigation
//!
//! Ensures that `this.field` navigates to the field declaration,
//! NOT to a constructor parameter with the same name.

// ============================================================================
// Primary Bug Fix: this.field should navigate to field, not constructor param
// ============================================================================

lsp_test_suite! {
    name: "this.field Navigation",

    // The primary bug case: this.field should go to field, not constructor parameter
    test this_field_not_constructor_param {
        files: {
            "com/example/Sorter.java" => r"
                package com.example;
                import java.util.Comparator;

                public class Sorter {
                    private Comparator<String> stringComparator;

                    public Sorter(Comparator<String> stringComparator) {
                        this.stringComp<|>arator = stringComparator;
                    }
                }
            "
        },
        nav: "com/example/Sorter.java", "<|>" => "Sorter.java"
    }

    // Test: this.method() navigates to method
    test this_method_navigates_to_method {
        files: {
            "com/example/Helper.java" => r#"
                package com.example;

                public class Helper {
                    public void doWork() {
                        this.help<|>er();
                    }

                    private void helper() {
                        System.out.println("helping");
                    }
                }
            "#
        },
        nav: "com/example/Helper.java", "<|>" => "Helper.java"
    }

    // Test: this.field on RHS of assignment goes to field
    test this_field_rhs_assignment {
        files: {
            "com/example/Container.java" => r"
                package com.example;

                public class Container {
                    private String value;

                    public String getValue() {
                        return this.val<|>ue;
                    }
                }
            "
        },
        nav: "com/example/Container.java", "<|>" => "Container.java"
    }
}

// ============================================================================
// Cross-file this.field navigation
// ============================================================================

lsp_test_suite! {
    name: "this.field Cross-File Navigation",

    // Test: this.field with imported type navigates to field
    test this_field_with_imported_type {
        files: {
            "com/example/Service.java" => r"
                package com.example;
                import com.example.model.User;

                public class Service {
                    private User currentUser;

                    public void setUser(User user) {
                        this.current<|>User = user;
                    }
                }
            ",
            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "Service.java"
    }

    // Test: this.field.method() - this.field part navigates to field
    test this_field_then_method_call {
        files: {
            "com/example/Wrapper.java" => r"
                package com.example;
                import com.example.model.User;

                public class Wrapper {
                    private User user;

                    public String getName() {
                        return this.us<|>er.getName();
                    }
                }
            ",
            "com/example/model/User.java" => r#"
                package com.example.model;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Wrapper.java", "<|>" => "Wrapper.java"
    }
}

// ============================================================================
// Bug reproduction: method name matches field name
// ============================================================================

lsp_test_suite! {
    name: "this.field with Same-Named Method",

    // Simple case: method name matches field name
    test this_field_when_method_has_same_name {
        files: {
            "com/example/Builder.java" => r"
                package com.example;

                public class Builder {
                    private String name;

                    public Builder name(String name) {
                        this.na<|>me = name;
                        return this;
                    }
                }
            "
        },
        nav: "com/example/Builder.java", "<|>" => "Builder.java"
    }

    // Verify RHS still goes to parameter (not field)
    test rhs_goes_to_parameter_not_field {
        files: {
            "com/example/Builder2.java" => r"
                package com.example;

                public class Builder2 {
                    private String value;

                    public Builder2 value(String value) {
                        this.value = val<|>ue;
                        return this;
                    }
                }
            "
        },
        nav: "com/example/Builder2.java", "<|>" => "Builder2.java"
    }
}

// ============================================================================
// Edge cases
// ============================================================================

lsp_test_suite! {
    name: "this.field Edge Cases",

    // Test: Multiple fields, this.field navigates to correct one
    test this_field_multiple_fields {
        files: {
            "com/example/MultiField.java" => r"
                package com.example;

                public class MultiField {
                    private String first;
                    private String second;
                    private String third;

                    public void setSecond(String second) {
                        this.sec<|>ond = second;
                    }
                }
            "
        },
        nav: "com/example/MultiField.java", "<|>" => "MultiField.java"
    }

    // Test: Nested class, this.field refers to inner class field
    test this_field_in_nested_class {
        files: {
            "com/example/Outer.java" => r"
                package com.example;

                public class Outer {
                    private String outerField;

                    public class Inner {
                        private String innerField;

                        public void test(String innerField) {
                            this.inner<|>Field = innerField;
                        }
                    }
                }
            "
        },
        nav: "com/example/Outer.java", "<|>" => "Outer.java"
    }
}
