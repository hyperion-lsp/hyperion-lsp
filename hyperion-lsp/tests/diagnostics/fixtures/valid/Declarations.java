package v;

import java.lang.annotation.*;
import java.util.*;

public class Declarations {
    private boolean isObject, squash;
    int a = 1, b = a + 1, c[] = new int[b];
    static final int X = 1, Y = X + 1;

    @Retention(RetentionPolicy.RUNTIME)
    @interface Marker {
        String value() default "";
        Kind kind() default Kind.A;
        enum Kind { A, B }
        int LIMIT = 3;
        @interface Nested { int n(); }
    }

    static class Builder {
        Builder with(String s) { return this; }
        Builder record(int x) { return this; }
        Builder open(int y) { return this; }
    }

    void t(Object o, Marker m) {
        for (int i = 0, len = 3; i < len; ++i) { }
        int p = 1, q = p + 1;
        q++;
        boolean flag = isObject || squash;
        String names = String.class.getName() + Integer.class.getSimpleName() + int[].class.getName();
        boolean assignable = Object.class.isAssignableFrom(String.class);
        Builder builder = new Builder().with("x").record(1).open(2);
        switch (m.kind()) {
            case A: break;
            case B: break;
        }
        int lim = Marker.LIMIT + c.length + X + Y;
        Marker.Kind k = Marker.Kind.A;
        Marker.Nested n = null;
        String v = m.value();
    }
}
