// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! High-level formatting API

use crate::config::FormatConfig;
use crate::error::FormatError;
use crate::ir::DocumentBuilder;
use crate::printer::Printer;
use crate::visitors::JavaFormatter;
use hyperion_parser::syntax::SyntaxKind;
use hyperion_parser::{Lexer, Parser, SyntaxNode};
use rowan::TextRange;

/// A text edit representing a change to the source
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// Start offset in the original source (byte offset)
    pub start: usize,
    /// End offset in the original source (byte offset)
    pub end: usize,
    /// New text to replace the range
    pub new_text: String,
}

/// Format a complete Java source file
///
/// # Arguments
/// * `source` - The Java source code to format
/// * `config` - Formatting configuration
///
/// # Returns
/// The formatted source code, or an error if parsing fails
pub fn format_source(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    let root = parse_formatting_root(source)?;

    // Build the document IR from the syntax tree
    let mut builder = DocumentBuilder::new(config.clone());
    let formatter = JavaFormatter::new(&mut builder);
    formatter.format(&root);

    // Print the document IR to a string
    let printer = Printer::new(config.clone());
    let doc = builder.finish();
    let output = printer.print(&doc);

    Ok(output)
}

/// Format a range within the source
///
/// The range will be expanded to cover complete statements/declarations.
/// Only the expanded range is reformatted; the rest of the file is preserved.
///
/// # Arguments
/// * `source` - The Java source code
/// * `range` - The byte range to format
/// * `config` - Formatting configuration
///
/// # Returns
/// The formatted source code (complete file with range formatted), or an error
pub fn format_range(
    source: &str,
    range: std::ops::Range<usize>,
    config: &FormatConfig,
) -> Result<String, FormatError> {
    // Validate range
    if range.start > range.end || range.end > source.len() {
        return Err(FormatError::InvalidRange {
            start: range.start,
            end: range.end,
            source_len: source.len(),
        });
    }

    // Empty range or full file - just format everything
    if range.is_empty() || (range.start == 0 && range.end == source.len()) {
        return format_source(source, config);
    }

    let root = parse_formatting_root(source)?;

    // Convert byte offsets to TextRange
    let start = u32::try_from(range.start).map_err(|_| FormatError::InvalidRange {
        start: range.start,
        end: range.end,
        source_len: source.len(),
    })?;
    let end = u32::try_from(range.end).map_err(|_| FormatError::InvalidRange {
        start: range.start,
        end: range.end,
        source_len: source.len(),
    })?;
    let text_range = TextRange::new(start.into(), end.into());

    // Find the covering element and expand to formattable boundary
    let formattable = find_formattable_node(&root, text_range);

    let Some(target_node) = formattable else {
        // No suitable node found, format the whole file
        return format_source(source, config);
    };

    let node_range = target_node.text_range();

    // Format just this node
    let mut builder = DocumentBuilder::new(config.clone());
    let java_fmt = JavaFormatter::new(&mut builder);
    java_fmt.format(&target_node);

    let printer = Printer::new(config.clone());
    let doc = builder.finish();
    let formatted = printer.print(&doc);

    // Splice the formatted node back into the original source
    let before = &source[..usize::from(node_range.start())];
    let after = &source[usize::from(node_range.end())..];

    // Trim trailing newline from formatted output if splicing mid-file
    let formatted_trimmed = if after.is_empty() {
        formatted
    } else {
        formatted.trim_end_matches('\n').to_string()
    };

    // The node was formatted as if at column 0. Its first line keeps the indentation that
    // already precedes it; every later line gets the indentation of the node's own line,
    // so a nested member's body and closing brace line up with its declaration.
    let start: usize = node_range.start().into();
    let line_start = source[..start].rfind('\n').map_or(0, |p| p + 1);
    let indent: String = source[line_start..start]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let indented: String = formatted_trimmed
        .split_inclusive('\n')
        .enumerate()
        .map(|(index, segment)| {
            if index == 0 || segment.trim_end_matches('\n').is_empty() {
                segment.to_string()
            } else {
                format!("{indent}{segment}")
            }
        })
        .collect();

    Ok(format!("{before}{indented}{after}"))
}

/// Parse a source file only when it is safe to format.
///
/// The parser performs error recovery and can produce a tree for malformed
/// source. That tree is useful to editor features, but it is not safe input to
/// the formatter: visiting it can otherwise emit a partial or corrupted file.
fn parse_formatting_root(source: &str) -> Result<SyntaxNode, FormatError> {
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);

    if !errors.is_empty() {
        return Err(FormatError::ParseError(format!(
            "{} parser diagnostic(s); formatting skipped",
            errors.len()
        )));
    }

    if contains_error_nodes(&root) {
        return Err(FormatError::ParseError(
            "parse tree contains error nodes; formatting skipped".to_string(),
        ));
    }

    Ok(root)
}

fn contains_error_nodes(root: &SyntaxNode) -> bool {
    root.descendants_with_tokens()
        .any(|element| element.kind() == SyntaxKind::ERROR)
}

/// Find a formattable node that covers the given range
///
/// Walks up from the covering element to find a statement or declaration
/// that can be independently formatted.
fn find_formattable_node(root: &SyntaxNode, range: TextRange) -> Option<SyntaxNode> {
    use rowan::SyntaxElement;

    // Find the deepest element covering the range
    let covering = root.covering_element(range);

    // Get the node (if it's a token, get its parent)
    let mut node = match covering {
        SyntaxElement::Node(n) => n,
        SyntaxElement::Token(t) => t.parent()?,
    };

    // Walk up to find a formattable boundary
    loop {
        if is_formattable_node(&node) {
            return Some(node);
        }

        node = node.parent()?;

        // Don't go past the source file
        if node.kind() == SyntaxKind::SOURCE_FILE {
            return None;
        }
    }
}

/// Check if a node is suitable for independent formatting
fn is_formattable_node(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        // Declarations
        SyntaxKind::CLASS_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::ENUM_DECL
            | SyntaxKind::RECORD_DECL
            | SyntaxKind::ANNOTATION_DECL
            | SyntaxKind::METHOD_DECL
            | SyntaxKind::CONSTRUCTOR_DECL
            | SyntaxKind::FIELD_DECL
            // Statements
            | SyntaxKind::BLOCK_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::DO_WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOREACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::SWITCH_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::THROW_STMT
            | SyntaxKind::EXPR_STMT
    )
}

/// Compute text edits for formatting
///
/// This is useful for LSP integration where we need to return edits
/// rather than the complete formatted source.
///
/// # Arguments
/// * `source` - The Java source code to format
/// * `config` - Formatting configuration
///
/// # Returns
/// A list of text edits to apply, or an error
pub fn format_to_edits(source: &str, config: &FormatConfig) -> Result<Vec<TextEdit>, FormatError> {
    let formatted = format_source(source, config)?;

    if source == formatted {
        return Ok(vec![]);
    }

    Ok(compute_minimal_edits(source, &formatted))
}

/// Compute a compact text edit by trimming common line prefix/suffix.
///
/// This uses linear memory/time relative to line count and avoids quadratic
/// diff matrices on large files.
// `suspicious_operation_groupings` flags the suffix comparison
// `orig_lines[orig_suffix - 1] == fmt_lines[fmt_suffix - 1]` and suggests using
// `orig_suffix` on both sides. That "fix" is wrong: `orig_suffix` and `fmt_suffix`
// are independent cursors into two different arrays, so each side must use its own.
#[allow(clippy::suspicious_operation_groupings)]
fn compute_minimal_edits(original: &str, formatted: &str) -> Vec<TextEdit> {
    if original == formatted {
        return vec![];
    }

    let orig_lines: Vec<&str> = original.split_inclusive('\n').collect();
    let fmt_lines: Vec<&str> = formatted.split_inclusive('\n').collect();

    let mut prefix_lines = 0usize;
    while prefix_lines < orig_lines.len()
        && prefix_lines < fmt_lines.len()
        && orig_lines[prefix_lines] == fmt_lines[prefix_lines]
    {
        prefix_lines += 1;
    }

    let mut orig_suffix = orig_lines.len();
    let mut fmt_suffix = fmt_lines.len();
    while orig_suffix > prefix_lines
        && fmt_suffix > prefix_lines
        && orig_lines[orig_suffix - 1] == fmt_lines[fmt_suffix - 1]
    {
        orig_suffix -= 1;
        fmt_suffix -= 1;
    }

    let start: usize = orig_lines
        .iter()
        .take(prefix_lines)
        .map(|line| line.len())
        .sum();
    let end = start
        + orig_lines[prefix_lines..orig_suffix]
            .iter()
            .map(|line| line.len())
            .sum::<usize>();
    let new_text = fmt_lines[prefix_lines..fmt_suffix].concat();

    vec![TextEdit {
        start,
        end,
        new_text,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_empty_source() {
        let result = format_source("", &FormatConfig::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_format_simple_class() {
        let source = "class Foo {}";
        let result = format_source(source, &FormatConfig::default());
        assert!(result.is_ok());
    }

    #[test]
    fn formatting_bails_when_parse_tree_contains_error_nodes() {
        let source = "class Foo { @@@ }";
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        assert!(
            contains_error_nodes(&root),
            "fixture must produce an error node in the parse tree"
        );
        assert!(matches!(
            format_source(source, &FormatConfig::default()),
            Err(FormatError::ParseError(_))
        ));
    }

    #[test]
    fn test_invalid_range() {
        let source = "class Foo {}";
        let result = format_range(source, 100..200, &FormatConfig::default());
        assert!(matches!(result, Err(FormatError::InvalidRange { .. })));
    }

    #[test]
    fn test_range_format_method() {
        let source = r"class Foo {
    void bar() {return;}
    void baz() { return; }
}";
        // Format just inside the bar method body (cursor on "return")
        let return_pos = source.find("return;").unwrap();

        let result = format_range(source, return_pos..return_pos + 7, &FormatConfig::default());
        assert!(result.is_ok());

        let formatted = result.unwrap();
        // The baz method should be unchanged (preserved from original)
        assert!(
            formatted.contains("void baz() { return; }"),
            "baz should be unchanged: {formatted}"
        );
    }

    #[test]
    fn test_range_format_empty_range_formats_all() {
        let source = "class Foo {}";
        let result = format_range(source, 0..0, &FormatConfig::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_range_format_full_file() {
        let source = "class Foo {}";
        let result = format_range(source, 0..source.len(), &FormatConfig::default());
        assert!(result.is_ok());
    }
}
