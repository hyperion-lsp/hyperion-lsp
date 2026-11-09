package v;

import java.util.*;
import java.io.*;
import org.unindexed.lib.BaseComponent;

public class InheritedLibrary extends BaseComponent {
    void test() {
        doSomethingInherited();
        config().get("x");
        this.anotherInherited(1, 2, 3);
        super.moreInherited();
        inheritedField++;
    }

    static class MyList extends ArrayList<String> {
        void test() { add("x"); size(); isEmpty(); stream(); forEach(System.out::println); removeIf(String::isEmpty); }
    }

    static class MyException extends RuntimeException {
        MyException(String m) { super(m); }
        void test() { getMessage(); printStackTrace(); getCause(); addSuppressed(this); }
    }

    static class MyReader extends BufferedReader {
        MyReader(Reader r) { super(r); }
        void test() throws IOException { readLine(); read(); ready(); close(); lines(); }
    }

    static class MyThread extends Thread {
        public void run() { setName("x"); getName(); interrupt(); isAlive(); join(); }
    }

    static class MyMap extends HashMap<String, Integer> {
        void test() { put("a", 1); get("a"); entrySet(); keySet(); values(); merge("a", 1, Integer::sum); }
    }
}
