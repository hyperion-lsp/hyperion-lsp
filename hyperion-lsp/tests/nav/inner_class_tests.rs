// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive inner and nested class tests
// Testing: Outer.Inner, static nested classes, anonymous classes, local classes
lsp_test_suite! {
    name: "Inner and Nested Classes",

    // Basic inner class reference
    test inner_class_reference {
        files: {
            "com/example/Outer.java" => r"
                package com.example;
                public class Outer {
                    public void test() {
                        Inn<|>er inner = new Inner();
                    }
                    public class Inner {
                        public void process() {}
                    }
                }
            "
        },
        nav: "com/example/Outer.java", "<|>" => "Outer.java"
    }

    // Static nested class
    test static_nested_class {
        files: {
            "com/example/Outer.java" => r"
                package com.example;
                public class Outer {
                    public void test() {
                        Buil<|>der builder = new Builder();
                    }
                    public static class Builder {
                        public Outer build() { return null; }
                    }
                }
            "
        },
        nav: "com/example/Outer.java", "<|>" => "Outer.java"
    }

    // Qualified inner class
    test qualified_inner_class {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Outer.Inn<|>er inner = new Outer().new Inner();
                    }
                }
            ",
            "com/example/Outer.java" => r"
                package com.example;
                public class Outer {
                    public class Inner {
                        public void process() {}
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Outer.java"
    }

    // Qualified static nested class
    test qualified_static_nested {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Outer.Buil<|>der builder = new Outer.Builder();
                    }
                }
            ",
            "com/example/Outer.java" => r"
                package com.example;
                public class Outer {
                    public static class Builder {
                        public Outer build() { return null; }
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Outer.java"
    }

    // Method in inner class
    test method_in_inner_class {
        files: {
            "com/example/Outer.java" => r"
                package com.example;
                public class Outer {
                    public void test() {
                        Inner inner = new Inner();
                        inner.proc<|>ess();
                    }
                    public class Inner {
                        public void process() {}
                    }
                }
            "
        },
        nav: "com/example/Outer.java", "<|>" => "Outer.java"
    }

    // Inner class constructor
    test inner_class_constructor {
        files: {
            "com/example/Outer.java" => r"
                package com.example;
                public class Outer {
                    public void test() {
                        Inner inner = new Inn<|>er();
                    }
                    public class Inner {
                        public Inner() {}
                    }
                }
            "
        },
        nav: "com/example/Outer.java", "<|>" => "Outer.java"
    }

    // Qualified inner class constructor
    test qualified_inner_constructor {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Outer.Inner obj = new Outer().new Inn<|>er();
                    }
                }
            ",
            "com/example/Outer.java" => r"
                package com.example;
                public class Outer {
                    public class Inner {
                        public void method() {}
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Outer.java"
    }

    test qualified_inner_constructor_keeps_the_explicit_outer_identity {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        First.Inner obj = new First().new Inn<|>er();
                    }
                }
            ",
            "com/example/First.java" => r"
                package com.example;
                public class First {
                    public class Inner {}
                }
            ",
            "com/example/Second.java" => r"
                package com.example;
                public class Second {
                    public class Inner {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "First.java"
    }

    // Multi-level nesting
    test multi_level_nesting {
        files: {
            "com/example/Outer.java" => r"
                package com.example;
                public class Outer {
                    public class Middle {
                        public void test() {
                            Inn<|>er inner = new Inner();
                        }
                        public class Inner {
                            public void process() {}
                        }
                    }
                }
            "
        },
        nav: "com/example/Outer.java", "<|>" => "Outer.java"
    }

    // Inner class accessing outer field
    test inner_accessing_outer_field {
        files: {
            "com/example/Outer.java" => r"
                package com.example;
                public class Outer {
                    private String name;
                    public class Inner {
                        public void test() {
                            System.out.println(na<|>me);
                        }
                    }
                }
            "
        },
        nav: "com/example/Outer.java", "<|>" => "Outer.java"
    }

    // Anonymous class
    test anonymous_class_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Runnable r = new Runnable() {
                            public void run() {
                                help<|>er();
                            }
                            private void helper() {}
                        };
                    }
                }
            ",
            "com/example/Runnable.java" => r"
                package com.example;
                public interface Runnable {
                    void run();
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }
}
