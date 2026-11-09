package v;

import java.util.*;

public class Access {
    private int secret = 1;
    private static int sstat = 2;
    protected int prot = 3;
    int pkg = 4;
    private void hidden() {}

    static class Nested {
        private int nsecret;
        void use(Access a) { a.secret++; a.hidden(); sstat++; }
    }
    void use(Nested n) { n.nsecret++; }
    class Inner { void use() { secret++; hidden(); } }
}

class OtherInPackage {
    void use(Access a) { a.pkg++; a.prot++; }
}

class Sub extends Access {
    void use() { prot++; pkg++; }
}
