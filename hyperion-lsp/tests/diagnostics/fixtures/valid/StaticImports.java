package v;

import static java.lang.Math.*;
import static java.util.stream.Collectors.toList;
import static java.util.Objects.requireNonNull;
import static v.Helpers.helper;
import static v.Helpers.CONST;
import static org.unindexed.Assertions.*;
import java.util.*;
import java.util.stream.*;

public class StaticImports {
    void test() {
        double d = max(1, 2.0) + abs(-1) + sqrt(4) + PI + floor(1.5);
        List<String> l = Stream.of("a").collect(toList());
        String s = requireNonNull("x");
        helper(1);
        helper("a");
        int c = CONST;
        assertEquals(1, 1);
        assertTrue(true);
    }
}

class Helpers {
    static final int CONST = 1;
    static void helper(int x) {}
    static void helper(String x) {}
}
