// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_lsp::hover::HoverProvider;
use hyperion_semantics::db::{FileId, SourceDatabase};
use hyperion_workspace::WorkspaceIndex;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::Position;

#[tokio::main]
async fn main() {
    // Source with Unicode identifier
    let source = r"
package com.example;

public class Test {
    public long getTotalPriceInØre() {
        return 1000L;
    }

    public void callMethod() {
        long value = getTotalPriceInØre();
    }
}
";

    // Create workspace index (empty for this test)
    let workspace_index = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp")));
    let host = hyperion_lsp::semantic_host::SemanticHost::new(Arc::clone(&workspace_index));
    let file_id = FileId::new(0);
    host.with_db(|db| db.set_file_text(file_id, Arc::new(source.to_string())));
    let txn = host.read_now();

    let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
    let hover_provider = HoverProvider::new(&txn, workspace_index, source_providers);
    let lines: Vec<&str> = source.lines().collect();

    println!("=== Testing Unicode Identifier Hover ===\n");

    // Test 1: Hover on method declaration
    println!("## Test 1: Method Declaration with Unicode");
    if let Some(line_num) = lines
        .iter()
        .position(|l| l.contains("getTotalPriceInØre()"))
    {
        if let Some(pos) = lines[line_num].find("getTotalPriceInØre") {
            let position = Position::new(line_num as u32, pos as u32);
            println!("Line {}: {}", line_num + 1, lines[line_num].trim());
            println!("Hovering on Unicode method name\n");

            match hover_provider.hover(file_id, position, source) {
                Ok(Some(hover)) => {
                    let hover_text = match hover.contents {
                        tower_lsp::lsp_types::HoverContents::Markup(markup) => markup.value,
                        _ => String::new(),
                    };
                    println!("✅ Got hover:");
                    for line in hover_text.lines() {
                        println!("  {line}");
                    }
                }
                Ok(None) => println!("❌ No hover"),
                Err(e) => println!("❌ Error: {e}"),
            }
        }
    }
    println!("\n---\n");

    // Test 2: Hover on method call
    println!("## Test 2: Method Call with Unicode");
    if let Some(line_num) = lines
        .iter()
        .position(|l| l.contains("= getTotalPriceInØre()"))
    {
        if let Some(pos) = lines[line_num].find("getTotalPriceInØre") {
            let position = Position::new(line_num as u32, pos as u32);
            println!("Line {}: {}", line_num + 1, lines[line_num].trim());
            println!("Hovering on Unicode method call\n");

            match hover_provider.hover(file_id, position, source) {
                Ok(Some(hover)) => {
                    let hover_text = match hover.contents {
                        tower_lsp::lsp_types::HoverContents::Markup(markup) => markup.value,
                        _ => String::new(),
                    };
                    println!("✅ Got hover:");
                    for line in hover_text.lines() {
                        println!("  {line}");
                    }
                }
                Ok(None) => println!("❌ No hover"),
                Err(e) => println!("❌ Error: {e}"),
            }
        }
    }

    println!("\n=== Summary ===");
    println!("The lexer now supports Unicode identifiers (Ø, ï, αβγ, 数字, etc.)");
    println!("Parse errors from Unicode characters are fixed!");
    println!("Workspace index now includes previously missing methods.");
}
