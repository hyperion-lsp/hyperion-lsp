// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for wildcard generic types (?, extends, super)
// Priority 1: High priority - very common in frameworks and libraries

lsp_test_suite! {
    name: "Wildcard Generic Navigation",

    test wildcard_extends_type_navigation {
        files: {
            "com/example/Container.java" => r"
                package com.example;
                import java.util.*;

                public class Container {
                    public void add(List<? extends Numbe<|>r> numbers) {
                    }
                }
            ",
            "java/lang/Number.java" => r"
                package java.lang;
                public abstract class Number {}
            "
        },
        nav: "com/example/Container.java", "<|>" => "Number.java"
    }

    test wildcard_extends_method_call {
        files: {
            "com/example/Processor.java" => r"
                package com.example;
                import java.util.*;

                public class Processor {
                    public void process(List<? extends Runnable> tasks) {
                        for (Runnable task : tasks) {
                            task.r<|>un();
                        }
                    }
                }
            ",
            "java/lang/Runnable.java" => r"
                package java.lang;
                public interface Runnable {
                    void run();
                }
            "
        },
        nav: "com/example/Processor.java", "<|>" => "Runnable.java"
    }

    test wildcard_super_navigation {
        files: {
            "com/example/Container.java" => r"
                package com.example;
                import java.util.*;

                public class Container {
                    public void addAll(List<? super Intege<|>r> list) {
                        list.add(42);
                    }
                }
            ",
            "java/lang/Integer.java" => r"
                package java.lang;
                public final class Integer extends Number {
                    public Integer(int value) {}
                }
            ",
            "java/lang/Number.java" => r"
                package java.lang;
                public abstract class Number {}
            "
        },
        nav: "com/example/Container.java", "<|>" => "Integer.java"
    }

    test nested_wildcard_navigation {
        files: {
            "com/example/Registry.java" => r"
                package com.example;

                public class Registry<T> {
                    public static class Key<K extends Registry<?>> {
                        public K getRegistr<|>y() { return null; }
                    }
                }
            "
        },
        nav: "com/example/Registry.java", "<|>" => "Registry.java"
    }

    test unbounded_wildcard {
        files: {
            "com/example/Util.java" => r"
                package com.example;
                import java.util.*;

                public class Util {
                    public static int size(List<?> list) {
                        return list.siz<|>e();
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> {
                    int size();
                }
            "
        },
        nav: "com/example/Util.java", "<|>" => "List.java"
    }

    test wildcard_in_return_type {
        files: {
            "com/example/Factory.java" => r#"
                package com.example;
                import java.util.*;

                public class Factory {
                    public List<? extends CharSequenc<|>e> getStrings() {
                        return Arrays.asList("a", "b", "c");
                    }
                }
            "#,
            "java/lang/CharSequence.java" => r"
                package java.lang;
                public interface CharSequence {
                    int length();
                }
            "
        },
        nav: "com/example/Factory.java", "<|>" => "CharSequence.java"
    }

    test wildcard_method_chaining {
        files: {
            "com/example/Service.java" => r"
                package com.example;
                import java.util.*;

                public class Service {
                    public void process(Collection<? extends User> users) {
                        users.stream()
                            .map(u -> u.getN<|>ame())
                            .collect(java.util.stream.Collectors.toList());
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Service.java", "<|>" => "User.java"
    }

    test multiple_wildcards_in_signature {
        files: {
            "com/example/Converter.java" => r"
                package com.example;
                import java.util.*;

                public class Converter {
                    public <T> void copy(List<? extends T> src, List<? super T> dest) {
                        for (T item : src) {
                            dest.ad<|>d(item);
                        }
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> extends Iterable<E> {
                    boolean add(E e);
                }
            "
        },
        nav: "com/example/Converter.java", "<|>" => "List.java"
    }

    test wildcard_with_multiple_bounds {
        files: {
            "com/example/Handler.java" => r"
                package com.example;
                import java.util.*;

                public class Handler {
                    public void handle(List<? extends Number & Comparab<|>le<?>> numbers) {
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
        nav: "com/example/Handler.java", "<|>" => "Comparable.java"
    }

    test wildcard_in_class_declaration {
        files: {
            "com/example/Box.java" => r"
                package com.example;
                import java.util.*;

                public class Box<T extends Comparable<? super T>> {
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

    test wildcard_array_type {
        files: {
            "com/example/ArrayUtil.java" => r"
                package com.example;

                public class ArrayUtil {
                    public static <T> T[] toArray(java.util.List<? extends T> list) {
                        return (T[]) list.toArra<|>y();
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> {
                    Object[] toArray();
                }
            "
        },
        nav: "com/example/ArrayUtil.java", "<|>" => "List.java"
    }

    test wildcard_in_constructor {
        files: {
            "com/example/Wrapper.java" => r"
                package com.example;
                import java.util.*;

                public class Wrapper<T> {
                    private List<T> items;

                    public Wrapper(Collection<? extends T> source) {
                        this.items = new ArrayList<>(source);
                    }

                    public List<T> getItems() {
                        return items;
                    }
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.*;

                public class Main {
                    public void test() {
                        List<String> strings = Arrays.asList("a", "b");
                        Wrapper<String> wrapper = new Wrapper<>(strings);
                        wrapper.getItem<|>s();
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Wrapper.java"
    }

    test recursive_wildcard {
        files: {
            "com/example/TreeNode.java" => r"
                package com.example;
                import java.util.*;

                public class TreeNode<T extends TreeNode<T>> {
                    private List<T> children;

                    public void addChild(T child) {
                        children.ad<|>d(child);
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> {
                    boolean add(E e);
                }
            "
        },
        nav: "com/example/TreeNode.java", "<|>" => "List.java"
    }

    test wildcard_capture {
        files: {
            "com/example/CaptureHelper.java" => r"
                package com.example;
                import java.util.*;

                public class CaptureHelper {
                    public void reverse(List<?> list) {
                        reverseHelper(list);
                    }

                    private <T> void reverseHelper(List<T> list) {
                        Collections.revers<|>e(list);
                    }
                }
            ",
            "java/util/Collections.java" => r"
                package java.util;
                public class Collections {
                    public static <T> void reverse(List<T> list) {}
                }
            "
        },
        nav: "com/example/CaptureHelper.java", "<|>" => "Collections.java"
    }
}
