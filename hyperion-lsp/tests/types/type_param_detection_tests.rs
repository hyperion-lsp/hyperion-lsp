// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Type Parameter Detection
// Tests that generic type parameters are correctly detected and tracked in symbol tables
// Current behavior: Type parameters may not be properly indexed or may be confused with class types
// Expected: Should distinguish type parameters from class types and track their bounds

lsp_test_suite! {
    name: "Type Parameter Detection",

    test simple_type_parameter {
        files: {
            "com/example/Container.java" => r"
                package com.example;

                public class Container<T> {
                    private T value;

                    public T getValue() {
                        return val<|>ue;
                    }
                }
            "
        },
        nav: "com/example/Container.java", "<|>" => "Container.java"
    }

    test bounded_type_parameter {
        files: {
            "com/example/Container.java" => r"
                package com.example;

                public class Container<T extends Base<|>Item> {
                    private T value;
                }
            ",
            "com/example/BaseItem.java" => r"
                package com.example;

                public class BaseItem {
                    private String name;
                }
            "
        },
        nav: "com/example/Container.java", "<|>" => "BaseItem.java"
    }

    test bounded_type_parameter_with_custom_class {
        files: {
            "com/example/Container.java" => r"
                package com.example;

                public class Container<T extends Us<|>er> {
                    private T value;
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                }
            "
        },
        nav: "com/example/Container.java", "<|>" => "User.java"
    }

    test multiple_type_parameters {
        files: {
            "com/example/Pair.java" => r"
                package com.example;

                public class Pair<K, V> {
                    private K key;
                    private V value;

                    public K getKey() {
                        return ke<|>y;
                    }
                }
            "
        },
        nav: "com/example/Pair.java", "<|>" => "Pair.java"
    }

    test type_parameter_with_multiple_bounds {
        files: {
            "com/example/Container.java" => r"
                package com.example;

                public class Container<T extends BaseItem & Validata<|>ble> {
                    private T value;
                }
            ",
            "com/example/BaseItem.java" => r"
                package com.example;

                public class BaseItem {
                    private String name;
                }
            ",
            "com/example/Validatable.java" => r"
                package com.example;

                public interface Validatable {
                    boolean isValid();
                }
            "
        },
        nav: "com/example/Container.java", "<|>" => "Validatable.java"
    }

    test method_type_parameter {
        files: {
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public <T> T identity(T value) {
                        return val<|>ue;
                    }
                }
            "
        },
        nav: "com/example/Utils.java", "<|>" => "Utils.java"
    }

    test method_type_parameter_with_bound {
        files: {
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public <T extends Base<|>Item> T wrap(T item) {
                        return item;
                    }
                }
            ",
            "com/example/BaseItem.java" => r"
                package com.example;

                public class BaseItem {
                    private String name;
                }
            "
        },
        nav: "com/example/Utils.java", "<|>" => "BaseItem.java"
    }

    test nested_type_parameters {
        files: {
            "com/example/Container.java" => r"
                package com.example;

                public class Container<T> {
                    public <U extends T> U get(Class<U> clazz) {
                        return null;
                    }

                    public void test() {
                        get(Base<|>Item.class);
                    }
                }
            ",
            "com/example/BaseItem.java" => r"
                package com.example;

                public class BaseItem {
                    private String name;
                }
            "
        },
        nav: "com/example/Container.java", "<|>" => "BaseItem.java"
    }

    test type_parameter_in_extends {
        files: {
            "com/example/StringContainer.java" => r"
                package com.example;

                public class StringContainer extends Contai<|>ner<String> {
                }
            ",
            "com/example/Container.java" => r"
                package com.example;

                public class Container<T> {
                    private T value;
                }
            "
        },
        nav: "com/example/StringContainer.java", "<|>" => "Container.java"
    }

    test type_parameter_in_implements {
        files: {
            "com/example/UserComparator.java" => r"
                package com.example;
                import java.util.Comparator;

                public class UserComparator implements Comparator<Us<|>er> {
                    public int compare(User a, User b) {
                        return 0;
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
        nav: "com/example/UserComparator.java", "<|>" => "User.java"
    }

    test wildcard_type_parameter {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.List;

                public class Main {
                    public void process(List<? extends Us<|>er> users) {
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

    test recursive_type_parameter {
        files: {
            "com/example/Node.java" => r"
                package com.example;

                public class Node<T extends Node<T>> {
                    private T next;

                    public T getNext() {
                        return ne<|>xt;
                    }
                }
            "
        },
        nav: "com/example/Node.java", "<|>" => "Node.java"
    }
}

lsp_hover_test! {
    name: hover_on_type_parameter,

    files: {
        "com/example/Container.java" => r"
            package com.example;

            public class Container<T> {
                private T value;

                public T get<|>Value() {
                    return value;
                }
            }
        "
    },

    hover: "com/example/Container.java", "<|>",
    contains: ["getValue"]
}

lsp_hover_test! {
    name: hover_on_bounded_type_parameter,

    files: {
        "com/example/Container.java" => r"
            package com.example;

            public class Container<T extends Numbe<|>r> {
                private T value;
            }
        "
    },

    hover: "com/example/Container.java", "<|>",
    contains: ["Number"]
}

lsp_hover_test! {
    name: hover_on_type_parameter_field,

    files: {
        "com/example/Container.java" => r"
            package com.example;

            public class Container<T> {
                private T val<|>ue;

                public T getValue() {
                    return value;
                }
            }
        "
    },

    hover: "com/example/Container.java", "<|>",
    contains: ["value"]
}
