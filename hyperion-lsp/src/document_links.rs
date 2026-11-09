// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Document Links Provider
//!
//! Provides clickable links in source code for URLs found in comments.
//! Supports:
//! - http:// and https:// URLs in line comments
//! - http:// and https:// URLs in block comments
//! - http:// and https:// URLs in doc comments

use hyperion_parser::syntax::SyntaxKind;
use hyperion_semantics::db::{FileId, ParseDatabase, SourceDatabase};
use regex::Regex;
use tower_lsp::lsp_types::{DocumentLink, Range, Url};
use tracing::debug;

use crate::position_utils::LineIndex;
use crate::semantic_host::ReadTxn;

/// Regex pattern for matching URLs in comments
/// Matches http:// and https:// URLs, stopping at whitespace and common delimiters
const URL_PATTERN: &str = r"https?://[^\s\)\]\}>*]+";

/// Provides document link information (clickable URLs in comments)
pub struct DocumentLinksProvider {
    url_regex: Regex,
}

impl Default for DocumentLinksProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentLinksProvider {
    /// Creates a new DocumentLinksProvider
    pub fn new() -> Self {
        Self {
            url_regex: Regex::new(URL_PATTERN).expect("URL regex should compile"),
        }
    }

    /// Get document links for a file
    pub fn document_links(
        &self,
        txn: &ReadTxn,
        file_id: FileId,
        source_text: &str,
    ) -> Result<Vec<DocumentLink>, String> {
        debug!("Computing document links for file {:?}", file_id);

        let root = {
            if txn.file_text(file_id).as_str() != source_text {
                return Ok(Vec::new());
            }
            txn.parse_file(file_id).syntax()
        };

        let lines = LineIndex::new(source_text);
        let mut links = Vec::new();
        for token in root
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
        {
            if !matches!(
                token.kind(),
                SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT
            ) {
                continue;
            }
            let token_start = usize::from(token.text_range().start());
            for matched in self.url_regex.find_iter(token.text()) {
                let url_text = matched.as_str();
                if let Ok(target) = Url::parse(url_text) {
                    links.push(DocumentLink {
                        range: Range::new(
                            lines.offset_to_position(source_text, token_start + matched.start()),
                            lines.offset_to_position(source_text, token_start + matched.end()),
                        ),
                        target: Some(target),
                        tooltip: Some(format!("Open {url_text}")),
                        data: None,
                    });
                }
            }
        }
        debug!("Generated {} document links", links.len());
        Ok(links)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_host::SemanticHost;
    use hyperion_semantics::db::Language;
    use std::sync::Arc;

    fn create_test_db() -> Arc<SemanticHost> {
        SemanticHost::for_tests()
    }

    fn setup_file(host: &Arc<SemanticHost>, source: &str) -> FileId {
        let file_id = FileId::new(1);
        host.with_db(|db| {
            db.register_file(file_id, Arc::new(source.to_string()), Language::Java);
        });
        file_id
    }

    #[test]
    fn mismatched_parse_source_does_not_produce_ranges() {
        let host = create_test_db();
        let file_id = setup_file(&host, "class Example {\n // https://example.com/docs\n}");
        let source = "class Example {\n // https://example.com/docs\n}";
        let provider = DocumentLinksProvider::new();
        let txn = host.read_now();
        assert!(!provider
            .document_links(&txn, file_id, source)
            .unwrap()
            .is_empty());
        assert!(provider
            .document_links(&txn, file_id, &format!("\n{source}"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_url_in_line_comment() {
        let host = create_test_db();
        let source = r"
public class Test {
    // See https://example.com/docs for more info
    void test() {}
}
";
        let file_id = setup_file(&host, source);
        let provider = DocumentLinksProvider::new();

        let links = provider
            .document_links(&host.read_now(), file_id, source)
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().unwrap().as_str(),
            "https://example.com/docs"
        );
    }

    #[test]
    fn test_url_in_block_comment() {
        let host = create_test_db();
        let source = r"
public class Test {
    /* Documentation at http://docs.example.com/api */
    void test() {}
}
";
        let file_id = setup_file(&host, source);
        let provider = DocumentLinksProvider::new();

        let links = provider
            .document_links(&host.read_now(), file_id, source)
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().unwrap().as_str(),
            "http://docs.example.com/api"
        );
    }

    #[test]
    fn test_url_in_doc_comment() {
        let host = create_test_db();
        let source = r"
/**
 * See https://github.com/user/repo for the source code.
 */
public class Test {}
";
        let file_id = setup_file(&host, source);
        let provider = DocumentLinksProvider::new();

        let links = provider
            .document_links(&host.read_now(), file_id, source)
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().unwrap().as_str(),
            "https://github.com/user/repo"
        );
    }

    #[test]
    fn test_multiple_urls() {
        let host = create_test_db();
        let source = r"
public class Test {
    // First: https://example.com second: http://test.org
    void test() {}
}
";
        let file_id = setup_file(&host, source);
        let provider = DocumentLinksProvider::new();

        let links = provider
            .document_links(&host.read_now(), file_id, source)
            .unwrap();
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn test_no_urls() {
        let host = create_test_db();
        let source = r"
public class Test {
    // Just a regular comment
    void test() {}
}
";
        let file_id = setup_file(&host, source);
        let provider = DocumentLinksProvider::new();

        let links = provider
            .document_links(&host.read_now(), file_id, source)
            .unwrap();
        assert!(links.is_empty());
    }

    #[test]
    fn test_url_not_in_code() {
        let host = create_test_db();
        let source = r#"
public class Test {
    String url = "https://should-not-be-linked.com";
}
"#;
        let file_id = setup_file(&host, source);
        let provider = DocumentLinksProvider::new();

        // URLs in strings should NOT be linked (only in comments)
        let links = provider
            .document_links(&host.read_now(), file_id, source)
            .unwrap();
        assert!(links.is_empty());
    }
}
