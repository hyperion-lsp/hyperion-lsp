// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Javadoc HTML parser for extracting documentation from javadoc JARs.
//!
//! This module reads bounded HTML entries from javadoc JARs. It currently
//! extracts only the first class-description block and converts a small set of
//! HTML tags to Markdown. Method and field maps are intentionally empty until
//! a structured Javadoc HTML parser is implemented.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;
use tracing::{debug, warn};
use zip::ZipArchive;

const MAX_JAVADOC_HTML_BYTES: usize = 8 * 1024 * 1024;
const MAX_PREALLOCATED_CLASSES: usize = 10_000;
const MAX_ARCHIVE_ENTRIES: usize = 500_000;
const MAX_HTML_ENTRIES: usize = 100_000;
const MAX_TOTAL_HTML_BYTES: usize = 512 * 1024 * 1024;
const MAX_TOTAL_DOCUMENT_BYTES: usize = 128 * 1024 * 1024;
const MAX_ARCHIVE_PATH_BYTES: usize = 4 * 1024;

/// Documentation extracted from a javadoc JAR
#[derive(Debug, Clone)]
pub struct JavadocExtraction {
    /// Map from fully qualified class name to class documentation
    pub class_docs: HashMap<String, String>,

    /// Map from fully qualified method name (class.method) to method documentation
    pub method_docs: HashMap<String, String>,

    /// Map from fully qualified field name (class.field) to field documentation
    pub field_docs: HashMap<String, String>,
}

impl JavadocExtraction {
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// Create a new JavadocExtraction with pre-allocated capacity
    /// Typical javadoc JARs have hundreds of classes, thousands of methods
    pub fn with_capacity(estimated_classes: usize) -> Self {
        let capacity = if estimated_classes == 0 {
            100 // Default estimate for small libraries
        } else {
            estimated_classes.min(MAX_PREALLOCATED_CLASSES)
        };

        Self {
            class_docs: HashMap::with_capacity(capacity),
            // Grow these on demand instead of speculatively reserving ten
            // entries for every estimated class in an untrusted archive.
            method_docs: HashMap::with_capacity(capacity.saturating_mul(2)),
            field_docs: HashMap::with_capacity(capacity),
        }
    }
}

impl Default for JavadocExtraction {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a javadoc JAR and extract documentation
pub fn parse_javadoc_jar<P: AsRef<Path>>(jar_path: P) -> Result<JavadocExtraction, String> {
    let file = std::fs::File::open(jar_path.as_ref())
        .map_err(|e| format!("Failed to open javadoc JAR: {}", e))?;

    parse_javadoc_file(file)
}

pub fn parse_javadoc_snapshot(
    snapshot: crate::ArchiveSnapshot,
) -> Result<JavadocExtraction, String> {
    parse_javadoc_file(snapshot.into_file())
}

fn parse_javadoc_file(file: std::fs::File) -> Result<JavadocExtraction, String> {
    let mut archive =
        ZipArchive::new(file).map_err(|e| format!("Failed to read javadoc JAR: {}", e))?;

    // Estimate number of classes by counting HTML files (avoid full scan)
    // Sample first 100 entries to estimate total
    let total_files = archive.len();
    if total_files > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "javadoc archive contains {total_files} entries (maximum {MAX_ARCHIVE_ENTRIES})"
        ));
    }
    let sample_size = total_files.min(100);
    let mut html_count = 0usize;

    for i in 0..sample_size {
        if let Ok(file) = archive.by_index(i) {
            let name = file
                .name()
                .map_err(|e| format!("Invalid JAR entry name: {e}"))?;
            if name.ends_with(".html") && !name.contains("package-") && !name.contains("index-") {
                html_count += 1;
            }
        }
    }

    // Extrapolate from sample
    let estimated_classes = if sample_size < total_files {
        html_count.saturating_mul(total_files) / sample_size
    } else {
        html_count
    };

    let mut extraction = JavadocExtraction::with_capacity(estimated_classes);
    let mut html_entries = 0_usize;
    let mut total_html_bytes = 0_usize;
    let mut total_document_bytes = 0_usize;

    // Iterate through all files in the JAR
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read file from JAR: {}", e))?;

        let name = file
            .name()
            .map_err(|e| format!("Invalid JAR entry name: {e}"))?
            .into_owned();
        if name.len() > MAX_ARCHIVE_PATH_BYTES {
            continue;
        }

        // We're interested in HTML files that represent class documentation
        if name.ends_with(".html") && !name.contains("package-") && !name.contains("index-") {
            html_entries += 1;
            if html_entries > MAX_HTML_ENTRIES {
                return Err(format!(
                    "javadoc archive contains more than {MAX_HTML_ENTRIES} HTML entries"
                ));
            }
            // Read the HTML content
            let declared_size = file.size();
            let html_content = match read_html(&mut file, declared_size) {
                Ok(content) => content,
                Err(error) => {
                    warn!("Failed to read HTML file {}: {}", name, error);
                    continue;
                }
            };
            total_html_bytes = total_html_bytes
                .checked_add(html_content.len())
                .ok_or_else(|| "javadoc expanded-byte count overflowed".to_string())?;
            if total_html_bytes > MAX_TOTAL_HTML_BYTES {
                return Err(format!(
                    "javadoc HTML expands beyond {MAX_TOTAL_HTML_BYTES} total bytes"
                ));
            }

            // Extract the fully qualified class name from the file path
            // e.g., "java/util/ArrayList.html" -> "java.util.ArrayList"
            let class_name = name.trim_end_matches(".html").replace('/', ".");

            // Parse the HTML and extract documentation
            if let Some((class_doc, method_docs, field_docs)) =
                parse_class_html(&html_content, &class_name)
            {
                if !class_doc.is_empty() {
                    retain(&mut total_document_bytes, &class_name, &class_doc)?;
                    extraction.class_docs.insert(class_name.clone(), class_doc);
                }

                for (method_name, doc) in method_docs {
                    let full_name = format!("{}.{}", class_name, method_name);
                    retain(&mut total_document_bytes, &full_name, &doc)?;
                    extraction.method_docs.insert(full_name, doc);
                }

                for (field_name, doc) in field_docs {
                    let full_name = format!("{}.{}", class_name, field_name);
                    retain(&mut total_document_bytes, &full_name, &doc)?;
                    extraction.field_docs.insert(full_name, doc);
                }
            }
        }
    }

    debug!(
        "Extracted {} class docs, {} method docs, {} field docs",
        extraction.class_docs.len(),
        extraction.method_docs.len(),
        extraction.field_docs.len()
    );

    Ok(extraction)
}

/// Adds a stored doc and its key to the retained-byte count, and fails once the count passes
/// `MAX_TOTAL_DOCUMENT_BYTES`.
fn retain(total: &mut usize, key: &str, doc: &str) -> Result<(), String> {
    *total = total
        .checked_add(key.len())
        .and_then(|bytes| bytes.checked_add(doc.len()))
        .ok_or_else(|| "javadoc retained-byte count overflowed".to_string())?;
    if *total > MAX_TOTAL_DOCUMENT_BYTES {
        return Err(format!(
            "javadoc output exceeds {MAX_TOTAL_DOCUMENT_BYTES} retained bytes"
        ));
    }
    Ok(())
}

fn read_html(reader: impl Read, declared_size: u64) -> Result<String, String> {
    if declared_size > MAX_JAVADOC_HTML_BYTES as u64 {
        return Err(format!(
            "entry is {declared_size} bytes (maximum {MAX_JAVADOC_HTML_BYTES})"
        ));
    }
    let mut bytes = Vec::with_capacity(declared_size as usize);
    reader
        .take(MAX_JAVADOC_HTML_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_JAVADOC_HTML_BYTES {
        return Err(format!(
            "entry expanded beyond {MAX_JAVADOC_HTML_BYTES} bytes"
        ));
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

/// Parsed class documentation: (class_doc, method_docs, field_docs)
type ParsedClassDocs = (String, HashMap<String, String>, HashMap<String, String>);

/// Parse a single class HTML file and extract documentation
fn parse_class_html(html: &str, _class_name: &str) -> Option<ParsedClassDocs> {
    let class_doc = extract_class_description(html);
    let (method_docs, field_docs) = extract_member_docs(html);

    Some((class_doc, method_docs, field_docs))
}

/// Extract the class description from HTML (simplified implementation)
fn extract_class_description(html: &str) -> String {
    // Look for common javadoc patterns
    // This is a simplified implementation - a full version would use a proper HTML parser

    // Try to find the class description block, then its block tags (@since, @deprecated, ...)
    let section = class_description_section(html);
    let description = match section.find("<div class=\"block\">") {
        Some(start) => match section[start..].find("</div>") {
            Some(end_pos) => html_to_markdown_simple(&section[start + 19..start + end_pos]),
            None => String::new(),
        },
        None => String::new(),
    };
    join_paragraphs(description, notes_markdown(section))
}

/// The part of a class page that documents the class itself. It starts at the class
/// description (`section.class-description` since JDK 16, `div.description` before) and
/// ends where the member summary or member details start, whose blocks and block tags
/// belong to members.
fn class_description_section(html: &str) -> &str {
    const STARTS: [&str; 2] = ["class=\"class-description\"", "class=\"description\""];
    const ENDS: [&str; 3] = ["class=\"summary\"", "class=\"details\"", "class=\"detail\""];
    let start = STARTS
        .iter()
        .filter_map(|marker| html.find(marker))
        .min()
        .unwrap_or(0);
    let end = ENDS
        .iter()
        .filter_map(|marker| html[start..].find(marker))
        .min()
        .map_or(html.len(), |offset| start + offset);
    &html[start..end]
}

/// Joins two Markdown parts with a blank line between them, when both are present.
fn join_paragraphs(first: String, second: String) -> String {
    match (first.is_empty(), second.is_empty()) {
        (_, true) => first,
        (true, false) => second,
        (false, false) => format!("{first}\n\n{second}"),
    }
}

/// The block tags of a page or member section, such as `@since`, `@deprecated`, `@param` and
/// `@return`. Javadoc writes each as a `<dt>` label and a `<dd>` text in a `dl.notes` list.
/// They become Markdown lines, each a bold label followed by its text.
fn notes_markdown(html: &str) -> String {
    const NOTES: &str = "<dl class=\"notes\">";
    let Some(start) = html.find(NOTES).map(|offset| offset + NOTES.len()) else {
        return String::new();
    };
    let end = html[start..]
        .find("</dl>")
        .map_or(html.len(), |offset| start + offset);
    let list = html[start..end]
        .replace("<dt>", "\n\n**")
        .replace("</dt>", "** ")
        .replace("<dd>", "")
        .replace("</dd>", "");
    html_to_markdown_simple(&list)
}

/// Method and field documentation from a class page's member sections.
///
/// A method is keyed `name/arity` and a field `name`. A method's anchor spells its
/// parameter types as the source declares them (`add(E)`), while the indexer's
/// signatures are erased, so the name and the number of parameters are the key. A
/// name and arity that more than one method of the class has is left out: a doc on
/// the wrong overload would be worse than none.
fn extract_member_docs(html: &str) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut method_counts: HashMap<String, (usize, String)> = HashMap::new();
    let mut field_docs = HashMap::new();
    for (anchor, description) in member_details(html) {
        match anchor_name_and_arity(&anchor) {
            Some((name, arity)) => {
                let entry = method_counts
                    .entry(format!("{name}/{arity}"))
                    .or_insert((0, description.clone()));
                entry.0 += 1;
            }
            None => {
                field_docs.insert(anchor, description);
            }
        }
    }
    let method_docs = method_counts
        .into_iter()
        .filter(|(_, (count, _))| *count == 1)
        .map(|(key, (_, description))| (key, description))
        .collect();
    (method_docs, field_docs)
}

/// The `(anchor, description)` of each `section.detail` of a class page that has a block
/// description. The anchor is the section's `id`, such as `add(E)` or `size`.
fn member_details(html: &str) -> Vec<(String, String)> {
    const SECTION: &str = "<section class=\"detail\"";
    let mut details = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(SECTION) {
        let section = &rest[start..];
        let Some(tag_end) = section.find('>') else {
            break;
        };
        let body_start = tag_end + 1;
        let body_end = section[body_start..]
            .find("</section>")
            .map_or(section.len(), |offset| body_start + offset);
        let body = &section[body_start..body_end];
        if let (Some(anchor), Some(description)) = (
            attribute(&section[..tag_end], "id"),
            block_description(body),
        ) {
            // The anchor's generic brackets are escaped in the page, so decode them before
            // the parameters are counted.
            let anchor = html_escape::decode_html_entities(anchor).to_string();
            details.push((anchor, description));
        }
        rest = &section[body_end..];
    }
    details
}

/// The value of `name="..."` in an HTML start tag.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("{name}=\"");
    let start = tag.find(&marker)? + marker.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// The text of a member's `<div class="block">`, as Markdown, if it is not empty.
fn block_description(body: &str) -> Option<String> {
    const BLOCK: &str = "<div class=\"block\">";
    let description = match body.find(BLOCK) {
        Some(offset) => {
            let start = offset + BLOCK.len();
            let end = body[start..]
                .find("</div>")
                .map_or(body.len(), |i| start + i);
            html_to_markdown_simple(&body[start..end])
        }
        None => String::new(),
    };
    let text = join_paragraphs(description, notes_markdown(body));
    (!text.is_empty()).then_some(text)
}

/// The name and arity of a method anchor such as `add(E)` or `of()`. A field anchor, or an
/// anchor whose parentheses do not open and then close, gives `None`.
fn anchor_name_and_arity(anchor: &str) -> Option<(&str, usize)> {
    let open = anchor.find('(')?;
    let close = anchor.rfind(')').filter(|close| *close > open)?;
    let parameters = anchor[open + 1..close].trim();
    let arity = if parameters.is_empty() {
        0
    } else {
        top_level_commas(parameters) + 1
    };
    Some((&anchor[..open], arity))
}

/// Commas that separate parameters, not those inside a generic argument list.
fn top_level_commas(text: &str) -> usize {
    let mut depth = 0usize;
    let mut commas = 0usize;
    for ch in text.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    commas
}

/// Convert HTML to markdown (simplified)
fn html_to_markdown_simple(html: &str) -> String {
    // This is a very basic HTML to markdown converter
    // A full implementation would handle all HTML tags properly

    let mut result = html.to_string();

    // Remove common HTML tags (very basic)
    result = result.replace("<p>", "\n\n");
    result = result.replace("</p>", "");
    result = result.replace("<code>", "`");
    result = result.replace("</code>", "`");
    result = result.replace("<pre>", "\n```\n");
    result = result.replace("</pre>", "\n```\n");
    result = result.replace("<b>", "**");
    result = result.replace("</b>", "**");
    result = result.replace("<i>", "*");
    result = result.replace("</i>", "*");
    result = result.replace("<br>", "\n");
    result = result.replace("<br/>", "\n");
    result = result.replace("<br />", "\n");

    // List items and table rows start their own lines, and cells are separated. Without
    // this, removing the tags below would run the items and cells together.
    static LIST_AND_TABLE: OnceLock<[regex::Regex; 4]> = OnceLock::new();
    let [list_item, list_end, table_row, cell_end] = LIST_AND_TABLE.get_or_init(|| {
        [
            regex::Regex::new(r"<li\b[^>]*>").expect("valid list item regex"),
            regex::Regex::new(r"</li\s*>").expect("valid list end regex"),
            regex::Regex::new(r"<tr\b[^>]*>").expect("valid table row regex"),
            regex::Regex::new(r"</t[dh]\s*>").expect("valid table cell regex"),
        ]
    });
    result = list_item.replace_all(&result, "\n- ").to_string();
    result = list_end.replace_all(&result, "").to_string();
    result = table_row.replace_all(&result, "\n").to_string();
    result = cell_end.replace_all(&result, " ").to_string();

    // Remove remaining HTML tags
    static HTML_TAGS: OnceLock<regex::Regex> = OnceLock::new();
    let re = HTML_TAGS.get_or_init(|| regex::Regex::new(r"<[^>]+>").expect("valid HTML tag regex"));
    result = re.replace_all(&result, "").to_string();

    // Decode HTML entities
    result = html_escape::decode_html_entities(&result).to_string();

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_markdown_simple() {
        let html = "<p>This is a <code>test</code> with <b>bold</b> text.</p>";
        let markdown = html_to_markdown_simple(html);
        assert!(markdown.contains("`test`"));
        assert!(markdown.contains("**bold**"));
    }

    #[test]
    fn list_items_are_not_run_together() {
        let html = "<ul><li>First item</li><li>Second item</li></ul>";
        let markdown = html_to_markdown_simple(html);
        assert!(
            markdown.contains("- First item\n- Second item"),
            "{markdown}"
        );
    }

    #[test]
    fn table_cells_and_rows_are_not_run_together() {
        let html =
            "<table><tr><th>Name</th><th>Meaning</th></tr><tr><td>a</td><td>b</td></tr></table>";
        let markdown = html_to_markdown_simple(html);
        assert!(markdown.contains("Name Meaning"), "{markdown}");
        assert!(markdown.contains("\na b"), "{markdown}");
        assert!(
            !markdown.contains("Namemeaning") && !markdown.contains("ab"),
            "{markdown}"
        );
    }

    #[test]
    fn test_extract_class_description() {
        let html = r#"
            <html>
                <body>
                    <div class="block">
                        This is a test class description.
                    </div>
                </body>
            </html>
        "#;
        let desc = extract_class_description(html);
        assert!(desc.contains("test class description"));
    }

    #[test]
    fn block_tags_are_read_into_the_description() {
        let html = r#"<section class="detail" id="size()"><div class="block">The number of elements.</div>
<dl class="notes"><dt>Returns:</dt><dd>the count</dd><dt>Since:</dt><dd>1.8</dd></dl></section>
<section class="detail" id="old()"><dl class="notes"><dt>Deprecated.</dt><dd>use size()</dd></dl></section>"#;
        let (methods, _) = extract_member_docs(html);

        let size = methods.get("size/0").expect("size has a block description");
        assert!(size.starts_with("The number of elements."), "{size}");
        assert!(size.contains("**Since:** 1.8"), "{size}");
        assert!(size.contains("**Returns:** the count"), "{size}");
        assert!(
            methods
                .get("old/0")
                .is_some_and(|doc| doc.contains("**Deprecated.** use size()")),
            "a deprecation with no description is still documented"
        );
    }

    #[test]
    fn member_docs_are_keyed_by_name_and_arity() {
        let html = r#"
<section class="detail" id="add(E)"><div class="block">Adds one element.</div></section>
<section class="detail" id="from(Map&lt;K,V&gt;)"><div class="block">Copies a map.</div></section>
<section class="detail" id="put(K,V)"><div class="block">Stores a pair.</div></section>
<section class="detail" id="put(K,V,int)"><div class="block">Stores a pair at a position.</div></section>
<section class="detail" id="remove(int)"><div class="block">Removes by index.</div></section>
<section class="detail" id="remove(Object)"><div class="block">Removes by value.</div></section>
<section class="detail" id="size()"><div class="block">The number of elements.</div></section>
<section class="detail" id="EMPTY"><div class="block">The empty value.</div></section>
<section class="detail" id="NOTHING"></section>
"#;
        let (methods, fields) = extract_member_docs(html);

        assert_eq!(
            methods.get("add/1").map(String::as_str),
            Some("Adds one element.")
        );
        assert_eq!(
            methods.get("from/1").map(String::as_str),
            Some("Copies a map.")
        );
        assert_eq!(
            methods.get("put/2").map(String::as_str),
            Some("Stores a pair.")
        );
        assert_eq!(
            methods.get("put/3").map(String::as_str),
            Some("Stores a pair at a position.")
        );
        assert_eq!(
            methods.get("size/0").map(String::as_str),
            Some("The number of elements.")
        );
        assert!(
            !methods.contains_key("remove/1"),
            "two overloads of one arity are ambiguous and must not share a doc"
        );
        assert_eq!(
            fields.get("EMPTY").map(String::as_str),
            Some("The empty value.")
        );
        assert!(
            !fields.contains_key("NOTHING"),
            "a section without a block description is skipped"
        );
    }

    #[test]
    fn an_anchor_with_a_closing_parenthesis_before_the_opening_one_is_not_a_method() {
        assert_eq!(anchor_name_and_arity("x)("), None);
        let html = r#"<section class="detail" id="x)("><div class="block">Odd.</div></section>"#;
        let (methods, fields) = extract_member_docs(html);
        assert!(methods.is_empty(), "{methods:?}");
        assert_eq!(fields.get("x)(").map(String::as_str), Some("Odd."));
    }

    #[test]
    fn class_description_does_not_take_a_member_description_or_its_block_tags() {
        let html = r#"<section class="class-description" id="class-description">
<div class="type-signature">public class Counter</div>
</section>
<section class="summary"><div class="block">Adds one value.</div></section>
<section class="details"><section class="detail" id="add(int)"><div class="block">Adds one value.</div>
<dl class="notes"><dt>Parameters:</dt><dd>x - the value</dd><dt>Returns:</dt><dd>the sum</dd></dl></section></section>"#;
        assert_eq!(extract_class_description(html), "");

        let with_description = html.replace(
            "</div>\n</section>",
            "</div>\n<div class=\"block\">Counts things.</div>\n</section>",
        );
        assert_eq!(
            extract_class_description(&with_description),
            "Counts things."
        );
    }

    #[test]
    fn member_docs_count_toward_the_retained_byte_limit() {
        // Each member doc is stored under the class name, so a long class name and many
        // small members retain far more than the page's own size.
        let class_path = format!("{}/Big.html", "p".repeat(MAX_ARCHIVE_PATH_BYTES - 16));
        let members = MAX_TOTAL_DOCUMENT_BYTES / (MAX_ARCHIVE_PATH_BYTES - 16) + 1_000;
        let mut html = String::new();
        for index in 0..members {
            html.push_str(&format!(
                "<section class=\"detail\" id=\"f{index}\"><div class=\"block\">d</div></section>"
            ));
        }
        let temp = tempfile::tempdir().expect("temporary fixture directory");
        let jar_path = temp.path().join("big-javadoc.jar");
        let mut jar = zip::ZipWriter::new(std::fs::File::create(&jar_path).expect("create JAR"));
        jar.start_file(class_path, zip::write::SimpleFileOptions::default())
            .expect("start entry");
        std::io::Write::write_all(&mut jar, html.as_bytes()).expect("write entry");
        jar.finish().expect("finish JAR");

        let error = parse_javadoc_jar(&jar_path).expect_err("retained bytes exceed the limit");
        assert!(error.contains("retained bytes"), "{error}");
    }

    #[test]
    fn oversized_html_is_rejected_before_reading() {
        let error = read_html(std::io::empty(), MAX_JAVADOC_HTML_BYTES as u64 + 1).unwrap_err();
        assert!(error.contains("maximum"));
    }
}
