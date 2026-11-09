// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Field Type Extraction
// Tests that field declarations correctly extract and resolve their types
// Current behavior: May fail to extract type for complex field declarations (arrays, generics, qualifiers)
// Expected: Should correctly identify field type for all declaration forms

lsp_test_suite! {
    name: "Field Type Extraction",

    test simple_field_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
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
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test array_field_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    private Us<|>er[] users;
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

    test multidimensional_array_field {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    private Us<|>er[][] userMatrix;
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

    test generic_field_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.List;

                public class Main {
                    private List<Us<|>er> users;
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

    test nested_generic_field {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.List;
                import java.util.Map;

                public class Main {
                    private Map<String, List<Us<|>er>> usersByGroup;
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

    test qualified_field_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    private com.example.model.Us<|>er user;
                }
            ",
            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test multiple_fields_same_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    private Us<|>er user1, user2, user3;
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

    test field_with_initializer {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    private Us<|>er user = new User();
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

    test annotated_field_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    @Deprecated
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
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test wildcard_generic_field {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.List;

                public class Main {
                    private List<? extends Us<|>er> users;
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

    test bounded_type_parameter_field {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.List;

                public class Main<T extends Us<|>er> {
                    private List<T> users;
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

lsp_hover_test! {
    name: hover_on_array_field,

    files: {
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                private User[] user<|>s;
            }
        ",
        "com/example/User.java" => r"
            package com.example;

            public class User {
                private String name;
            }
        "
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["User[]", "users"]
}

lsp_hover_test! {
    name: hover_on_generic_field,

    files: {
        "com/example/Main.java" => r"
            package com.example;
            import java.util.List;

            public class Main {
                private List<User> user<|>s;
            }
        ",
        "com/example/User.java" => r"
            package com.example;

            public class User {
                private String name;
            }
        "
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["List", "User"]
}
