// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end integration tests for Type Inference System
//!
//! These tests validate that TypeInferenceEngine correctly integrates across
//! all LSP features: navigation, completion, and hover.

lsp_test_suite! {
    name: "Type Inference - Method Chain Navigation",

    test method_chain_to_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;
                import com.example.model.Profile;

                public class Main {
                    public void test(User user) {
                        user.getProfile().get<|>Name();
                    }
                }
            ",
            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    public Profile getProfile() {
                        return new Profile();
                    }
                }
            ",
            "com/example/model/Profile.java" => r#"
                package com.example.model;

                public class Profile {
                    public String getName() {
                        return "Profile Name";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Profile.java"
    }

    test triple_method_chain {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test(User user) {
                        user.getProfile().getAddress().get<|>City();
                    }
                }
            ",
            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    public Profile getProfile() {
                        return new Profile();
                    }
                }
            ",
            "com/example/model/Profile.java" => r"
                package com.example.model;

                public class Profile {
                    public Address getAddress() {
                        return new Address();
                    }
                }
            ",
            "com/example/model/Address.java" => r#"
                package com.example.model;

                public class Address {
                    public String getCity() {
                        return "New York";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Address.java"
    }

    test field_then_method_chain {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test(User user) {
                        user.profile.get<|>Name();
                    }
                }
            ",
            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    public Profile profile;
                }
            ",
            "com/example/model/Profile.java" => r#"
                package com.example.model;

                public class Profile {
                    public String getName() {
                        return "Profile Name";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Profile.java"
    }
}

lsp_test_suite! {
    name: "Type Inference - Array Handling",

    test array_method_chain {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test(User[] users) {
                        User user = users[0];
                        user.get<|>Name();
                    }
                }
            ",
            "com/example/model/User.java" => r#"
                package com.example.model;

                public class User {
                    public String getName() {
                        return "User Name";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test method_returning_array {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;
                import com.example.model.UserRepository;

                public class Main {
                    public void test(UserRepository repo) {
                        User[] users = repo.getUsers();
                        User user = users[0];
                        user.get<|>Name();
                    }
                }
            ",
            "com/example/model/User.java" => r#"
                package com.example.model;

                public class User {
                    public String getName() {
                        return "User Name";
                    }
                }
            "#,
            "com/example/model/UserRepository.java" => r"
                package com.example.model;

                public class UserRepository {
                    public User[] getUsers() {
                        return new User[0];
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

lsp_test_suite! {
    name: "Type Inference - Primitive Type Handling",

    test primitive_return_types {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.Counter;

                public class Main {
                    public void test(Counter counter) {
                        int count = counter.get<|>Count();
                    }
                }
            ",
            "com/example/model/Counter.java" => r"
                package com.example.model;

                public class Counter {
                    public int getCount() {
                        return 42;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Counter.java"
    }
}

lsp_test_suite! {
    name: "Type Inference - Builder Pattern",

    test builder_method_chain {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test() {
                        User user = User.builder()
                            .withName("John")
                            .withAge(30)
                            .bu<|>ild();
                    }
                }
            "#,
            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    public static Builder builder() {
                        return new Builder();
                    }

                    public static class Builder {
                        public Builder withName(String name) {
                            return this;
                        }

                        public Builder withAge(int age) {
                            return this;
                        }

                        public User build() {
                            return new User();
                        }
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

// Hover tests for type inference
lsp_hover_test! {
    name: hover_on_method_chain_result,

    files: {
        "com/example/Main.java" => r"
            package com.example;
            import com.example.model.User;

            public class Main {
                public void test(User user) {
                    String name = user.getProfile().get<|>Name();
                }
            }
        ",
        "com/example/model/User.java" => r"
            package com.example.model;

            public class User {
                public Profile getProfile() {
                    return new Profile();
                }
            }
        ",
        "com/example/model/Profile.java" => r#"
            package com.example.model;

            public class Profile {
                public String getName() {
                    return "Name";
                }
            }
        "#
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["String", "getName"]
}

lsp_hover_test! {
    name: hover_on_array_element,

    files: {
        "com/example/Main.java" => r"
            package com.example;
            import com.example.model.User;

            public class Main {
                public void test(User[] users) {
                    User user = use<|>rs[0];
                }
            }
        ",
        "com/example/model/User.java" => r"
            package com.example.model;

            public class User {
                public String name;
            }
        "
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["User", "users"]
}
