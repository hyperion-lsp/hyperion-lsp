package v;

import java.util.*;
import java.util.function.*;

public class Generics {
    static class Box<T> {
        T value;
        Box(T v) { value = v; }
        T get() { return value; }
        <R> Box<R> map(Function<? super T, ? extends R> f) { return new Box<>(f.apply(value)); }
        static <A> Box<A> of(A a) { return new Box<>(a); }
    }
    static class Node<T extends Comparable<T>> {
        T val; Node<T> next;
        int cmp(Node<T> o) { return val.compareTo(o.val); }
    }
    interface Visitor<R> { R visit(String s); }
    static <K, V extends Comparable<V>> K argMax(Map<K, V> m) {
        K best = null; V bv = null;
        for (Map.Entry<K, V> e : m.entrySet()) {
            if (bv == null || e.getValue().compareTo(bv) > 0) { best = e.getKey(); bv = e.getValue(); }
        }
        return best;
    }
    static <T> List<T> listOf(T... xs) { return Arrays.asList(xs); }

    void test() {
        Box<String> b = new Box<>("s");
        Box<Integer> len = b.map(String::length);
        int n = len.get();
        Integer boxed = len.get();
        long asLong = len.get();
        double d = len.get();
        Box<List<String>> bl = Box.of(new ArrayList<>());
        bl.get().add("x");
        String s = b.get().trim();
        Node<Integer> node = new Node<>();
        node.cmp(node);
        Visitor<Integer> v = str -> str.length();
        v.visit("x");
        Map<String, Integer> counts = new HashMap<>();
        String key = argMax(counts);
        List<String> l = listOf("a", "b");
        List<Integer> l2 = listOf(1, 2);
        Double pct = counts.size() > 0 ? counts.values().stream().mapToInt(Integer::intValue).sum() * 100.0 : 0;
        Long big = 1L;
        long prim = big;
        Integer i2 = 5;
        long widened = i2;
        double dd = i2;
        char ch = 'a';
        int fromChar = ch;
        byte by = 10;
        short sh = 20;
        float fl = 1;
        Object o = 1;
        Number num = 1;
        Comparable<String> cs = "x";
        CharSequence seq = "x";
        Object[] oa = new String[0];
        int[][] mat = new int[2][];
        var inferred = new ArrayList<Map<String, Integer>>();
        inferred.add(new HashMap<>());
        Map.Entry<String, Integer> entry = Map.entry("a", 1);
        for (var e : counts.entrySet()) { e.getKey().length(); e.getValue().intValue(); }
        for (Map.Entry<String, Integer> e : counts.entrySet()) { e.getKey(); }
        for (Object[] row : new ArrayList<Object[]>()) { String x = (String) row[0]; }
        for (int[] row : mat) { row[0]++; }
        Supplier<List<String>> sup = ArrayList::new;
        BiFunction<Integer, Integer, Integer> add = Integer::sum;
        add.apply(1, 2);
        UnaryOperator<String> up = String::toUpperCase;
        up.apply("x");
    }
}
