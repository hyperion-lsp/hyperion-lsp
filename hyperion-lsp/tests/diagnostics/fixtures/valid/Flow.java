package v;

import java.io.*;
import java.util.*;
import java.util.concurrent.*;

public class Flow {
    static final boolean DEBUG = true;

    static int a(int x) {
        if (x > 0) { return 1; } else { return 2; }
    }
    static int b(int x) {
        while (true) {
            if (x > 3) return x;
            x++;
        }
    }
    static int c(int x) {
        for (;;) { if (x > 3) return x; x++; }
    }
    static int d(int x) {
        switch (x) {
            case 1: return 1;
            default: return 2;
        }
    }
    static int e(int x) {
        try { return x; } finally { System.out.println(); }
    }
    static int f(int x) {
        try { return Integer.parseInt("1"); } catch (NumberFormatException ex) { return 0; }
    }
    static int g(int x) {
        throw new UnsupportedOperationException();
    }
    static int h(int x) {
        do { x++; } while (x < 3);
        return x;
    }
    static int i(int x) {
        synchronized (Flow.class) { return x; }
    }
    static int j(int x) {
        return switch (x) { case 1 -> 1; default -> { yield 2; } };
    }
    static int k(int x) {
        label: while (true) { if (x > 2) break label; x++; }
        return x;
    }
    static int l(boolean b) {
        if (b) return 1;
        return 0;
    }
    static int m(int x) {
        while (DEBUG) { return x; }
    }
    static int n(Object o) {
        if (o instanceof String s) { return s.length(); }
        else { throw new IllegalStateException(); }
    }
    static void o() {
        int x = 1;
        return;
    }
    static int p(int x) {
        switch (x) {
            case 1 -> { return 1; }
            default -> { return 2; }
        }
    }
    static Callable<Integer> q() { return () -> { return 1; }; }

    static void r() throws IOException, InterruptedException {
        Thread.sleep(1);
        new FileReader("x").close();
        try (Reader rd = new StringReader("a")) { rd.read(); }
        try { Thread.sleep(1); } catch (InterruptedException ie) { Thread.currentThread().interrupt(); }
        try { new FileReader("x"); } catch (IOException | RuntimeException ex) { }
        try { throw new IOException(); } catch (Exception ex) { }
        Callable<String> c = () -> { Thread.sleep(1); return "x"; };
        Runnable run = () -> { try { Thread.sleep(1); } catch (InterruptedException ie) { } };
    }
    interface Thrower { void go() throws Exception; }
    static void s() { Thrower t = () -> { throw new Exception(); }; }

    final int fin;
    Flow() { fin = 1; }
    Flow(int x) { this.fin = x; }
    void t() { final int local; local = 2; int y = local; }
    static final int SC; static { SC = 1; }
}
