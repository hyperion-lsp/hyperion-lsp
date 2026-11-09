package v;

import java.io.*;
import java.util.*;

public class Hierarchy {
    interface Shape { double area(); default String label() { return "s"; } }
    static abstract class Base implements Shape, Comparable<Base> {
        public int compareTo(Base o) { return 0; }
        protected abstract void hook();
        static Base create() { return null; }
    }
    static class Circle extends Base {
        final double r;
        Circle(double r) { this.r = r; }
        Circle() { this(1.0); }
        public double area() { return r; }
        protected void hook() {}
    }
    enum Color { RED, GREEN; Color next() { return values()[(ordinal() + 1) % values().length]; } }
    record Point(int x, int y) {
        Point { if (x < 0) throw new IllegalArgumentException(); }
        static Point origin() { return new Point(0, 0); }
        double dist() { return Math.sqrt(x * x + y * y); }
    }
    sealed interface Expr permits Num, Add {}
    record Num(int v) implements Expr {}
    record Add(Expr l, Expr r) implements Expr {}

    static int eval(Expr e) {
        return switch (e) {
            case Num n -> n.v();
            case Add a -> eval(a.l()) + eval(a.r());
        };
    }

    static String name(Color c) {
        return switch (c) { case RED -> "r"; case GREEN -> "g"; };
    }

    class Inner { int v = 1; int outer() { return Hierarchy.this.field + v; } }
    int field;

    void test() throws IOException {
        Circle c = new Circle(2);
        c.area(); c.label(); c.hook(); c.compareTo(c); c.equals(c); c.hashCode(); c.toString(); c.getClass();
        Shape s = c;
        s.area(); s.label(); s.toString(); s.equals(null); s.hashCode();
        Point p = new Point(1, 2);
        p.x(); p.y(); p.dist(); p.equals(p); p.hashCode(); p.toString();
        Point.origin();
        Color col = Color.valueOf("RED");
        col.name(); col.ordinal(); col.next(); col.compareTo(Color.GREEN); Color.values();
        Inner in = new Inner();
        in.outer();
        Hierarchy h = new Hierarchy();
        Hierarchy.Inner in2 = h.new Inner();
        Runnable r = new Runnable() {
            int count;
            public void run() { count++; helper(); this.helper2(); }
            void helper2() {}
        };
        r.run();
        Comparator<String> cmp = new Comparator<>() {
            public int compare(String a, String b) { return a.compareTo(b); }
        };
        cmp.compare("a", "b"); cmp.reversed(); cmp.thenComparing(cmp);
        int[] arr = new int[3];
        arr.clone(); int n = arr.length;
        String[][] grid = new String[2][2];
        grid[0].clone();
        List<String> l = new ArrayList<>();
        l.add("x"); l.stream().map(String::trim); l.forEach(System.out::println); l.iterator().next();
        StringWriter sw = new StringWriter();
        try (PrintWriter pw = new PrintWriter(sw)) { pw.println("x"); pw.flush(); }
        try (BufferedReader br = new BufferedReader(new StringReader("x"))) { br.readLine(); }
        Object o = l;
        if (o instanceof List<?> lst && !lst.isEmpty()) { lst.get(0); lst.size(); }
        Throwable t = new IllegalStateException("x");
        t.getMessage(); t.printStackTrace(); t.getStackTrace(); t.getCause();
    }

    static int negated(Object o) {
        if (!(o instanceof String s)) { return 0; }
        return s.length();
    }
    static boolean negatedOr(Object o) { return !(o instanceof String s) || s.isEmpty(); }
    static int qualifiedThis() { return new Hierarchy().new Inner().outer(); }
    static String recordPattern(Object o) {
        if (o instanceof Point(int x, int y) && x > 0) { return "p" + x + y; }
        return switch (o) { case Point(var px, var py) -> "q" + px + py; default -> "d"; };
    }

    void helper() {}
}
