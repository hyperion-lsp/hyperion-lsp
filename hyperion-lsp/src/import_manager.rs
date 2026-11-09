// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Import Auto-Insertion Manager
//!
//! Handles automatic insertion of import statements when completing unimported types.
//!
//! # Features
//! - Detects when a completion needs an import
//! - Finds optimal insertion point for new imports
//! - Generates TextEdit for LSP additionalTextEdits
//! - Handles edge cases (java.lang, same package, wildcards, duplicates)
//!
//! # Example
//! ```ignore
//! let manager = ImportManager::new(workspace_index);
//!
//! if manager.needs_import(file_id, "ArrayList", "java.util.ArrayList") {
//!     let edit = manager.generate_import_edit(file_id, "java.util.ArrayList", source);
//!     completion_item.additional_text_edits = Some(vec![edit]);
//! }
//! ```

use hyperion_semantics::db::FileId;
use hyperion_workspace::index::WorkspaceIndex;
use std::sync::Arc;
use tower_lsp::lsp_types::{Position, Range, TextEdit};
use tracing::debug;

/// Manages automatic import insertion for code completion
pub struct ImportManager {
    workspace_index: Arc<WorkspaceIndex>,
}

impl ImportManager {
    /// Create a new ImportManager
    pub const fn new(workspace_index: Arc<WorkspaceIndex>) -> Self {
        Self { workspace_index }
    }

    /// Check if a type needs an import statement
    ///
    /// Returns `true` if:
    /// - Type is not in java.lang (implicitly imported)
    /// - Type is not in same package as current file
    /// - Type is not already explicitly imported
    /// - Type is not covered by a wildcard import
    ///
    /// # Arguments
    /// * `file_id` - The file where completion is requested
    /// * `simple_name` - Simple class name (e.g., "ArrayList")
    /// * `fqn` - Fully qualified name (e.g., "java.util.ArrayList")
    pub fn needs_import(&self, file_id: FileId, simple_name: &str, fqn: &str) -> bool {
        debug!("Checking if import needed: {} ({})", simple_name, fqn);

        // 1. java.lang types are implicitly imported
        // Exception: Inner classes like java.lang.Thread.State still need import
        if let Some(after_java_lang) = fqn.strip_prefix("java.lang.") {
            // Check if it's a top-level class (no $ for inner classes)
            if !after_java_lang.contains('.') && !after_java_lang.contains('$') {
                debug!("  → Skip: java.lang type (implicitly imported)");
                return false;
            }
        }

        // 2. Get current file's import map
        let imports = if let Some(i) = self.workspace_index.get_imports(file_id) {
            i
        } else {
            debug!("  → No import map found, assume import needed");
            return true;
        };

        // 3. Check if in same package
        if let Some(ref current_pkg) = imports.current_package {
            // Extract package from FQN
            if let Some(dot_pos) = fqn.rfind('.') {
                let type_pkg = &fqn[..dot_pos];
                if type_pkg == current_pkg {
                    debug!("  → Skip: Same package as current file ({})", current_pkg);
                    return false;
                }
            }
        }

        // 4. Check if already explicitly imported
        if let Some(existing_fqn) = imports.explicit.get(simple_name) {
            if existing_fqn == fqn {
                debug!("  → Skip: Already explicitly imported");
                return false;
            }
            debug!(
                "  → CONFLICT: {} already maps to {} (want {})",
                simple_name, existing_fqn, fqn
            );
            // Import conflict - don't auto-import
            // User should use fully qualified name
            return false;
        }

        // 5. Check if covered by wildcard import
        if let Some(dot_pos) = fqn.rfind('.') {
            let type_pkg = &fqn[..dot_pos];
            for wildcard_pkg in &imports.wildcards {
                if type_pkg == wildcard_pkg {
                    debug!("  → Skip: Covered by wildcard import ({}.*)", wildcard_pkg);
                    return false;
                }
            }
        }

        debug!("  → Import NEEDED");
        true
    }

    /// Generate a TextEdit to insert an import statement
    ///
    /// Finds the optimal position (after last import or after package)
    /// and creates a TextEdit that can be added to CompletionItem.additional_text_edits
    ///
    /// # Arguments
    /// * `file_id` - The file where import should be inserted
    /// * `fqn` - Fully qualified name to import
    /// * `source` - Source code of the file
    ///
    /// # Returns
    /// TextEdit with import statement, or None if insertion point not found
    pub fn generate_import_edit(
        &self,
        _file_id: FileId,
        fqn: &str,
        source: &str,
    ) -> Option<TextEdit> {
        debug!("Generating import edit for: {}", fqn);

        // Find insertion point
        let position = self.find_import_insertion_point(source)?;

        Some(self.generate_import_edit_at(fqn, position))
    }

    /// Build an import edit at a previously computed insertion point.
    /// Completion uses this to scan the source header at most once per request.
    pub(crate) fn generate_import_edit_at(&self, fqn: &str, position: Position) -> TextEdit {
        // Create import statement
        let import_text = format!("import {fqn};\n");

        debug!(
            "  → Insert at line {}: {}",
            position.line,
            import_text.trim()
        );

        TextEdit {
            range: Range::new(position, position),
            new_text: import_text,
        }
    }

    /// Import edit for a Kotlin source: no `;`, placed *after* the package
    /// declaration and file annotations, and alphabetically among existing
    /// imports when they are sorted (otherwise after the last one).
    pub(crate) fn generate_kotlin_import_edit(&self, fqn: &str, source: &str) -> TextEdit {
        let lines: Vec<&str> = source.lines().collect();
        let is_import = |line: &str| line.trim_start().starts_with("import ");
        let import_target = |line: &str| -> String {
            line.trim()
                .trim_start_matches("import")
                .trim()
                .trim_end_matches(';')
                .split(" as ")
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        };

        let mut package_line: Option<usize> = None;
        let mut imports: Vec<(usize, String)> = Vec::new();
        // Line after leading comments / `@file:` annotations when there is no package.
        let mut header_end = 0usize;
        let mut in_block_comment = false;
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if in_block_comment {
                header_end = index + 1;
                if trimmed.contains("*/") {
                    in_block_comment = false;
                }
                continue;
            }
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("package ") {
                package_line = Some(index);
                continue;
            }
            if is_import(line) {
                imports.push((index, import_target(line)));
                continue;
            }
            if trimmed.starts_with("/*") {
                header_end = index + 1;
                in_block_comment = !trimmed.contains("*/");
                continue;
            }
            if trimmed.starts_with("//") || trimmed.starts_with("@file:") {
                if package_line.is_none() && imports.is_empty() {
                    header_end = index + 1;
                }
                continue;
            }
            // First declaration: the header is over.
            break;
        }

        let import_line = format!("import {fqn}\n");
        let at = |line: usize, text: String| TextEdit {
            range: Range::new(Position::new(line as u32, 0), Position::new(line as u32, 0)),
            new_text: text,
        };

        if !imports.is_empty() {
            let sorted = imports.windows(2).all(|pair| pair[0].1 <= pair[1].1);
            if sorted {
                if let Some((line, _)) =
                    imports.iter().find(|(_, existing)| existing.as_str() > fqn)
                {
                    return at(*line, import_line);
                }
            }
            let last = imports.last().map_or(0, |(line, _)| *line);
            return at(last + 1, import_line);
        }

        if let Some(package) = package_line {
            let next = lines.get(package + 1).map(|line| line.trim());
            return match next {
                // `package a` / blank / code
                Some("") => {
                    let after_blank = lines
                        .get(package + 2)
                        .is_some_and(|line| !line.trim().is_empty());
                    at(
                        package + 2,
                        if after_blank {
                            format!("{import_line}\n")
                        } else {
                            import_line
                        },
                    )
                }
                Some(_) => at(package + 1, format!("\n{import_line}\n")),
                None => at(package + 1, format!("\n{import_line}")),
            };
        }

        // No package: after the header comment / file annotations.
        let text = if lines
            .get(header_end)
            .is_some_and(|line| !line.trim().is_empty())
        {
            format!("{import_line}\n")
        } else {
            import_line
        };
        at(header_end, text)
    }

    /// Find the optimal position to insert a new import statement
    ///
    /// Strategy:
    /// 1. If imports exist: Insert after last import
    /// 2. Else if package exists: Insert after package (with blank line)
    /// 3. Else: Insert at top of file
    ///
    /// # Returns
    /// Position where import should be inserted
    pub(crate) fn find_import_insertion_point(&self, source: &str) -> Option<Position> {
        let mut last_import_line: Option<u32> = None;
        let mut package_line: Option<u32> = None;

        // Simple line-by-line parsing
        for (line_num, line) in source.lines().enumerate() {
            let trimmed = line.trim();

            // Track package statement
            if trimmed.starts_with("package ") && trimmed.ends_with(';') {
                package_line = Some(line_num as u32);
            }

            // Track import statements
            if trimmed.starts_with("import ") && trimmed.contains(';') {
                last_import_line = Some(line_num as u32);
            }

            // Stop at first non-package/non-import/non-comment line
            if !trimmed.is_empty()
                && !trimmed.starts_with("package ")
                && !trimmed.starts_with("import ")
                && !trimmed.starts_with("//")
                && !trimmed.starts_with("/*")
                && !trimmed.starts_with('*')
            {
                break;
            }
        }

        // Determine insertion position
        if let Some(last_import) = last_import_line {
            // Insert after last import
            debug!("  → Found last import at line {}", last_import);
            Some(Position::new(last_import + 1, 0))
        } else if let Some(pkg_line) = package_line {
            // Insert after package with blank line
            debug!("  → Found package at line {}", pkg_line);
            // Insert blank line + import after package
            Some(Position::new(pkg_line + 1, 0))
        } else {
            // Insert at top of file
            debug!("  → No package/imports found, insert at top");
            Some(Position::new(0, 0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_workspace::index::WorkspaceIndex;

    use tempfile::TempDir;

    fn create_test_manager() -> (ImportManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
        let manager = ImportManager::new(Arc::new(index));
        (manager, temp_dir)
    }

    #[test]
    fn test_find_insertion_after_imports() {
        let (manager, _temp) = create_test_manager();

        let source = r"package com.example;

import java.util.List;
import java.util.Map;

public class Test {
}
";

        let position = manager.find_import_insertion_point(source).unwrap();
        assert_eq!(position.line, 4); // After "import java.util.Map;"
    }

    #[test]
    fn test_find_insertion_after_package_no_imports() {
        let (manager, _temp) = create_test_manager();

        let source = r"package com.example;

public class Test {
}
";

        let position = manager.find_import_insertion_point(source).unwrap();
        assert_eq!(position.line, 1); // After package
    }

    #[test]
    fn test_find_insertion_no_package_no_imports() {
        let (manager, _temp) = create_test_manager();

        let source = r"public class Test {
}
";

        let position = manager.find_import_insertion_point(source).unwrap();
        assert_eq!(position.line, 0); // Top of file
    }

    #[test]
    fn test_java_lang_no_import_needed() {
        let (manager, _temp) = create_test_manager();
        let file_id = FileId::new(0);

        // java.lang.String should not need import
        assert!(!manager.needs_import(file_id, "String", "java.lang.String"));

        // java.lang.Integer should not need import
        assert!(!manager.needs_import(file_id, "Integer", "java.lang.Integer"));
    }
}
