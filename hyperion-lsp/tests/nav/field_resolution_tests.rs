// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Field Resolution Tests
//!
//! Tests goto-definition for field access in various scenarios:
//! - Basic field access (obj.field)
//! - Inherited fields
//! - Field chains (field.field.field)
//! - Static fields
//! - Field shadowing
//! - Mixed method/field chains

lsp_test_suite! {
    name: "Basic Field Access",

    test simple_field_access {
        files: {
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    String name;
                    int age;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User();
                        user.na<|>me;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test field_access_with_type_usage {
        files: {
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    String name;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User();
                        String value = user.na<|>me;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test public_field_access {
        files: {
            "com/example/Config.java" => r"
                package com.example;

                public class Config {
                    public String appName;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Config config = new Config();
                        config.app<|>Name;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Config.java"
    }
}

lsp_test_suite! {
    name: "Inherited Field Access",

    test field_from_parent_class {
        files: {
            "com/example/Person.java" => r"
                package com.example;

                public class Person {
                    protected String name;
                    protected int age;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User extends Person {
                    private int userId;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User();
                        user.na<|>me;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Person.java"
    }

    test field_from_grandparent_class {
        files: {
            "com/example/Entity.java" => r"
                package com.example;

                public class Entity {
                    protected String id;
                }
            ",
            "com/example/Person.java" => r"
                package com.example;

                public class Person extends Entity {
                    protected String name;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User extends Person {
                    private int userId;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User();
                        user.i<|>d;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Entity.java"
    }
}

lsp_test_suite! {
    name: "Field Chain Access",

    test simple_field_chain {
        files: {
            "com/example/Profile.java" => r"
                package com.example;

                public class Profile {
                    public String email;
                    public String bio;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    public Profile profile;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User();
                        user.prof<|>ile.email;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test field_chain_second_field {
        files: {
            "com/example/Profile.java" => r"
                package com.example;

                public class Profile {
                    public String email;
                    public String bio;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    public Profile profile;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User();
                        user.profile.ema<|>il;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Profile.java"
    }

    test triple_field_chain {
        files: {
            "com/example/Address.java" => r"
                package com.example;

                public class Address {
                    public String city;
                }
            ",
            "com/example/Profile.java" => r"
                package com.example;

                public class Profile {
                    public Address address;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    public Profile profile;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User();
                        user.profile.address.ci<|>ty;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Address.java"
    }
}

lsp_test_suite! {
    name: "Static Field Access",

    test static_field_via_class {
        files: {
            "com/example/Config.java" => r#"
                package com.example;

                public class Config {
                    public static String VERSION = "1.0.0";
                    public static int MAX_SIZE = 100;
                }
            "#,
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        String version = Config.VERS<|>ION;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Config.java"
    }

    test static_field_via_instance {
        files: {
            "com/example/Config.java" => r#"
                package com.example;

                public class Config {
                    public static String VERSION = "1.0.0";
                }
            "#,
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Config config = new Config();
                        String version = config.VERS<|>ION;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Config.java"
    }
}

lsp_test_suite! {
    name: "Field Shadowing",

    test child_shadows_parent_field {
        files: {
            "com/example/Parent.java" => r"
                package com.example;

                public class Parent {
                    public int value = 1;
                }
            ",
            "com/example/Child.java" => r#"
                package com.example;

                public class Child extends Parent {
                    public String value = "child";
                }
            "#,
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Child child = new Child();
                        child.val<|>ue;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Child.java"
    }

    test parent_reference_to_parent_field {
        files: {
            "com/example/Parent.java" => r"
                package com.example;

                public class Parent {
                    public int value = 1;
                }
            ",
            "com/example/Child.java" => r#"
                package com.example;

                public class Child extends Parent {
                    public String value = "child";
                }
            "#,
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Parent parent = new Child();
                        parent.val<|>ue;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Parent.java"
    }

    test private_parent_field_does_not_hide_inherited_interface_constant {
        files: {
            "com/example/Named.java" => r"
                package com.example;

                public interface Named {
                    int VALUE = 1;
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;

                public class Parent {
                    private int VALUE = 2;
                }
            ",
            "com/example/Child.java" => r"
                package com.example;

                public class Child extends Parent implements Named {
                    int inherited() {
                        return this.VAL<|>UE;
                    }
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Named.java"
    }
}

lsp_test_suite! {
    name: "Mixed Method and Field Chains",

    test method_then_field {
        files: {
            "com/example/Profile.java" => r"
                package com.example;

                public class Profile {
                    public String email;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private Profile profile;

                    public Profile getProfile() {
                        return profile;
                    }
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User();
                        user.getProfile().ema<|>il;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Profile.java"
    }

    test field_then_method {
        files: {
            "com/example/Profile.java" => r#"
                package com.example;

                public class Profile {
                    public String getEmail() {
                        return "";
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    public Profile profile;
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User();
                        user.profile.getEm<|>ail();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Profile.java"
    }
}

lsp_test_suite! {
    name: "Special Field Access",

    test this_field_access {
        files: {
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;

                    public void setName(String name) {
                        this.na<|>me = name;
                    }
                }
            "
        },
        nav: "com/example/User.java", "<|>" => "User.java"
    }

    test super_field_access {
        files: {
            "com/example/Parent.java" => r"
                package com.example;

                public class Parent {
                    protected String name;
                }
            ",
            "com/example/Child.java" => r"
                package com.example;

                public class Child extends Parent {
                    private String name;

                    public void test() {
                        String value = super.na<|>me;
                    }
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Parent.java"
    }
}
