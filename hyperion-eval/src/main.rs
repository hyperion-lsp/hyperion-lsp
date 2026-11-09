// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hyperion Eval CLI - Execute JVM code instantly without the JVM
//!
//! Usage: hyperion-eval '1+1'  -> 2

use clap::Parser;
use hyperion_eval::{eval, EvalOptions, Language};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "hyperion-eval")]
#[command(about = "Execute JVM code instantly without the JVM")]
#[command(version)]
#[command(after_help = r#"EXAMPLES:
    hyperion-eval -l groovy '1 + 1'              # -> 2 (expressions)
    hyperion-eval -l groovy '"hello".length()'   # -> 5
    hyperion-eval 'System.out.println("Hello");' # -> Hello (Java statements)
    hyperion-eval -f Main.java                   # Execute file
    hyperion-eval -l kotlin 'listOf(1,2,3).sum()'# -> 6

NOTE: Use -l groovy for expressions that return values.
      Java mode is best for statements (void methods).

    hyperion-eval --class Main --classpath out             # run javac-compiled classes
    hyperion-eval --class Main -cp out:lib.jar -- a b c     # with a classpath and argv
"#)]
struct Cli {
    /// Expression or statement to evaluate
    #[arg()]
    code: Option<String>,

    /// Execute a source file
    #[arg(short, long)]
    file: Option<PathBuf>,

    /// Run a `javac`-compiled class from the classpath instead of compiling
    /// source with Hyperion's own frontend. Names `public static void
    /// main(String[] args)` on the given class.
    #[arg(long = "class")]
    class: Option<String>,

    /// Classpath for `--class` mode: `:`-separated directories and/or `.jar`
    /// files. `-cp` is accepted as an alias (rewritten to `--classpath`
    /// before parsing, since clap short options are single-character).
    #[arg(long = "classpath")]
    classpath: Option<String>,

    /// Arguments passed to `main(String[] args)` in `--class` mode. Put after
    /// a literal `--`.
    #[arg(last = true)]
    program_args: Vec<String>,

    /// Language: java, kotlin, groovy [default: auto-detect]
    #[arg(short, long)]
    lang: Option<String>,

    /// Print result type alongside value
    #[arg(short = 't', long)]
    r#type: bool,

    /// Show compilation details
    #[arg(short, long)]
    verbose: bool,

    /// Execution timeout in milliseconds
    #[arg(long, default_value = "5000")]
    timeout: u64,

    /// Debug aid: write the produced Java `.class` bytes to this path and exit
    /// (inspect with `javap -v`). Hidden; for diagnosing ClassFormatError.
    #[arg(long, hide = true)]
    emit_class: Option<PathBuf>,
}

fn main() {
    // clap short options are a single character, so `-cp` (two letters, one
    // dash -- a `javac`/`java`-ism, not a POSIX short flag) can't be declared
    // as a `#[arg(short)]` alias. Rewrite it to `--classpath` before parsing.
    // Only tokens *before* a literal `--` are eligible, so a program argument
    // that happens to spell `-cp` (after `--`) is passed through untouched.
    let raw_args: Vec<String> = std::env::args().collect();
    let sep_idx = raw_args.iter().position(|a| a == "--");
    let (head, tail) = match sep_idx {
        Some(i) => (&raw_args[..i], &raw_args[i..]),
        None => (&raw_args[..], &[][..]),
    };
    let mut normalized: Vec<String> = head
        .iter()
        .map(|a| {
            if a == "-cp" {
                "--classpath".to_string()
            } else {
                a.clone()
            }
        })
        .collect();
    normalized.extend_from_slice(tail);

    let cli = Cli::parse_from(normalized);

    // Class-file execution mode: load real `javac`-compiled `.class` files
    // from a classpath and run them directly in the bytecode VM, bypassing
    // Hyperion's own Java frontend entirely.
    if let Some(main_class) = &cli.class {
        let classpath = cli.classpath.clone().unwrap_or_else(|| ".".to_string());
        let exit_code =
            hyperion_eval::run_class(main_class, &classpath, &cli.program_args, cli.timeout);
        std::process::exit(exit_code);
    }

    // Get source code
    let code = match (&cli.code, &cli.file) {
        (Some(code), None) => code.clone(),
        (None, Some(path)) => match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error reading file: {e}");
                std::process::exit(1);
            }
        },
        (Some(_), Some(_)) => {
            eprintln!("Error: Cannot specify both code and file");
            std::process::exit(1);
        }
        (None, None) => {
            eprintln!("Error: No code provided. Use --help for usage.");
            std::process::exit(1);
        }
    };

    // Determine language
    let language = if let Some(ref lang_str) = cli.lang {
        Some(lang_str.parse::<Language>().unwrap_or_else(|_| {
            eprintln!("Error: Unknown language '{lang_str}'. Use java, kotlin, or groovy.");
            std::process::exit(1);
        }))
    } else if let Some(ref path) = cli.file {
        // Try to detect from file extension
        hyperion_eval::snippet::detect_language_from_extension(&path.to_string_lossy())
    } else {
        None // Auto-detect from content
    };

    // Debug aid: dump the produced .class and exit (for `javap -v` inspection).
    if let Some(out) = &cli.emit_class {
        match hyperion_eval::java_class_bytes(&code) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(out, &bytes) {
                    eprintln!("Error writing class file: {e}");
                    std::process::exit(1);
                }
                eprintln!("wrote {} bytes to {}", bytes.len(), out.display());
                return;
            }
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }

    let options = EvalOptions {
        language,
        timeout_ms: cli.timeout,
        verbose: cli.verbose,
        show_type: cli.r#type,
    };

    match eval(&code, &options) {
        Ok(result) => {
            // A void entry point (e.g. `public static void main`) has no value to
            // print — its observable effect is whatever it wrote to stdout.
            if result.is_void {
                return;
            }
            if options.show_type {
                if let Some(ref ty) = result.result_type {
                    let display = &result.display;
                    println!("{display} : {ty}");
                } else {
                    let display = &result.display;
                    println!("{display}");
                }
            } else {
                let display = &result.display;
                println!("{display}");
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
