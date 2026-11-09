// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Regression tests: two root causes, pinned separately:
//
// 1. `ResolutionContext::resolve_identifier_with_lookup` (hyperion-types)
//    fell back to "assume same package" for java.lang types outside the
//    hardcoded well-known list (Comparable, Iterable, ...), so a
//    `Comparable value` parameter typed as the nonexistent
//    `com.example.Comparable` and member lookup failed.
// 2. `find_type_parameter_bound` (qualified_access_resolver) searched only
//    the enclosing METHOD_DECL's type-parameter list, so class-level
//    parameters (`class Box<T extends Comparable<T>>`) never resolved to
//    their bound.

lsp_test_suite! {
    name: "Type parameter bound navigation",

    test type_param_plain_generic_bound {
        files: {
            "com/example/Box.java" => r"
                package com.example;
                import java.util.*;

                public class Box<T extends Comparable<T>> {
                    private T value;

                    public boolean isGreaterThan(Box<T> other) {
                        return value.compareTo<|>(other.value) > 0;
                    }
                }
            ",
            "java/lang/Comparable.java" => r"
                package java.lang;
                public interface Comparable<T> {
                    int compareTo(T o);
                }
            "
        },
        nav: "com/example/Box.java", "<|>" => "Comparable.java"
    }

    test type_param_raw_bound {
        files: {
            "com/example/Box.java" => r"
                package com.example;
                import java.util.*;

                public class Box<T extends Comparable> {
                    private T value;

                    public boolean isGreaterThan(Box<T> other) {
                        return value.compareTo<|>(other.value) > 0;
                    }
                }
            ",
            "java/lang/Comparable.java" => r"
                package java.lang;
                public interface Comparable<T> {
                    int compareTo(T o);
                }
            "
        },
        nav: "com/example/Box.java", "<|>" => "Comparable.java"
    }

    test type_param_wildcard_bound_no_field {
        files: {
            "com/example/Box.java" => r"
                package com.example;
                import java.util.*;

                public class Box<T extends Comparable<? super T>> {
                    public boolean isGreaterThan(T value, T other) {
                        return value.compareTo<|>(other) > 0;
                    }
                }
            ",
            "java/lang/Comparable.java" => r"
                package java.lang;
                public interface Comparable<T> {
                    int compareTo(T o);
                }
            "
        },
        nav: "com/example/Box.java", "<|>" => "Comparable.java"
    }

    test type_param_concrete_receiver_java_lang {
        files: {
            "com/example/Box.java" => r"
                package com.example;

                public class Box {
                    public int cmp(Comparable value, Object other) {
                        return value.compareTo<|>(other);
                    }
                }
            ",
            "java/lang/Comparable.java" => r"
                package java.lang;
                public interface Comparable<T> {
                    int compareTo(T o);
                }
            "
        },
        nav: "com/example/Box.java", "<|>" => "Comparable.java"
    }

    test type_param_no_arg_workspace_interface {
        files: {
            "com/example/Box.java" => r"
                package com.example;

                public class Box {
                    public int cmp(Sortable value) {
                        return value.rank<|>();
                    }
                }
            ",
            "com/example/Sortable.java" => r"
                package com.example;
                public interface Sortable {
                    int rank();
                }
            "
        },
        nav: "com/example/Box.java", "<|>" => "Sortable.java"
    }

    test type_param_one_arg_exact_type {
        files: {
            "com/example/Box.java" => r"
                package com.example;

                public class Box {
                    public int cmp(Sortable value, String s) {
                        return value.rank<|>(s);
                    }
                }
            ",
            "com/example/Sortable.java" => r"
                package com.example;
                public interface Sortable {
                    int rank(String s);
                }
            "
        },
        nav: "com/example/Box.java", "<|>" => "Sortable.java"
    }

    test type_param_object_param_string_arg {
        files: {
            "com/example/Box.java" => r"
                package com.example;

                public class Box {
                    public int cmp(Sortable value, String s) {
                        return value.rank<|>(s);
                    }
                }
            ",
            "com/example/Sortable.java" => r"
                package com.example;
                public interface Sortable {
                    int rank(Object o);
                }
            "
        },
        nav: "com/example/Box.java", "<|>" => "Sortable.java"
    }

    test type_param_generic_param_string_arg {
        files: {
            "com/example/Box.java" => r"
                package com.example;

                public class Box {
                    public int cmp(Sortable value, String s) {
                        return value.rank<|>(s);
                    }
                }
            ",
            "com/example/Sortable.java" => r"
                package com.example;
                public interface Sortable<T> {
                    int rank(T t);
                }
            "
        },
        nav: "com/example/Box.java", "<|>" => "Sortable.java"
    }

    test type_param_type_param_bound_workspace_interface {
        files: {
            "com/example/Box.java" => r"
                package com.example;

                public class Box<T extends Sortable> {
                    private T value;

                    public boolean isGreaterThan(Box<T> other) {
                        return value.rank<|>(other.value) > 0;
                    }
                }
            ",
            "com/example/Sortable.java" => r"
                package com.example;
                public interface Sortable {
                    int rank(Object o);
                }
            "
        },
        nav: "com/example/Box.java", "<|>" => "Sortable.java"
    }
}
