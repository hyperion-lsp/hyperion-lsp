package v;

import java.util.*;

public class NestedQualified {
    static class Cart {
        class Audit { void note() {} }
        Audit newAudit() { return new Audit(); }
        interface Listener { void on(Audit a); }
        enum Mode { A, B }
        static class Builder { Cart build() { return new Cart(); } }
    }

    static class Sub extends Cart {
        Mode mode = Mode.A;          // nested enum inherited from the superclass
        Audit audit() { return newAudit(); }
    }

    void t() {
        Cart cart = new Cart();
        Cart.Audit a = cart.new Audit();
        Cart.Audit b = cart.newAudit();
        NestedQualified.Cart.Audit c = b;
        a.note();
        c.note();
        Cart.Mode m = Cart.Mode.B;
        Cart.Builder builder = new Cart.Builder();
        Cart built = builder.build();
        Cart.Listener l = x -> x.note();
        Map.Entry<String, Integer> e = new AbstractMap.SimpleEntry<>("a", 1);
        Map.Entry<String, Integer> e2 = Map.entry("a", 1);
        Sub s = new Sub();
        Cart.Audit sa = s.audit();
    }
}
