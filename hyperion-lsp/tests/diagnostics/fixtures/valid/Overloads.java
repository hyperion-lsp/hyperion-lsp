package v;

import java.util.*;
import java.util.function.*;
import java.util.stream.*;

public class Overloads {
    static void num(long x) {}
    static void num(Integer x) {}
    static void box(Object o) {}
    static void box(Long l) {}
    static void primitive(double d) {}
    static void str(CharSequence s) {}
    static void arr(int[] a) {}
    static void arr(Object o) {}
    static void objArr(Object[] a) {}
    static void vararg(String fmt, Object... args) {}
    static void vararg2(int... xs) {}
    static <T extends Comparable<T>> T max(List<? extends T> l) { return null; }
    static <T> void take(Class<T> c, T value) {}
    static <T> T identity(T t) { return t; }
    static void fn(Function<String, Integer> f) {}
    static void run(Runnable r) {}
    static void call(java.util.concurrent.Callable<String> c) {}
    static void list(List<String> l) {}
    static void coll(Collection<? extends Number> c) {}
    static void iter(Iterable<String> i) {}
    static void map(Map<String, ? super Integer> m) {}

    void test(int i, Integer boxed, short s, char c, byte b, float f, long l) {
        num(i);            // int -> long widening
        num(boxed);
        num(s);
        num(c);
        box(i);            // boxing to Object
        box(5L);           // Long
        primitive(i);
        primitive(boxed);  // unbox + widen
        primitive(l);
        primitive(f);
        str("x");
        str(new StringBuilder());
        arr(new int[0]);
        arr("x");
        objArr(new String[0]);
        objArr(new Integer[]{1, 2});
        vararg("f");
        vararg("f", 1, "two", 3.0);
        vararg("f", (Object[]) new String[]{"a"});
        vararg("f", new Object[]{"a"});
        vararg2();
        vararg2(1, 2, 3);
        vararg2(new int[]{1});
        Object m = max(Arrays.asList(1, 2, 3));
        take(String.class, "s");
        take(Integer.class, 1);
        String x = identity("s");
        Integer y = identity(1);
        fn(String::length);
        fn(str -> str.length());
        run(() -> {});
        run(this::helper);
        call(() -> "x");
        list(new ArrayList<>());
        list(Arrays.asList("a", "b"));
        list(List.of());
        coll(List.of(1, 2));
        coll(new HashSet<Integer>());
        iter(new ArrayList<String>());
        map(new HashMap<String, Number>());
        map(new HashMap<>());
        List<Object> objs = new ArrayList<>();
        objs.add("string");
        objs.add(1);
        objs.add(null);
        Map<String, List<Integer>> grouped = new HashMap<>();
        grouped.computeIfAbsent("k", k -> new ArrayList<>()).add(1);
        String joined = Stream.of("a", "b").collect(Collectors.joining(","));
        Set<String> set = Stream.of("a").collect(Collectors.toSet());
        Map<Boolean, List<String>> parts = Stream.of("a").collect(Collectors.partitioningBy(z -> z.isEmpty()));
        StringBuilder sb = new StringBuilder();
        sb.append("a").append(1).append('c').append(2.0).append(true).append((Object) null);
        sb.insert(0, "x");
        String.valueOf(i);
        String.valueOf(new char[]{'a'});
        Math.max(i, l);
        Math.max(1, 2.0);
        Math.abs(-1L);
        Arrays.asList(1, 2, 3);
        Arrays.asList("a");
        Arrays.sort(new int[0]);
        Arrays.sort(new String[0], Comparator.naturalOrder());
        Collections.sort(new ArrayList<String>());
        Optional.of("a").map(String::length).orElse(0);
        Optional.ofNullable((String) null).orElseGet(() -> "d");
        Integer.parseInt("1");
        Integer.valueOf(1);
        Long.valueOf(1L);
        Objects.equals(i, boxed);
        Objects.hash(i, "a", null);
        System.out.printf("%d %s%n", i, "a");
        System.out.println();
        System.out.println(i);
        System.out.println((Object) null);
        System.out.println(new char[]{'a'});
        Thread t = new Thread(() -> {});
        new Thread(this::helper, "name").start();
    }

    void helper() {}
}
