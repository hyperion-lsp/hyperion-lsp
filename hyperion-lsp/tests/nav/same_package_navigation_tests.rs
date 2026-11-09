// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Same-package class navigation
//
// Issue: When classes are in the same package and no explicit import exists,
// goto-definition should still work because Java allows unqualified references
// to classes in the same package.
//
// Current behavior: Navigation fails without import statement
// Expected behavior: Navigation succeeds for same-package classes

lsp_test_suite! {
    name: "Same Package Navigation - Basic",

    test same_package_variable_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Us<|>er user = new User();
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
    test same_package_constructor {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new Us<|>er();
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
    test same_package_method_parameter {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    public void process(Us<|>er user) {
                        // process user
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "User.java"
    }
    test same_package_method_return_type {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    public Us<|>er getUser() {
                        return new User();
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "User.java"
    }
    test same_package_field_type {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    private Us<|>er user;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "User.java"
    }
}

lsp_test_suite! {
    name: "Same Package Navigation - Advanced",
    test same_package_static_method_call {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Us<|>erFactory.createUser();
                    }
                }
            ",
            "com/example/UserFactory.java" => r"
                package com.example;

                public class UserFactory {
                    public static User createUser() {
                        return new User();
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "UserFactory.java"
    }
    test same_package_generic_type_parameter {
        files: {
            "com/example/Repository.java" => r"
                package com.example;

                import java.util.List;

                public class Repository {
                    private List<User> users;
                    private Us<|>er user;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Repository.java", "<|>" => "User.java"
    }
    test same_package_array_type {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    public Us<|>er[] getAllUsers() {
                        return new User[10];
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "User.java"
    }
    test same_package_extends {
        files: {
            "com/example/AdminUser.java" => r"
                package com.example;

                public class AdminUser extends Us<|>er {
                    private boolean isAdmin;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/AdminUser.java", "<|>" => "User.java"
    }
    test same_package_implements {
        files: {
            "com/example/UserImpl.java" => r"
                package com.example;

                public class UserImpl implements UserInterfa<|>ce {
                    private String name;
                }
            ",
            "com/example/UserInterface.java" => r"
                package com.example;

                public interface UserInterface {
                    String getName();
                }
            "
        },
        nav: "com/example/UserImpl.java", "<|>" => "UserInterface.java"
    }
}

lsp_test_suite! {
    name: "Same Package Navigation - Edge Cases",
    test same_package_nested_references {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        UserServi<|>ce service = new UserService();
                        User user = service.getUser();
                    }
                }
            ",
            "com/example/UserService.java" => r"
                package com.example;

                public class UserService {
                    public User getUser() {
                        return new User();
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "UserService.java"
    }
    test multiple_same_package_references {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    private Us<|>er user;
                    private UserService service;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            ",
            "com/example/UserService.java" => r"
                package com.example;

                public class UserService {
                    public User getUser() {
                        return new User();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test same_package_precedence_over_java_lang {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    private MyStrin<|>g value;
                }
            ",
            "com/example/MyString.java" => r"
                package com.example;

                public class MyString {
                    private String internal;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "MyString.java"
    }
}

// Test that ensures explicit imports still work (regression test)
lsp_test_suite! {
    name: "Same Package Navigation - Regression Tests",
    test explicit_import_still_works {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                import com.example.User;

                public class Main {
                    public void test() {
                        Us<|>er user = new User();
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

lsp_goto_none_test! {
    name: different_package_requires_import,
    files: {
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                public void test() {
                    Us<|>er user = new User();
                }
            }
        ",
        "com/other/User.java" => r"
            package com.other;

            public class User {
                private String name;
            }
        "
    },
    nav: "com/example/Main.java", "<|>"
}
