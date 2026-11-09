// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Class-file execution mode: load and run real `javac`-compiled `.class`
//! files directly in the Hyperion bytecode VM, bypassing Hyperion's own
//! (weaker) Java frontend entirely.
//!
//! This is deliberately thin wiring around `hyperion_bytecode::interpreter`:
//! it eagerly loads every `.class` on the classpath, then
//! `invoke_static(..., "main", "([Ljava/lang/String;)V", ...)`), just fed
//! from a real classpath (directories and/or JARs) instead of a single temp
//! dir populated by the test harness itself.

use hyperion_bytecode::classpath::load_classpath_classes;
use hyperion_bytecode::interpreter::{MemoryConfig, VMError, VirtualMachine};

/// Load every `.class` on `classpath`, invoke `main_class`'s
/// `public static void main(String[] args)` with `program_args`, and return
/// the process exit code (0 on normal completion, nonzero on failure).
///
/// `classpath` entries use the platform path separator; each entry is either
/// a directory containing package-structured `.class` files or a `.jar`
/// archive.
/// Program stdout is produced by the VM's normal `System.out` console echo
/// (untouched here), so it reaches the real process stdout byte-for-byte,
/// with no banners or prefixes added by this function. All diagnostics below
/// go to stderr only.
pub fn run_class(
    main_class: &str,
    classpath: &str,
    program_args: &[String],
    timeout_ms: u64,
) -> i32 {
    let classes = match collect_classpath_classes(classpath) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    if classes.is_empty() {
        eprintln!("Error: no .class files found on classpath '{classpath}'");
        return 1;
    }

    let config = MemoryConfig {
        max_instructions: timeout_ms.saturating_mul(10_000),
        ..MemoryConfig::default()
    };
    let mut vm = VirtualMachine::new(config);

    // Stage the whole classpath so superclass/interface definition is
    // independent of alphabetical discovery order. As on a JVM classpath,
    // malformed unrelated classes remain lazy: they do not prevent an already
    // linked main class from running.
    let load_report = vm.load_classes_from_bytes(&classes);

    // Internal JVM class names use '/' separators; accept the main class
    // spelled either way (`com.example.Main` or `com/example/Main`).
    let main_internal = main_class.replace('.', "/");

    if !classes.iter().any(|(name, _)| name == &main_internal) {
        eprintln!("Error: class '{main_class}' not found on classpath '{classpath}'");
        return 1;
    }
    if !load_report.is_loaded(&main_internal) {
        eprintln!("Error: class '{main_class}' could not be linked");
        if let Some(failure) = load_report.failure_for(&main_internal) {
            eprintln!("  {}: {}", failure.class_name, failure.error);
        }
        return 1;
    }

    let args_value = match vm.make_string_array(program_args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: failed to build String[] args: {e}");
            return 1;
        }
    };

    match vm.invoke_static(
        &main_internal,
        "main",
        "([Ljava/lang/String;)V",
        &[args_value],
    ) {
        Ok(_) => 0,
        Err(VMError::UncaughtException {
            exception_class,
            stack_trace,
            ..
        }) => {
            // Best-effort JVM-style rendering. Hyperion's `UncaughtException`
            // carries the exception's class and a frame list, but not the
            // exception's own detail message (e.g. `new RuntimeException("x")`'s
            // "x") -- that would require reaching into the thrown object's
            // heap fields, which is not exposed by the current VM API and is
            // out of scope here (CLI-layer wiring only).
            eprintln!(
                "Exception in thread \"main\" {}",
                exception_class.replace('/', ".")
            );
            if !stack_trace.is_empty() {
                eprintln!("{stack_trace}");
            }
            1
        }
        Err(VMError::NoSuchMethod(m)) => {
            eprintln!(
                "Error: no such method: {m} (expected `public static void main(String[] args)` in {main_class})"
            );
            1
        }
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

/// Eagerly collect `(canonical_class_name, bytecode)` for every `.class` file
/// reachable from the platform-separated `classpath` directories and/or JAR
/// files. Canonical names use `/` separators (e.g. `com/example/Main`),
/// matching the internal form the VM expects. Earlier classpath entries take
/// precedence over later ones on name collision (standard classpath order).
fn collect_classpath_classes(classpath: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let entries = std::env::split_paths(classpath)
        .filter(|path| !path.as_os_str().is_empty())
        .inspect(|path| {
            if !path.is_dir() && path.extension().is_none_or(|extension| extension != "jar") {
                eprintln!(
                    "Warning: classpath entry '{}' is not a directory or .jar file; skipping",
                    path.display()
                );
            }
        })
        .collect::<Vec<_>>();
    load_classpath_classes(&entries).map_err(|error| error.to_string())
}
