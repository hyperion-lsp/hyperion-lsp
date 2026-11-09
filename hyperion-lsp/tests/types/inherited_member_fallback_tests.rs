// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests: `resolve_inherited_member` resolved
//! "current class" as the file's first top-level CLASS_DECL regardless of
//! cursor position, and probed only `parent_class` — never the class's
//! interfaces. It now resolves the enclosing type at the cursor and seeds
//! `find_member_with_inheritance` with the class itself, covering both the
//! parent chain and interface default methods.

lsp_test_suite! {
    name: "Inherited member fallback navigation",

    test interface_default_method_reachable {
        files: {
            "com/example/Greeter.java" => r#"
                package com.example;

                public interface Greeter {
                    default String greeting() {
                        return "hello";
                    }
                }
            "#,
            "com/example/ConsoleGreeter.java" => r"
                package com.example;

                public class ConsoleGreeter implements Greeter {
                    public void run() {
                        System.out.println(greet<|>ing());
                    }
                }
            "
        },
        nav: "com/example/ConsoleGreeter.java", "<|>" => "Greeter.java"
    }

    test second_class_in_file_uses_its_own_parent {
        files: {
            "com/example/Base.java" => r"
                package com.example;

                public class Base {
                    protected int baseValue() {
                        return 42;
                    }
                }
            ",
            "com/example/Holder.java" => r"
                package com.example;

                public class Holder {
                }

                class Derived extends Base {
                    int use() {
                        return baseVal<|>ue();
                    }
                }
            "
        },
        nav: "com/example/Holder.java", "<|>" => "Base.java"
    }

    test inherited_member_in_enum {
        files: {
            "com/example/Describable.java" => r#"
                package com.example;

                public interface Describable {
                    default String describe() {
                        return "thing";
                    }
                }
            "#,
            "com/example/Status.java" => r"
                package com.example;

                public enum Status implements Describable {
                    OPEN, CLOSED;

                    public String label() {
                        return descr<|>ibe();
                    }
                }
            "
        },
        nav: "com/example/Status.java", "<|>" => "Describable.java"
    }

    test grandparent_method_reachable {
        files: {
            "com/example/Root.java" => r"
                package com.example;

                public class Root {
                    protected int rootValue() {
                        return 1;
                    }
                }
            ",
            "com/example/Middle.java" => r"
                package com.example;

                public class Middle extends Root {
                }
            ",
            "com/example/Leaf.java" => r"
                package com.example;

                public class Leaf extends Middle {
                    int use() {
                        return rootVa<|>lue();
                    }
                }
            "
        },
        nav: "com/example/Leaf.java", "<|>" => "Root.java"
    }

    test superinterface_default_method_reachable {
        files: {
            "com/example/BaseGreeter.java" => r#"
                package com.example;

                public interface BaseGreeter {
                    default String salute() {
                        return "hi";
                    }
                }
            "#,
            "com/example/DerivedGreeter.java" => r"
                package com.example;

                public interface DerivedGreeter extends BaseGreeter {
                }
            ",
            "com/example/Polite.java" => r"
                package com.example;

                public class Polite implements DerivedGreeter {
                    public String run() {
                        return sal<|>ute();
                    }
                }
            "
        },
        nav: "com/example/Polite.java", "<|>" => "BaseGreeter.java"
    }

    test inherited_member_in_record {
        files: {
            "com/example/Sizable.java" => r"
                package com.example;

                public interface Sizable {
                    default int defaultSize() {
                        return 0;
                    }
                }
            ",
            "com/example/Pair.java" => r"
                package com.example;

                public record Pair(int a, int b) implements Sizable {
                    public int total() {
                        return defaultSi<|>ze();
                    }
                }
            "
        },
        nav: "com/example/Pair.java", "<|>" => "Sizable.java"
    }
}
