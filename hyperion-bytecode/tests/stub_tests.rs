// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Stubs rendered from real class files (compiled with the JDK's `javac`;
//! skipped when none is on PATH).

use std::path::Path;
use std::process::Command;

fn javac_available() -> bool {
    Command::new("javac")
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn compile(dir: &Path, name: &str, source: &str) {
    std::fs::write(dir.join(format!("{name}.java")), source).unwrap();
    let output = Command::new("javac")
        .arg("-g")
        .arg("-d")
        .arg(dir)
        .arg(dir.join(format!("{name}.java")))
        .output()
        .expect("javac");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stub_of(dir: &Path, top: &str, nested: &[&str]) -> String {
    let top_bytes = std::fs::read(dir.join(format!("{top}.class"))).unwrap();
    let nested_bytes: Vec<Vec<u8>> = nested
        .iter()
        .map(|name| std::fs::read(dir.join(format!("{name}.class"))).unwrap())
        .collect();
    hyperion_bytecode::stub::render_class_stub(&top_bytes, &nested_bytes).expect("stub")
}

#[test]
fn stub_lists_signatures_generics_constants_and_nested_types() {
    if !javac_available() {
        eprintln!("SKIP: no javac");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    compile(
        temp.path(),
        "Registry",
        r#"package demo.lib;
import java.util.*;
public class Registry<K extends Comparable<K>, V> implements Iterable<V> {
    public static final int LIMIT = 42;
    public static final String NAME = "reg";
    protected Map<K, List<V>> entries;
    private int hidden;
    public Registry() {}
    public <T extends V> T lookup(K key, Class<T> type) throws java.io.IOException { return null; }
    public void putAll(K key, V... values) {}
    public Iterator<V> iterator() { return null; }
    public static <A> List<A> listOf(A a) { return null; }
    protected abstract static class Base<T> { abstract T value(); }
    public interface Listener { void changed(String key); default void done() {} }
    public enum Mode { FAST, SLOW; public int cost() { return 1; } }
}
"#,
    );
    let stub = stub_of(
        temp.path(),
        "demo/lib/Registry",
        &[
            "demo/lib/Registry$Base",
            "demo/lib/Registry$Listener",
            "demo/lib/Registry$Mode",
        ],
    );
    for expected in [
        "package demo.lib;",
        "public class Registry<K, V> implements java.lang.Iterable<V>",
        "public static final int LIMIT = 42;",
        "public static final java.lang.String NAME = \"reg\";",
        "protected java.util.Map<K,java.util.List<V>> entries;",
        "public Registry()",
        "public <T> T lookup(K key, java.lang.Class<T> type) throws java.io.IOException",
        "putAll(K key, V... values)",
        "public static <A> java.util.List<A> listOf(A a)",
        "protected static abstract class Base<T>",
        "public interface Listener",
        "void changed(java.lang.String arg0);",
        "default void done()",
        "public enum Mode",
        "FAST, SLOW;",
        "public int cost()",
    ] {
        assert!(stub.contains(expected), "missing {expected:?} in:\n{stub}");
    }
    assert!(
        !stub.contains("hidden"),
        "private members are omitted:\n{stub}"
    );
    assert!(
        !stub.contains("valueOf") && !stub.contains("values()"),
        "{stub}"
    );
}
