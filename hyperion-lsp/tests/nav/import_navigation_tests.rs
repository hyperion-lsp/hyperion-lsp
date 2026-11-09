// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

lsp_test_suite! {
    name: "Import Navigation Tests",

    test simple_import_navigation {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.Us<|>er;

                public class Main {
                    User user;
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

    test nested_package_import {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.Us<|>er;

                public class Main {
                    User user;
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

    test multiple_class_import {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.User;
                import com.example.Produ<|>ct;
                import com.example.Order;

                public class Main {}
            ",
            "com/example/User.java" => r"
                package com.example;
                public class User {}
            ",
            "com/example/Product.java" => r"
                package com.example;
                public class Product {}
            ",
            "com/example/Order.java" => r"
                package com.example;
                public class Order {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "Product.java"
    }

    test static_import_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Utils.help<|>er;

                public class Main {
                    void test() {
                        helper();
                    }
                }
            ",
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public static void helper() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    test static_import_class {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Const<|>ants.*;

                public class Main {
                    void test() {
                        System.out.println(MAX_SIZE);
                    }
                }
            ",
            "com/example/Constants.java" => r"
                package com.example;

                public class Constants {
                    public static final int MAX_SIZE = 100;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Constants.java"
    }

    test static_imported_member_type_usage {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Outer.Nested;

                public class Main {
                    Ne<|>sted value;
                }
            ",
            "com/example/Outer.java" => r"
                package com.example;

                public class Outer {
                    public static class Nested {}
                    public static int Nested;
                    public static void Nested() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Outer.java"
    }

    test static_imported_value_beats_same_named_local_type {
        files: {
            "consumer/Main.java" => r"
                package consumer;
                import static p.Constants.Token;

                public class Main {
                    static class Token {}
                    int value = To<|>ken;
                }
            ",
            "p/Constants.java" => r"
                package p;

                public class Constants {
                    public static int Token;
                }
            "
        },
        nav: "consumer/Main.java", "<|>" => "Constants.java"
    }

    test static_imported_callable_beats_same_named_local_type {
        files: {
            "consumer/Main.java" => r"
                package consumer;
                import static p.Constants.Token;

                public class Main {
                    static class Token {}

                    void run() {
                        To<|>ken();
                    }
                }
            ",
            "p/Constants.java" => r"
                package p;

                public class Constants {
                    public static void Token() {}
                }
            "
        },
        nav: "consumer/Main.java", "<|>" => "Constants.java"
    }
}
