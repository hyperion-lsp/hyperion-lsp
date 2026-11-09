// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for `hyperion-eval --class`: run real `javac`-compiled
//! `.class` files directly in the Hyperion bytecode VM instead of compiling
//! source with Hyperion's own Java frontend.
//!
//! Compiles a small multi-class program (main class + helper class + a
//! record + a stream pipeline) with a real `javac`, runs it through the new
//! `--class` mode, and asserts stdout matches byte-for-byte.
//!
//! Self-skips (eprintln + return) if `javac` is not on `PATH`, following the
//! same pattern as the ignored differential harness in this directory
//! (`groovy_differential.rs`) and in
//! `hyperion-bytecode/tests/jvm_differential_test.rs`. Unlike those, these
//! tests are *not* `#[ignore]`d: they are the acceptance test for this
//! feature and are meant to run as part of the normal `cargo nextest run`.

use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

const EVAL_BIN: &str = env!("CARGO_BIN_EXE_hyperion-eval");

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Compile every `.java` file in `dir` with `javac`, in place.
fn javac_compile(dir: &Path, sources: &[&str]) {
    let mut cmd = Command::new("javac");
    cmd.arg("-d").arg(dir);
    for s in sources {
        cmd.arg(dir.join(s));
    }
    let out = cmd.output().expect("failed to spawn javac");
    assert!(
        out.status.success(),
        "javac failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write_file(dir: &Path, name: &str, contents: &str) {
    let path = dir.join(name);
    let mut f =
        std::fs::File::create(&path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    f.write_all(contents.as_bytes()).unwrap();
}

const MAIN_SRC: &str = r#"import java.util.Arrays;
import java.util.List;
import java.util.stream.Collectors;

public class Main {
    record Point(int x, int y) {
        int sum() { return x + y; }
    }

    public static void main(String[] args) {
        Helper h = new Helper("hi");
        System.out.println(h.greet());
        List<Point> pts = Arrays.asList(new Point(1, 2), new Point(3, 4));
        String joined = pts.stream()
            .map(p -> p.sum() + "")
            .collect(Collectors.joining(","));
        System.out.println("sums=" + joined);
        for (String a : args) {
            System.out.println("arg=" + a);
        }
    }
}
"#;

const HELPER_SRC: &str = r#"public class Helper {
    private final String name;
    Helper(String name) { this.name = name; }
    String greet() { return "Hello, " + name; }
}
"#;

const BOOM_SRC: &str = r#"public class Boom {
    public static void main(String[] args) {
        System.out.println("before");
        if (args.length == 0 || args.length >= 0) {
            throw new RuntimeException("kaboom");
        }
        System.out.println("after");
    }
}
"#;

#[test]
fn class_mode_runs_multi_class_program() {
    if !tool_available("javac") {
        eprintln!("SKIP class_mode_runs_multi_class_program: javac not on PATH");
        return;
    }

    let dir = std::env::temp_dir().join("hyp_eval_class_mode_multi");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_file(&dir, "Main.java", MAIN_SRC);
    write_file(&dir, "Helper.java", HELPER_SRC);
    javac_compile(&dir, &["Main.java", "Helper.java"]);

    // Sanity: the record's nested class file must actually be there, or this
    // test would trivially pass without exercising multi-class loading.
    assert!(
        dir.join("Main$Point.class").exists(),
        "expected javac to emit Main$Point.class"
    );
    assert!(dir.join("Helper.class").exists());

    let started = Instant::now();
    let out = Command::new(EVAL_BIN)
        .arg("--class")
        .arg("Main")
        .arg("--classpath")
        .arg(&dir)
        .arg("--")
        .arg("foo")
        .arg("bar")
        .output()
        .expect("failed to run hyperion-eval");
    let elapsed = started.elapsed();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "hyperion-eval --class Main exited with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );

    let expected = "Hello, hi\nsums=3,7\narg=foo\narg=bar\n";
    assert_eq!(
        stdout, expected,
        "stdout must match byte-for-byte (no banners/prefixes)"
    );

    println!(
        "[class_mode_runs_multi_class_program] measured wall time: {:?} ({} ms)",
        elapsed,
        elapsed.as_millis()
    );
}

#[test]
fn class_mode_uncaught_exception_nonzero_exit() {
    if !tool_available("javac") {
        eprintln!("SKIP class_mode_uncaught_exception_nonzero_exit: javac not on PATH");
        return;
    }

    let dir = std::env::temp_dir().join("hyp_eval_class_mode_throw");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_file(&dir, "Boom.java", BOOM_SRC);
    javac_compile(&dir, &["Boom.java"]);

    let out = Command::new(EVAL_BIN)
        .arg("--class")
        .arg("Boom")
        .arg("--classpath")
        .arg(&dir)
        .output()
        .expect("failed to run hyperion-eval");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        !out.status.success(),
        "expected nonzero exit for an uncaught exception, got success. stdout:\n{stdout}"
    );

    // stdout is exact up to the throw point -- nothing printed after it, and
    // nothing else added (no banners on stdout).
    assert_eq!(stdout, "before\n");

    // stderr carries a stack-trace-ish uncaught-exception message.
    assert!(
        stderr.contains("Exception") && stderr.contains("RuntimeException"),
        "expected a stack-trace-ish message on stderr, got:\n{stderr}"
    );
}
