// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Differential test: run real `javac`-compiled programs on Hyperion's VM and
//! assert the captured `System.out` matches the reference JVM (`java`) byte for
//! byte. This is the production-grade yardstick — every program here is genuine
//! Java executed from genuine bytecode, with the real JVM as oracle.
//!
//! Run: cargo test -p hyperion-bytecode --test jvm_differential_test -- --nocapture

use hyperion_bytecode::interpreter::{MemoryConfig, ObjectRef, Value, VirtualMachine};
use std::path::{Path, PathBuf};
use std::process::Command;

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn collect_classes(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_classes(&p, out);
            } else if p.extension().is_some_and(|x| x == "class") {
                out.push(p);
            }
        }
    }
}

/// Compile `source` (a single public class named `main_class`) with javac, run
/// it on the reference JVM, and run it on Hyperion. Returns (reference, hyperion).
fn run_both(main_class: &str, source: &str) -> (String, String) {
    let temp = tempfile::Builder::new()
        .prefix(&format!("hyp_diff_{main_class}_"))
        .tempdir()
        .expect("differential temp directory");
    let dir = temp.path();
    let src_path = dir.join(format!("{main_class}.java"));
    std::fs::write(&src_path, source).unwrap();

    let javac = Command::new("javac")
        .arg("-d")
        .arg(dir)
        .arg(&src_path)
        .output()
        .unwrap();
    assert!(
        javac.status.success(),
        "[{main_class}] javac failed:\n{}",
        String::from_utf8_lossy(&javac.stderr)
    );

    // Reference JVM output. Force the en-US locale so number formatting
    // (decimal separator, grouping) is deterministic and matches Hyperion.
    let reference = Command::new("java")
        .arg("-Duser.language=en")
        .arg("-Duser.country=US")
        .arg("-Dfile.encoding=UTF-8")
        .arg("-Dsun.stdout.encoding=UTF-8")
        .arg("-cp")
        .arg(dir)
        .arg(main_class)
        .output()
        .unwrap();
    let reference_out = String::from_utf8_lossy(&reference.stdout).to_string();

    // Hyperion output.
    let mut classes = Vec::new();
    collect_classes(dir, &mut classes);
    classes.sort();
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.set_suppress_console(true);
    let definitions: Vec<_> = classes
        .iter()
        .map(|path| {
            let bytes = std::fs::read(path).unwrap();
            let rel = path.strip_prefix(dir).unwrap().with_extension("");
            let name = rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            (name, bytes)
        })
        .collect();
    let report = vm.load_classes_from_bytes(&definitions);
    assert!(
        report.failures.is_empty(),
        "[{main_class}] class definition failures: {:#?}",
        report.failures
    );
    assert!(
        report.is_loaded(main_class),
        "[{main_class}] was not loaded"
    );
    let hyperion_out = match vm.invoke_static(
        main_class,
        "main",
        "([Ljava/lang/String;)V",
        &[Value::Reference(ObjectRef::Null)],
    ) {
        Ok(_) => vm.captured_stdout().to_string(),
        Err(e) => format!("<<HYPERION ERROR: {e:?}>>\n{}", vm.captured_stdout()),
    };

    (reference_out, hyperion_out)
}

/// The corpus of differential programs. Keep each deterministic (no time/random).
fn programs() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "Arith",
            r#"public class Arith {
                public static void main(String[] a){
                    int s=0; for(int i=1;i<=10;i++) s+=i*i;
                    long l = 1; for(int i=1;i<=15;i++) l*=2;
                    System.out.println(s + " " + l + " " + (s%7) + " " + (-l/3));
                    System.out.println((s<<2) + " " + (s>>1) + " " + (s^123) + " " + (s&15));
                }
            }"#,
        ),
        (
            "Recur",
            r#"public class Recur {
                static long fib(int n){ return n<2 ? n : fib(n-1)+fib(n-2); }
                static int ack(int m,int n){ return m==0 ? n+1 : (n==0 ? ack(m-1,1) : ack(m-1,ack(m,n-1))); }
                public static void main(String[] a){
                    for(int i=0;i<15;i++) System.out.print(fib(i)+" ");
                    System.out.println();
                    System.out.println(ack(2,3));
                }
            }"#,
        ),
        (
            "Strings",
            r#"public class Strings {
                public static void main(String[] a){
                    StringBuilder sb = new StringBuilder();
                    for(int i=0;i<5;i++) sb.append(i).append(',');
                    String s = sb.toString();
                    System.out.println(s + " len=" + s.length());
                    System.out.println(s.substring(2,7).toUpperCase());
                    System.out.println("hello world".replace('o','0').indexOf("w"));
                    System.out.println(String.join("-", "a","b","c"));
                }
            }"#,
        ),
        (
            "Collections",
            r#"import java.util.*;
            public class Collections {
                public static void main(String[] a){
                    List<Integer> xs = new ArrayList<>();
                    for(int i=0;i<6;i++) xs.add(i*i);
                    int sum=0; for(int x: xs) sum+=x;
                    System.out.println(xs + " sum=" + sum);
                    Map<String,Integer> m = new HashMap<>();
                    m.put("a",1); m.put("b",2); m.put("a",3);
                    System.out.println(m.get("a") + " " + m.get("b") + " size=" + m.size());
                }
            }"#,
        ),
        (
            "Inherit",
            r#"public class Inherit {
                static abstract class Shape { abstract double area(); String describe(){ return getClass().getSimpleName()+":"+area(); } }
                static class Circle extends Shape { double r; Circle(double r){this.r=r;} double area(){ return 3*r*r; } }
                static class Square extends Shape { double s; Square(double s){this.s=s;} double area(){ return s*s; } }
                public static void main(String[] a){
                    Shape[] shapes = { new Circle(2), new Square(3) };
                    for(Shape sh: shapes) System.out.println(sh.describe());
                }
            }"#,
        ),
        (
            "Exceptions",
            r#"public class Exceptions {
                static int risky(int x){ if(x<0) throw new IllegalArgumentException("neg"); return 100/x; }
                public static void main(String[] a){
                    int[] inputs = {5, 0, -1, 4};
                    for(int x: inputs){
                        try { System.out.println("ok "+risky(x)); }
                        catch(ArithmeticException e){ System.out.println("arith"); }
                        catch(IllegalArgumentException e){ System.out.println("iae "+e.getMessage()); }
                        finally { System.out.println("done "+x); }
                    }
                }
            }"#,
        ),
        (
            "Switches",
            r#"public class Switches {
                public static void main(String[] a){
                    for(int i=0;i<6;i++){
                        String r;
                        switch(i){
                            case 0: case 1: r="low"; break;
                            case 2: r="two"; break;
                            case 3: r="three"; break;
                            default: r="high";
                        }
                        System.out.print(r+" ");
                    }
                    System.out.println();
                }
            }"#,
        ),
        (
            "Generics",
            r#"import java.util.*;
            public class Generics {
                static <T extends Comparable<T>> T max(List<T> xs){
                    T m = xs.get(0);
                    for(T x: xs) if(x.compareTo(m)>0) m=x;
                    return m;
                }
                public static void main(String[] a){
                    System.out.println(max(Arrays.asList(3,7,2,9,4)));
                    System.out.println(max(Arrays.asList("pear","apple","zebra")));
                }
            }"#,
        ),
        (
            "StaticInit",
            r#"public class StaticInit {
                static int counter;
                static int[] table = new int[5];
                static { counter = 10; for(int i=0;i<5;i++) table[i]=i*counter; }
                public static void main(String[] a){
                    int s=0; for(int v: table) s+=v;
                    System.out.println("counter="+counter+" sum="+s);
                }
            }"#,
        ),
        (
            "Formatting",
            r#"public class Formatting {
                public static void main(String[] a){
                    System.out.printf("%d/%d = %.3f%n", 22, 7, 22.0/7);
                    System.out.println(String.format("[%5d][%-5d][%05d]", 42, 42, 42));
                    System.out.println(String.format("%s=%b hex=%x", "flag", true, 255));
                }
            }"#,
        ),
        (
            "Enums",
            r#"public class Enums {
                enum Day { MON, TUE, WED;
                    boolean isMid(){ return this == TUE || this == WED; } }
                public static void main(String[] a){
                    for(Day d: Day.values()) System.out.println(d + " " + d.ordinal() + " " + d.isMid());
                    System.out.println(Day.valueOf("WED").name());
                }
            }"#,
        ),
        (
            "BitOps",
            r#"public class BitOps {
                public static void main(String[] a){
                    int x = 0xCAFE;
                    System.out.println(Integer.toBinaryString(x) + " " + Integer.toHexString(x));
                    System.out.println(Integer.bitCount(x) + " " + Integer.numberOfLeadingZeros(x));
                    long big = 1L << 40;
                    System.out.println(big + " " + (big >>> 35) + " " + Long.toHexString(big));
                    System.out.println((Integer.MAX_VALUE + 1) + " " + (Integer.MIN_VALUE - 1));
                }
            }"#,
        ),
        (
            "Varargs",
            r#"public class Varargs {
                static int sum(int... xs){ int s=0; for(int x: xs) s+=x; return s; }
                static String tag(String pre, Object... parts){
                    StringBuilder sb = new StringBuilder(pre);
                    for(Object p: parts) sb.append('|').append(p);
                    return sb.toString();
                }
                public static void main(String[] a){
                    System.out.println(sum() + " " + sum(1,2,3) + " " + sum(5,5,5,5));
                    System.out.println(tag("x", 1, "two", 3.5, true));
                }
            }"#,
        ),
        (
            "InstanceOf",
            r#"public class InstanceOf {
                static String kind(Object o){
                    if(o instanceof Integer) return "int:"+((Integer)o + 1);
                    if(o instanceof String) return "str:"+((String)o).length();
                    if(o instanceof int[]) return "arr:"+((int[])o).length;
                    return "other";
                }
                public static void main(String[] a){
                    System.out.println(kind(41));
                    System.out.println(kind("hello"));
                    System.out.println(kind(new int[]{1,2,3}));
                    System.out.println(kind(3.14));
                }
            }"#,
        ),
        (
            "Sorting",
            r#"import java.util.*;
            public class Sorting {
                public static void main(String[] a){
                    int[] xs = {5,2,8,1,9,3};
                    Arrays.sort(xs);
                    System.out.println(Arrays.toString(xs));
                    List<String> words = new ArrayList<>(Arrays.asList("pear","apple","fig","banana"));
                    Collections.sort(words);
                    System.out.println(words);
                    words.sort((x, y) -> x.length() - y.length());
                    System.out.println(words);
                }
            }"#,
        ),
        (
            "Interfaces",
            r#"public class Interfaces {
                interface Greeter { String greet(String n); default String shout(String n){ return greet(n).toUpperCase(); } }
                public static void main(String[] a){
                    Greeter g = n -> "hi " + n;
                    System.out.println(g.greet("bob"));
                    System.out.println(g.shout("sue"));
                }
            }"#,
        ),
        (
            "Arrays2D",
            r#"public class Arrays2D {
                public static void main(String[] a){
                    int n=4; int[][] m = new int[n][n];
                    for(int i=0;i<n;i++) for(int j=0;j<n;j++) m[i][j]=i*n+j;
                    long trace=0; for(int i=0;i<n;i++) trace+=m[i][i];
                    System.out.println("trace="+trace);
                    int[] flat = new int[n*n];
                    for(int i=0;i<n;i++) System.arraycopy(m[i],0,flat,i*n,n);
                    int s=0; for(int v: flat) s+=v;
                    System.out.println("sum="+s);
                }
            }"#,
        ),
        (
            "MathOps",
            r#"public class MathOps {
                public static void main(String[] a){
                    System.out.println(Math.max(3,7) + " " + Math.min(3,7) + " " + Math.abs(-5));
                    System.out.printf("%.4f %.4f %.0f%n", Math.sqrt(2), Math.pow(2,10), Math.floor(3.7));
                    System.out.println(Math.floorDiv(-7,2) + " " + Math.floorMod(-7,2) + " " + (-7/2) + " " + (-7%2));
                    System.out.println(Math.round(2.5) + " " + Math.round(2.4) + " " + (int)Math.ceil(2.1));
                    System.out.println(Math.abs(Integer.MIN_VALUE) + " " + Math.abs(Long.MIN_VALUE));
                    System.out.println(Math.floorDiv(-7,-2) + " " + Math.floorMod(-7,-2) + " " + Math.floorDiv(7,-2) + " " + Math.floorMod(7,-2));
                    System.out.println(Math.round(-1.5) + " " + Math.round(-1.4));
                    double nan = Math.max(Double.NaN, 1.0);
                    System.out.println((nan != nan) + " " + (1.0 / Math.max(-0.0, 0.0)) + " " + (1.0 / Math.min(-0.0, 0.0)));
                    try { Math.addExact(Integer.MAX_VALUE, 1); }
                    catch (ArithmeticException expected) { System.out.println("addExact overflow"); }
                }
            }"#,
        ),
        (
            "ThreadBasics",
            r#"public class ThreadBasics {
                static int ran = 0;
                public static void main(String[] a) throws Exception {
                    Thread worker = new Thread(() -> {
                        ran = 7;
                        System.out.println(Thread.currentThread().getName());
                    }, "worker");
                    worker.start();
                    worker.join();
                    System.out.println(ran + " " + worker.getName() + " " + worker.isAlive());
                    System.out.println(Thread.currentThread().getName());
                }
            }"#,
        ),
        (
            "NumericParsing",
            r#"public class NumericParsing {
                static String parseInt(String value) {
                    try { return Integer.toString(Integer.parseInt(value)); }
                    catch (NumberFormatException expected) { return "NFE"; }
                }
                static String parseLong(String value) {
                    try { return Long.toString(Long.parseLong(value)); }
                    catch (NumberFormatException expected) { return "NFE"; }
                }
                public static void main(String[] a) {
                    System.out.println(parseInt("+7") + " " + parseInt(" 7") + " " + parseInt("7 ") + " " + parseInt("2147483648"));
                    System.out.println(parseLong("-9223372036854775808") + " " + parseLong("9223372036854775808") + " " + parseLong(""));
                }
            }"#,
        ),
        (
            "StringMethods",
            r#"public class StringMethods {
                public static void main(String[] a){
                    String s = "the quick brown fox";
                    String[] parts = s.split(" ");
                    System.out.println(parts.length + " " + parts[0] + " " + parts[3]);
                    System.out.println("ab".repeat(3) + " " + s.indexOf("brown") + " " + s.lastIndexOf("o"));
                    System.out.println(s.toUpperCase().substring(4, 9) + " " + "  pad  ".trim());
                    int vowels = 0;
                    for(char c: s.toCharArray()) if("aeiou".indexOf(c) >= 0) vowels++;
                    System.out.println("vowels=" + vowels);
                }
            }"#,
        ),
        (
            "StringUtf16",
            r#"public class StringUtf16 {
                public static void main(String[] a) {
                    String value = "A😀B";
                    System.out.println(value.length() + " " + (int)value.charAt(1) + " " + (int)value.charAt(2) + " " + value.indexOf("B"));
                    System.out.println(value.substring(1, 3).equals("😀"));
                    char[] units = value.toCharArray();
                    System.out.println(units.length + " " + (int)units[1] + " " + (int)units[2] + " " + value.lastIndexOf("B"));
                    System.out.println(value.indexOf("😀", 2) + " " + value.indexOf((int)value.charAt(2)) + " " + value.lastIndexOf("😀", 1) + " " + value.lastIndexOf("😀", 0));
                    System.out.println(value.hashCode() + " " + "😀".compareTo("\uE000"));
                    StringBuilder builder = new StringBuilder().append(value);
                    System.out.println(builder.length() + " " + (int)builder.charAt(1) + " " + (int)builder.charAt(2) + " " + builder.substring(1, 3).equals("😀"));
                    StringBuilder truncated = new StringBuilder().append(value);
                    truncated.setLength(3);
                    StringBuilder replaced = new StringBuilder().append(value);
                    replaced.setCharAt(3, 'C');
                    StringBuilder deleted = new StringBuilder().append(value);
                    deleted.deleteCharAt(3);
                    StringBuilder inserted = new StringBuilder().append(value);
                    inserted.insert(3, "X");
                    System.out.println(truncated + " " + replaced + " " + deleted + " " + inserted);
                    System.out.println(String.join(",", "a1b22c333".split("\\d+")));
                    System.out.println(String.join(",", "a1b22".split("\\d+", -1)) + " " + String.join(",", "a1b22".split("\\d+", 2)));
                    String padded = "  x  ";
                    System.out.println(padded.trim().length() + " " + padded.strip().length());
                    try { value.charAt(4); }
                    catch (StringIndexOutOfBoundsException expected) { System.out.println("charAt bounds"); }
                    try { value.substring(3, 2); }
                    catch (StringIndexOutOfBoundsException expected) { System.out.println("substring bounds"); }
                    try { builder.charAt(4); }
                    catch (IndexOutOfBoundsException expected) { System.out.println("builder bounds"); }
                    try { builder.insert(5, "X"); }
                    catch (IndexOutOfBoundsException expected) { System.out.println("insert bounds"); }
                    try { value.repeat(-1); }
                    catch (IllegalArgumentException expected) { System.out.println("repeat bounds"); }
                    try { value.split("["); }
                    catch (java.util.regex.PatternSyntaxException expected) { System.out.println("split pattern"); }
                }
            }"#,
        ),
        (
            "TreeStructures",
            r#"import java.util.*;
            public class TreeStructures {
                public static void main(String[] a){
                    TreeMap<String,Integer> tm = new TreeMap<>();
                    tm.put("banana",3); tm.put("apple",1); tm.put("cherry",2);
                    for(Map.Entry<String,Integer> e: tm.entrySet())
                        System.out.println(e.getKey() + "=" + e.getValue());
                    TreeSet<Integer> ts = new TreeSet<>(Arrays.asList(5,1,3,1,4,2));
                    System.out.println(ts + " first=" + ts.first() + " last=" + ts.last());
                }
            }"#,
        ),
        (
            "Ternary",
            r#"public class Ternary {
                static String fizz(int n){ return n%15==0?"FB":n%3==0?"F":n%5==0?"B":""+n; }
                public static void main(String[] a){
                    StringBuilder sb = new StringBuilder();
                    outer:
                    for(int i=1;i<=20;i++){
                        if(i==17) break outer;
                        if(i%2==0) continue;
                        sb.append(fizz(i)).append(' ');
                    }
                    System.out.println(sb.toString().trim());
                }
            }"#,
        ),
        (
            "TryResources",
            r#"public class TryResources {
                static class Res implements AutoCloseable {
                    String name; Res(String n){ name=n; System.out.println("open "+n); }
                    void use(){ System.out.println("use "+name); }
                    public void close(){ System.out.println("close "+name); }
                }
                public static void main(String[] a){
                    try (Res r1 = new Res("A"); Res r2 = new Res("B")) {
                        r1.use(); r2.use();
                    }
                    System.out.println("done");
                }
            }"#,
        ),
        (
            "AnonClass",
            r#"public class AnonClass {
                interface Op { int apply(int x); }
                static int run(Op op, int v){ return op.apply(v); }
                public static void main(String[] a){
                    final int base = 100;
                    Op sq = new Op(){ public int apply(int x){ return x*x + base; } };
                    System.out.println(run(sq, 5) + " " + run(x -> x+1, 9));
                }
            }"#,
        ),
        (
            "SwitchExpr",
            r#"public class SwitchExpr {
                static int score(String grade){
                    return switch(grade){
                        case "A" -> 4;
                        case "B" -> 3;
                        case "C" -> 2;
                        default -> { yield 0; }
                    };
                }
                public static void main(String[] a){
                    for(String g: new String[]{"A","B","C","F"})
                        System.out.print(g + "=" + score(g) + " ");
                    System.out.println();
                }
            }"#,
        ),
        (
            "Streams",
            r#"import java.util.*;
            import java.util.stream.*;
            public class Streams {
                public static void main(String[] a){
                    List<Integer> xs = Arrays.asList(1,2,3,4,5,6,7,8,9,10);
                    int sum = xs.stream().filter(x -> x%2==0).mapToInt(Integer::intValue).sum();
                    System.out.println("evensum=" + sum);
                    List<String> names = Arrays.asList("alice","bob","carol");
                    String joined = names.stream().map(String::toUpperCase).collect(Collectors.joining(","));
                    System.out.println(joined);
                    long count = xs.stream().filter(x -> x > 5).count();
                    System.out.println("count=" + count);
                }
            }"#,
        ),
        (
            "StreamFactories",
            r#"import java.util.*;
            import java.util.stream.*;
            public class StreamFactories {
                public static void main(String[] a){
                    List<String> upper = Stream.of("alice","bob","carol")
                        .filter(s -> s.length() > 3)
                        .map(String::toUpperCase)
                        .collect(Collectors.toList());
                    System.out.println(upper);

                    long soloCount = Stream.of("solo").count();
                    System.out.println("solo=" + soloCount);

                    long emptyCount = Stream.empty().count();
                    System.out.println("empty=" + emptyCount);

                    long presentCount = Stream.ofNullable("x").count();
                    long nullCount = Stream.ofNullable(null).count();
                    System.out.println("present=" + presentCount + " null=" + nullCount);

                    int rangeSum = IntStream.range(1, 6).filter(x -> x % 2 == 0).sum();
                    System.out.println("rangesum=" + rangeSum);
                    int closedSum = IntStream.rangeClosed(1, 5).sum();
                    System.out.println("closedsum=" + closedSum);
                    int ofSum = IntStream.of(3, 1, 4, 1, 5).sum();
                    System.out.println("ofsum=" + ofSum);
                }
            }"#,
        ),
        (
            "Records",
            r#"public class Records {
                record Point(int x, int y){
                    int dist2(){ return x*x + y*y; }
                }
                public static void main(String[] a){
                    Point p = new Point(3, 4);
                    System.out.println(p.x() + " " + p.y() + " " + p.dist2());
                    System.out.println(p);
                    System.out.println(p.equals(new Point(3,4)));
                }
            }"#,
        ),
        (
            "StreamReduceMinMax",
            r#"import java.util.*;
            import java.util.stream.*;
            public class StreamReduceMinMax {
                public static void main(String[] a){
                    List<Integer> xs = Arrays.asList(4,1,7,3,9,2);

                    int product = xs.stream().reduce(1, (x,y) -> x*y);
                    System.out.println("product=" + product);

                    Optional<Integer> maybeSum = xs.stream().reduce((x,y) -> x+y);
                    System.out.println("sum=" + maybeSum.get());

                    Optional<String> longest = Stream.of("pear","apple","fig","banana")
                        .max((s1,s2) -> s1.length() - s2.length());
                    System.out.println("longest=" + longest.get());

                    Optional<Integer> smallest = xs.stream().min((x,y) -> x - y);
                    System.out.println("smallest=" + smallest.get());

                    Optional<Integer> firstBig = xs.stream().filter(x -> x > 5).findFirst();
                    System.out.println("first>5=" + firstBig.orElse(-1));

                    Optional<Integer> none = xs.stream().filter(x -> x > 100).findAny();
                    System.out.println("none present=" + none.isPresent());

                    int emptyReduce = Stream.<Integer>empty().reduce(0, (x,y) -> x+y);
                    System.out.println("emptyReduce=" + emptyReduce);
                }
            }"#,
        ),
        (
            "StreamFlatMapPeek",
            r#"import java.util.*;
            import java.util.stream.*;
            public class StreamFlatMapPeek {
                public static void main(String[] a){
                    List<List<Integer>> nested = Arrays.asList(
                        Arrays.asList(1,2,3),
                        Arrays.asList(4,5),
                        Arrays.asList(6,7,8,9)
                    );
                    List<Integer> flat = nested.stream()
                        .flatMap(xs -> xs.stream())
                        .collect(Collectors.toList());
                    System.out.println("flat=" + flat);

                    StringBuilder trace = new StringBuilder();
                    List<Integer> evens = flat.stream()
                        .peek(x -> trace.append(x).append(','))
                        .filter(x -> x % 2 == 0)
                        .collect(Collectors.toList());
                    System.out.println("trace=" + trace);
                    System.out.println("evens=" + evens);

                    List<Integer> taken = flat.stream().takeWhile(x -> x < 6).collect(Collectors.toList());
                    System.out.println("taken=" + taken);

                    List<Integer> dropped = flat.stream().dropWhile(x -> x < 6).collect(Collectors.toList());
                    System.out.println("dropped=" + dropped);
                }
            }"#,
        ),
        (
            "IntStreamAdvanced",
            r#"import java.util.*;
            import java.util.stream.*;
            public class IntStreamAdvanced {
                public static void main(String[] a){
                    int[] xs = {5,3,8,1,9,3,5};

                    OptionalDouble avg = IntStream.of(xs).average();
                    System.out.printf("avg=%.4f%n", avg.getAsDouble());

                    int sumReduced = IntStream.of(xs).reduce(0, (x,y) -> x+y);
                    System.out.println("sumReduced=" + sumReduced);

                    OptionalInt maybeProduct = IntStream.rangeClosed(1,5).reduce((x,y) -> x*y);
                    System.out.println("factorial5=" + maybeProduct.getAsInt());

                    OptionalInt mn = IntStream.of(xs).min();
                    OptionalInt mx = IntStream.of(xs).max();
                    System.out.println("min=" + mn.getAsInt() + " max=" + mx.getAsInt());

                    List<Integer> boxedSorted = IntStream.of(xs)
                        .distinct()
                        .sorted()
                        .boxed()
                        .collect(Collectors.toList());
                    System.out.println("boxedSorted=" + boxedSorted);

                    List<String> mapped = IntStream.range(0,5)
                        .mapToObj(i -> "x" + i)
                        .collect(Collectors.toList());
                    System.out.println("mapped=" + mapped);
                }
            }"#,
        ),
    ]
}

#[test]
fn jvm_differential() {
    if !tool_available("javac") || !tool_available("java") {
        eprintln!("skipping jvm_differential: javac/java not on PATH");
        return;
    }
    let mut pass = 0;
    let mut fail = 0;
    let mut failures = Vec::new();
    for (name, src) in programs() {
        let (reference, hyperion) = run_both(name, src);
        if reference == hyperion {
            pass += 1;
            println!("[MATCH] {name}");
        } else {
            fail += 1;
            println!(
                "[DIFF]  {name}\n  --- reference ---\n{reference}\n  --- hyperion ---\n{hyperion}"
            );
            failures.push(name);
        }
    }
    println!("\njvm_differential: {pass} match, {fail} differ");
    assert!(
        failures.is_empty(),
        "VM output diverged from the reference JVM for: {failures:?}"
    );
}

#[test]
fn boxed_numeric_order_matches_jvm() {
    if !tool_available("javac") || !tool_available("java") {
        eprintln!("skipping boxed_numeric_order_matches_jvm: javac/java not on PATH");
        return;
    }
    let source = r#"import java.util.*;
        public class BoxedNumericOrder {
            public static void main(String[] args) {
                Long low = 9007199254740992L;
                Long high = 9007199254740993L;
                System.out.println(low.compareTo(high));
                TreeSet<Long> values = new TreeSet<>();
                values.add(high);
                values.add(low);
                System.out.println(values.first() + "," + values.last());
                System.out.println(Double.valueOf(-0.0).compareTo(0.0));
                System.out.println(Double.valueOf(Double.NaN).compareTo(Double.POSITIVE_INFINITY));
                System.out.println(Float.valueOf(Float.NaN).compareTo(Float.POSITIVE_INFINITY));
                try { low.compareTo(null); }
                catch (NullPointerException expected) { System.out.println("null rejected"); }
                try { ((Comparable) low).compareTo(Integer.valueOf(1)); }
                catch (ClassCastException expected) { System.out.println("wrong wrapper rejected"); }
                TreeSet<String> strings = new TreeSet<>();
                strings.add("𐀀");
                strings.add("");
                System.out.println(strings.first().equals("𐀀"));
            }
        }"#;
    let (reference, hyperion) = run_both("BoxedNumericOrder", source);
    assert!(reference.contains("9007199254740992,9007199254740993"));
    assert!(reference.contains("null rejected"));
    assert!(reference.contains("wrong wrapper rejected"));
    assert!(reference.ends_with("true\n"));
    assert_eq!(hyperion, reference);
}

#[test]
fn sorting_an_arrays_as_list_view_matches_jvm() {
    if !tool_available("javac") || !tool_available("java") {
        eprintln!("skipping sorting_an_arrays_as_list_view_matches_jvm: javac/java not on PATH");
        return;
    }
    let source = r#"import java.util.*;
        public class AsListSort {
            public static void main(String[] args) {
                Integer[] numbers = {3, 1, 2};
                Collections.sort(Arrays.asList(numbers));
                System.out.println(Arrays.toString(numbers));
                String[] words = {"pear", "fig", "apple"};
                List<String> view = Arrays.asList(words);
                view.sort((a, b) -> b.compareTo(a));
                System.out.println(Arrays.toString(words));
                view.sort(null);
                System.out.println(view);
                Collections.sort(view, (a, b) -> b.length() - a.length());
                System.out.println(Arrays.toString(words));
            }
        }"#;
    let (reference, hyperion) = run_both("AsListSort", source);
    assert_eq!(hyperion, reference);
}

#[test]
fn removing_through_a_map_view_removes_from_the_map_as_on_the_jvm() {
    if !tool_available("javac") || !tool_available("java") {
        eprintln!("skipping removing_through_a_map_view_removes_from_the_map_as_on_the_jvm: javac/java not on PATH");
        return;
    }
    let source = r#"import java.util.*;
        public class MapViewRemove {
            static Map<String, Integer> fresh() {
                Map<String, Integer> m = new HashMap<>();
                m.put("a", 1);
                m.put("b", 2);
                m.put("c", 3);
                return m;
            }
            public static void main(String[] args) {
                Map<String, Integer> m = fresh();
                System.out.println(m.keySet().remove("b") + " " + m.keySet().remove("z") + " " + m);
                m = fresh();
                System.out.println(m.keySet().removeIf(k -> k.equals("a")) + " " + m);
                m = fresh();
                System.out.println(m.values().remove(3) + " " + m);
                m = fresh();
                System.out.println(m.values().removeIf(v -> v > 1) + " " + m);
                m = fresh();
                System.out.println(m.entrySet().removeIf(e -> e.getValue() == 2) + " " + m);
                m = fresh();
                m.keySet().clear();
                System.out.println(m.size());
                m = fresh();
                m.values().clear();
                System.out.println(m.isEmpty());
                m = fresh();
                m.entrySet().clear();
                System.out.println(m.size());
            }
        }"#;
    let (reference, hyperion) = run_both("MapViewRemove", source);
    assert_eq!(hyperion, reference);
}

#[test]
fn remove_if_on_a_linked_list_matches_jvm() {
    if !tool_available("javac") || !tool_available("java") {
        eprintln!("skipping remove_if_on_a_linked_list_matches_jvm: javac/java not on PATH");
        return;
    }
    let source = r#"import java.util.*;
        public class LinkedListRemoveIf {
            public static void main(String[] args) {
                List<Integer> list = new LinkedList<>();
                list.add(1);
                list.add(2);
                list.add(3);
                list.add(1);
                System.out.println(list.removeIf(x -> x == 1) + " " + list);
                System.out.println(list.removeIf(x -> x > 5) + " " + list);
            }
        }"#;
    let (reference, hyperion) = run_both("LinkedListRemoveIf", source);
    assert_eq!(hyperion, reference);
}

#[test]
fn a_map_or_set_keeps_the_key_object_that_inserted_the_entry_as_on_the_jvm() {
    if !tool_available("javac") || !tool_available("java") {
        eprintln!("skipping a_map_or_set_keeps_the_key_object_that_inserted_the_entry_as_on_the_jvm: javac/java not on PATH");
        return;
    }
    let source = r#"import java.util.*;
        public class KeyIdentity {
            static class K {
                final int id;
                K(int id) { this.id = id; }
                public boolean equals(Object o) { return o instanceof K && ((K) o).id == id; }
                public int hashCode() { return id; }
            }
            public static void main(String[] args) {
                String a = new String("k");
                String b = new String("k");
                Map<String, Integer> first = new HashMap<>();
                first.put(a, 1);
                Map<String, Integer> second = new HashMap<>();
                second.put(b, 2);
                System.out.println(second.keySet().iterator().next() == b);
                first.put(b, 3);
                System.out.println(first.keySet().iterator().next() == a);
                first.keySet().remove(a);
                first.put(b, 4);
                System.out.println(first.keySet().iterator().next() == b);
                Integer x = 1000;
                Integer y = 1000;
                Set<Integer> xs = new HashSet<>();
                xs.add(x);
                Set<Integer> ys = new HashSet<>();
                ys.add(y);
                System.out.println(ys.iterator().next() == y);
                K k1 = new K(7);
                K k2 = new K(7);
                Set<K> ks1 = new HashSet<>();
                ks1.add(k1);
                Set<K> ks2 = new HashSet<>();
                System.out.println(ks2.contains(k2));
                ks2.add(k2);
                System.out.println(ks2.iterator().next() == k2);
            }
        }"#;
    let (reference, hyperion) = run_both("KeyIdentity", source);
    assert_eq!(hyperion, reference);
}
