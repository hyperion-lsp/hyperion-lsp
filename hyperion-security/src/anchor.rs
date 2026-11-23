// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Where a finding points.
//!
//! A check reports the syntax node it reasoned about. For a class with no
//! authorization, or a method that never audits, that node is the whole
//! declaration. `Finding::source_location` keeps that extent, because reports,
//! suppression comments and deduplication depend on it. An editor draws a
//! diagnostic over its whole range, though, so a declaration-wide range
//! underlines every line of the class. The anchor is the short range an
//! editor should mark instead: the declaration's name, or for any other
//! finding that runs over several lines, its first line.

use crate::finding::{Finding, Location};
use hyperion_parser::syntax::{
    AnnotationDecl, AstNode, ClassDecl, ConstructorDecl, EnumConstant, EnumDecl, InterfaceDecl,
    MethodDecl, RecordDecl,
};
use hyperion_parser::{SyntaxKind, SyntaxNode};
use rowan::{TextRange, TextSize};

/// Give every finding that runs over several lines an anchor on one line:
/// the declaration's name when the finding covers a whole declaration,
/// otherwise the first line of the finding. Findings on one line keep
/// pointing at their own range.
pub(crate) fn anchor_findings(root: &SyntaxNode, source: &str, findings: &mut [Finding]) {
    let mut lines: Option<LineTable<'_>> = None;
    for finding in findings {
        let location = &finding.source_location;
        if finding.anchor.is_some() || location.end_line <= location.line {
            continue;
        }
        let lines = lines.get_or_insert_with(|| LineTable::new(source));
        let (Some(start), Some(end)) = (
            lines.offset(location.line, location.column),
            lines.offset(location.end_line, location.end_column),
        ) else {
            continue;
        };
        if start >= end || end > root.text_range().end() {
            continue;
        }
        let name = declaration_name_at(root, TextRange::new(start, end))
            .unwrap_or_else(|| lines.first_line(start));
        let (line, column) = lines.line_col(name.start());
        let (end_line, end_column) = lines.line_col(name.end());
        finding.anchor = Some(Location {
            file: location.file.clone(),
            line,
            column,
            end_line,
            end_column,
        });
    }
}

/// The name of the declaration that occupies exactly `range`, if there is one.
fn declaration_name_at(root: &SyntaxNode, range: TextRange) -> Option<TextRange> {
    let element = root.covering_element(range);
    let node = element.as_node().cloned().or_else(|| element.parent())?;
    node.ancestors()
        .take_while(|ancestor| ancestor.text_range() == range)
        .find_map(|ancestor| declaration_name(&ancestor))
}

/// The name token's range, for the node kinds that declare a named thing.
#[must_use]
pub fn declaration_name(node: &SyntaxNode) -> Option<TextRange> {
    let name = match node.kind() {
        SyntaxKind::CLASS_DECL => ClassDecl::cast(node.clone())?.name(),
        SyntaxKind::INTERFACE_DECL => InterfaceDecl::cast(node.clone())?.name(),
        SyntaxKind::ENUM_DECL => EnumDecl::cast(node.clone())?.name(),
        SyntaxKind::ANNOTATION_DECL => AnnotationDecl::cast(node.clone())?.name(),
        SyntaxKind::RECORD_DECL => RecordDecl::cast(node.clone())?.name(),
        SyntaxKind::METHOD_DECL => MethodDecl::cast(node.clone())?.name(),
        SyntaxKind::CONSTRUCTOR_DECL => ConstructorDecl::cast(node.clone())?.name(),
        SyntaxKind::ENUM_CONSTANT => EnumConstant::cast(node.clone())?.name(),
        _ => None,
    }?;
    Some(name.text_range())
}

/// Converts between byte offsets and the 1-based line and character columns
/// of a [`Location`], with the line-break rules of
/// `scanner::utilities::offset_to_line_col` (`\n`, `\r\n` and a lone `\r`).
struct LineTable<'a> {
    source: &'a str,
    /// Byte offset of the first character of each line.
    starts: Vec<usize>,
}

impl<'a> LineTable<'a> {
    fn new(source: &'a str) -> Self {
        let bytes = source.as_bytes();
        let mut starts = vec![0];
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'\r' => {
                    if bytes.get(index + 1) == Some(&b'\n') {
                        index += 1;
                    }
                    starts.push(index + 1);
                }
                b'\n' => starts.push(index + 1),
                _ => {}
            }
            index += 1;
        }
        Self { source, starts }
    }

    fn offset(&self, line: u32, column: u32) -> Option<TextSize> {
        let start = *self
            .starts
            .get(usize::try_from(line.checked_sub(1)?).ok()?)?;
        let rest = &self.source[start..];
        let chars = usize::try_from(column.checked_sub(1)?).ok()?;
        let within = rest
            .char_indices()
            .nth(chars)
            .map_or(rest.len(), |(index, _)| index);
        TextSize::try_from(start + within).ok()
    }

    /// From `start` to the last non-blank character of its line.
    fn first_line(&self, start: TextSize) -> TextRange {
        let from = usize::from(start);
        let rest = &self.source[from..];
        let line = rest.split(['\r', '\n']).next().unwrap_or(rest).trim_end();
        let end = TextSize::try_from(from + line.len()).unwrap_or(start);
        TextRange::new(start, end)
    }

    fn line_col(&self, offset: TextSize) -> (u32, u32) {
        let offset = usize::from(offset);
        let line = self.starts.partition_point(|&start| start <= offset) - 1;
        let column = self.source[self.starts[line]..offset].chars().count();
        (
            u32::try_from(line + 1).unwrap_or(u32::MAX),
            u32::try_from(column + 1).unwrap_or(u32::MAX),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::VulnType;
    use hyperion_parser::{Lexer, Parser};
    use std::path::PathBuf;

    fn parse(source: &str) -> SyntaxNode {
        let (green, _) = Parser::parse(Lexer::new(source).collect());
        SyntaxNode::new_root(green)
    }

    fn finding_on(node: &SyntaxNode, source: &str) -> Finding {
        let location =
            crate::scanner::utilities::location_from_node(node, source, &PathBuf::from("A.java"));
        Finding::new(
            VulnType::SqlInjection,
            "test".to_string(),
            location,
            String::new(),
        )
    }

    fn anchored(source: &str, kind: SyntaxKind) -> (Finding, String) {
        let root = parse(source);
        let node = root
            .descendants()
            .find(|node| node.kind() == kind)
            .expect("the fixture has a node of this kind");
        let mut findings = [finding_on(&node, source)];
        anchor_findings(&root, source, &mut findings);
        let [finding] = findings;
        let lines = LineTable::new(source);
        let location = finding.display_location();
        let start = lines.offset(location.line, location.column).unwrap();
        let end = lines
            .offset(location.end_line, location.end_column)
            .unwrap();
        let text = source[usize::from(start)..usize::from(end)].to_string();
        (finding, text)
    }

    const CONTROLLER: &str = "package a;\r\n\r\n/** Docs. */\r\n@RestController\r\npublic class OrderController {\r\n    @GetMapping\r\n    public Order find(\r\n            long id) {\r\n        return repo\r\n            .find(id);\r\n    }\r\n}\r\n";

    #[test]
    fn a_class_wide_finding_points_at_the_class_name() {
        let (finding, text) = anchored(CONTROLLER, SyntaxKind::CLASS_DECL);
        assert_eq!(text, "OrderController");
        let anchor = finding.anchor.as_ref().expect("anchored");
        assert_eq!((anchor.line, anchor.end_line), (5, 5));
        // The finding itself still covers the class.
        assert!(finding.source_location.end_line > finding.source_location.line);
    }

    #[test]
    fn a_method_wide_finding_points_at_the_method_name() {
        let (_, text) = anchored(CONTROLLER, SyntaxKind::METHOD_DECL);
        assert_eq!(text, "find");
    }

    #[test]
    fn any_other_finding_over_several_lines_points_at_its_first_line() {
        let (finding, text) = anchored(CONTROLLER, SyntaxKind::RETURN_STMT);
        assert_eq!(text, "return repo");
        let anchor = finding.anchor.as_ref().expect("anchored");
        assert_eq!(anchor.line, anchor.end_line);
    }

    #[test]
    fn a_finding_on_one_line_keeps_its_own_range() {
        let source = "class A { void f() { run(x); } }";
        let (finding, text) = anchored(source, SyntaxKind::METHOD_CALL_EXPR);
        assert!(finding.anchor.is_none());
        assert_eq!(text, "run(x)");
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let source = "class A {\n  String é = \"ü\"; void run(\n  ) {}\n}";
        let (_, text) = anchored(source, SyntaxKind::METHOD_DECL);
        assert_eq!(text, "run");
    }
}
