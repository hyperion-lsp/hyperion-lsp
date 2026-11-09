// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::path::Path;

use tower_lsp::lsp_types::{
    DocumentFormattingParams, DocumentRangeFormattingParams, Range, TextEdit, Url,
};
use tracing::{debug, warn};

use crate::server::Backend;

impl Backend {
    pub(crate) async fn handle_formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Option<Vec<TextEdit>> {
        // Cancellation checkpoint: Allow tower-lsp to abort this request if $/cancelRequest was sent
        tokio::task::yield_now().await;

        let uri = params.text_document.uri;
        debug!("Document formatting request for {}", uri);

        let (language_id, source_text) = if let Some(state) = self
            .documents
            .by_uri
            .get(uri.as_str())
            .filter(|state| !state.desynchronized)
        {
            (state.language_id.clone(), state.text.clone())
        } else {
            warn!("Formatting requested for unopened document: {}", uri);
            return None;
        };

        match format_document(&uri, &language_id, &source_text, None) {
            Ok(Some(edits)) => {
                debug!("Formatting successful, {} edits", edits.len());
                Some(edits)
            }
            Ok(None) => {
                debug!("Skipping formatting for non-Java document {}", uri);
                None
            }
            Err(e) => {
                warn!("Formatting failed: {}", e);
                None
            }
        }
    }

    pub(crate) fn handle_range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Option<Vec<TextEdit>> {
        let uri = params.text_document.uri;
        let range = params.range;
        debug!("Range formatting request for {} at {:?}", uri, range);

        let (language_id, source_text) = if let Some(state) = self
            .documents
            .by_uri
            .get(uri.as_str())
            .filter(|state| !state.desynchronized)
        {
            (state.language_id.clone(), state.text.clone())
        } else {
            warn!("Range formatting requested for unopened document: {}", uri);
            return None;
        };

        match format_document(&uri, &language_id, &source_text, Some(range)) {
            Ok(Some(edits)) => {
                debug!("Range formatting successful, {} edits", edits.len());
                Some(edits)
            }
            Ok(None) => {
                debug!("Skipping range formatting for non-Java document {}", uri);
                None
            }
            Err(e) => {
                warn!("Range formatting failed: {}", e);
                None
            }
        }
    }
}

/// Formats a document only when it is Java.
///
/// The language ID supplied with `didOpen` is authoritative. Older or non-conforming clients
/// can leave it empty, in which case a `.java` file extension is accepted as a fallback.
fn format_document(
    uri: &Url,
    language_id: &str,
    source_text: &str,
    range: Option<Range>,
) -> Result<Option<Vec<TextEdit>>, String> {
    if !is_java_document(uri, language_id) {
        return Ok(None);
    }

    crate::formatting::format_source(source_text, range).map(Some)
}

fn is_java_document(uri: &Url, language_id: &str) -> bool {
    if !language_id.is_empty() {
        return language_id == "java";
    }

    Path::new(uri.path())
        .extension()
        .is_some_and(|extension| extension == "java")
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::{Position, Range, Url};

    use super::format_document;

    #[test]
    fn kotlin_document_formatting_returns_no_edits_and_preserves_content() {
        let uri = Url::parse("file:///workspace/Example.kt").expect("valid URI");
        let source = "class Example{fun main()=println(\"unchanged\")}";

        let edits = format_document(&uri, "kotlin", source, None).expect("format request succeeds");

        assert!(edits.is_none(), "Kotlin formatting must return no edits");
        assert_eq!(source, "class Example{fun main()=println(\"unchanged\")}");
    }

    #[test]
    fn java_document_formatting_still_returns_edits() {
        let uri = Url::parse("file:///workspace/Example.java").expect("valid URI");

        let edits = format_document(&uri, "java", "class Example{int value;}", None)
            .expect("Java format request succeeds");

        assert!(edits.is_some_and(|edits| !edits.is_empty()));
    }

    #[test]
    fn kotlin_range_formatting_returns_no_edits() {
        let uri = Url::parse("file:///workspace/Example.kt").expect("valid URI");
        let range = Range {
            start: Position::new(0, 0),
            end: Position::new(0, 12),
        };

        let edits = format_document(
            &uri,
            "kotlin",
            "class Example{fun main()=println(\"unchanged\")}",
            Some(range),
        )
        .expect("range format request succeeds");

        assert!(
            edits.is_none(),
            "Kotlin range formatting must return no edits"
        );
    }

    #[test]
    fn missing_language_id_falls_back_to_java_extension() {
        let uri = Url::parse("file:///workspace/Example.java").expect("valid URI");

        let edits = format_document(&uri, "", "class Example{int value;}", None)
            .expect("Java extension fallback succeeds");

        assert!(edits.is_some_and(|edits| !edits.is_empty()));
    }

    #[test]
    fn language_id_overrides_java_extension() {
        let uri = Url::parse("file:///workspace/Example.java").expect("valid URI");

        let edits = format_document(&uri, "kotlin", "class Example{int value;}", None)
            .expect("format request succeeds");

        assert!(edits.is_none(), "language ID must override the extension");
    }
}
