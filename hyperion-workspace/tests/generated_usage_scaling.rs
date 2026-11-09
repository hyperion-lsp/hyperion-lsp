// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Reproducible generated-Java scaling probe for the source indexing path.
//! Run with `cargo test -p hyperion-workspace --test generated_usage_scaling
//! -- --ignored --nocapture` and compare the same binary profile on both revisions.

use hyperion_workspace::WorkspaceIndex;
use std::time::Instant;

fn generated_source(members: usize) -> String {
    let mut source = String::from("package generated; public class Message {\n");
    for member in 0..members {
        source.push_str(&format!(
            "private int field{member}; \
             public int getField{member}() {{ return field{member}; }} \
             public void setField{member}(int value) {{ this.field{member} = value; }}\n"
        ));
    }
    source.push_str("public int checksum() { int sum = 0;\n");
    for member in 0..members {
        source.push_str(&format!("sum += this.getField{member}();\n"));
    }
    source.push_str("return sum; } }");
    source
}

fn generated_repeated_calls(calls: usize) -> String {
    let mut source = generated_source(32);
    source.truncate(
        source
            .rfind("public int checksum()")
            .expect("checksum declaration"),
    );
    source.push_str("public int checksum() { Message other = this; int sum = 0;\n");
    for call in 0..calls {
        source.push_str(&format!("sum += other.getField{}();\n", call % 32));
    }
    source.push_str("return sum; } }");
    source
}

fn protocol_source(methods: usize) -> String {
    let mut source = String::from("package generated; public class Protocol {\n");
    for method in 0..methods {
        source.push_str(&format!(
            "public int read{method}() {{ return {method}; }}\n"
        ));
    }
    source.push('}');
    source
}

fn generated_protocol_calls(calls: usize, methods: usize) -> String {
    let mut source = String::from(
        "package generated; public class Message { \
         public int read(Protocol input) { int sum = 0;\n",
    );
    for call in 0..calls {
        source.push_str(&format!("sum += input.read{}();\n", call % methods));
    }
    source.push_str("return sum; } }");
    source
}

#[test]
#[ignore = "manual performance probe"]
fn generated_calls_resolve_published_protocol_members() {
    let sizes = std::env::var("HYPERION_PROTOCOL_METHODS")
        .ok()
        .and_then(|value| value.parse().ok())
        .map_or_else(|| vec![128, 256, 512], |methods| vec![methods]);
    for methods in sizes {
        let calls = std::env::var("HYPERION_PROTOCOL_CALLS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(methods * 2);
        let directory = tempfile::tempdir().expect("temporary workspace");
        let protocol_path = directory.path().join("Protocol.java");
        let message_path = directory.path().join("Message.java");
        std::fs::write(&protocol_path, protocol_source(methods)).expect("write protocol Java");
        std::fs::write(&message_path, generated_protocol_calls(calls, methods))
            .expect("write generated Java");
        let index = WorkspaceIndex::new(directory.path().to_path_buf());
        assert_eq!(
            index
                .index_files_parallel_skip_cached(&[protocol_path])
                .files_indexed,
            1
        );
        let before = index.index_phase_snapshot();

        let started = Instant::now();
        let stats = index.index_files_parallel_skip_cached(&[message_path]);
        let elapsed = started.elapsed();
        let after = index.index_phase_snapshot();
        assert_eq!(stats.files_indexed, 1);
        assert_eq!(
            index.find_usages("generated.Protocol.read0()").len(),
            calls.div_ceil(methods)
        );
        eprintln!(
            "protocol_methods={methods} indexed_ms={:.3} symbols_ms={:.3} usages_ms={:.3}",
            elapsed.as_secs_f64() * 1e3,
            (after.symbol_extract_ns[0] - before.symbol_extract_ns[0]) as f64 / 1e6,
            (after.usage_extract_ns[0] - before.usage_extract_ns[0]) as f64 / 1e6,
        );
    }
}

#[test]
#[ignore = "manual performance probe"]
fn generated_calls_scale_with_usage_count() {
    let sizes = std::env::var("HYPERION_GENERATED_CALLS")
        .ok()
        .and_then(|value| value.parse().ok())
        .map_or_else(|| vec![256, 512, 1024], |calls| vec![calls]);
    for calls in sizes {
        let directory = tempfile::tempdir().expect("temporary workspace");
        let path = directory.path().join("Message.java");
        std::fs::write(&path, generated_repeated_calls(calls)).expect("write generated Java");
        let index = WorkspaceIndex::new(directory.path().to_path_buf());

        let started = Instant::now();
        let stats = index.index_files_parallel_skip_cached(&[path]);
        let elapsed = started.elapsed();
        let profile = index.index_phase_snapshot();
        assert_eq!(stats.files_indexed, 1);
        assert_eq!(
            index.find_usages("generated.Message.getField0()").len(),
            calls.div_ceil(32)
        );
        eprintln!(
            "generated_calls={calls} indexed_ms={:.3} symbols_ms={:.3} usages_ms={:.3}",
            elapsed.as_secs_f64() * 1e3,
            profile.symbol_extract_ns[0] as f64 / 1e6,
            profile.usage_extract_ns[0] as f64 / 1e6,
        );
    }
}

#[test]
#[ignore = "manual performance probe"]
fn generated_source_indexing_scales_with_member_count() {
    let sizes = std::env::var("HYPERION_GENERATED_MEMBERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .map_or_else(|| vec![64, 128, 256], |members| vec![members]);
    for members in sizes {
        let directory = tempfile::tempdir().expect("temporary workspace");
        let path = directory.path().join("Message.java");
        std::fs::write(&path, generated_source(members)).expect("write generated Java");
        let index = WorkspaceIndex::new(directory.path().to_path_buf());

        let started = Instant::now();
        let stats = index.index_files_parallel_skip_cached(&[path]);
        let elapsed = started.elapsed();
        let profile = index.index_phase_snapshot();
        assert_eq!(stats.files_indexed, 1);
        assert_eq!(index.find_usages("generated.Message.getField0()").len(), 1);
        assert_eq!(
            index
                .find_usages(&format!("generated.Message.getField{}()", members - 1))
                .len(),
            1
        );
        eprintln!(
            "generated_members={members} indexed_ms={:.3} symbols_ms={:.3} usages_ms={:.3}",
            elapsed.as_secs_f64() * 1e3,
            profile.symbol_extract_ns[0] as f64 / 1e6,
            profile.usage_extract_ns[0] as f64 / 1e6,
        );
    }
}
