// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for TextEdit operations
//!
//! These tests cover all text editing operations including insertions, deletions,
//! replacements, edge cases, error handling, and unicode support.

#[cfg(test)]
mod text_edit_comprehensive_tests {
    use crate::text_edit::TextEdit;
    use rowan::{TextRange, TextSize};

    // ========================================================================
    // Basic Operations Tests
    // ========================================================================

    #[test]
    fn test_insertion_at_beginning() {
        let source = "world";
        let edit = TextEdit::insert(0.into(), "Hello, ".to_string());
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello, world");
    }

    #[test]
    fn test_insertion_at_middle() {
        let source = "HelloWorld";
        let edit = TextEdit::insert(5.into(), " ".to_string());
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_insertion_at_end() {
        let source = "Hello";
        let edit = TextEdit::insert(5.into(), "!".to_string());
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello!");
    }

    #[test]
    fn test_deletion_from_beginning() {
        let source = "Hello, world!";
        let edit = TextEdit::delete(TextRange::new(0.into(), 7.into()));
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "world!");
    }

    #[test]
    fn test_deletion_from_middle() {
        let source = "Hello, cruel world!";
        let edit = TextEdit::delete(TextRange::new(6.into(), 12.into()));
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_deletion_to_end() {
        let source = "Hello, world!";
        let edit = TextEdit::delete(TextRange::new(5.into(), 13.into()));
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_simple_replacement() {
        let source = "Hello, world!";
        let edit = TextEdit::replace(
            TextRange::new(7.into(), 12.into()),
            "universe".into(),
        );
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello, universe!");
    }

    #[test]
    fn test_replacement_shorter_text() {
        let source = "The quick brown fox";
        let edit = TextEdit::replace(TextRange::new(4.into(), 9.into()), "slow".to_string());
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "The slow brown fox");
    }

    #[test]
    fn test_replacement_longer_text() {
        let source = "The fast fox";
        let edit = TextEdit::replace(
            TextRange::new(4.into(), 8.into()),
            "extremely quick".into(),
        );
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "The extremely quick fox");
    }

    // ========================================================================
    // Multiple Edits Tests
    // ========================================================================

    #[test]
    fn test_multiple_insertions_ordered() {
        let source = "abc";
        let edits = vec![
            TextEdit::insert(0.into(), "0".to_string()),
            TextEdit::insert(1.into(), "1".to_string()),
            TextEdit::insert(2.into(), "2".to_string()),
            TextEdit::insert(3.into(), "3".to_string()),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "0a1b2c3");
    }

    #[test]
    fn test_multiple_insertions_unordered() {
        let source = "abc";
        let edits = vec![
            TextEdit::insert(3.into(), "3".to_string()),
            TextEdit::insert(0.into(), "0".to_string()),
            TextEdit::insert(2.into(), "2".to_string()),
            TextEdit::insert(1.into(), "1".to_string()),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "0a1b2c3");
    }

    #[test]
    fn test_multiple_deletions() {
        let source = "a1b2c3d4";
        let edits = vec![
            TextEdit::delete(TextRange::new(1.into(), 2.into())),
            TextEdit::delete(TextRange::new(3.into(), 4.into())),
            TextEdit::delete(TextRange::new(5.into(), 6.into())),
            TextEdit::delete(TextRange::new(7.into(), 8.into())),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "abcd");
    }

    #[test]
    fn test_multiple_replacements() {
        let source = "one two three four";
        let edits = vec![
            TextEdit::replace(TextRange::new(0.into(), 3.into()), "1".to_string()),
            TextEdit::replace(TextRange::new(4.into(), 7.into()), "2".to_string()),
            TextEdit::replace(TextRange::new(8.into(), 13.into()), "3".to_string()),
            TextEdit::replace(TextRange::new(14.into(), 18.into()), "4".to_string()),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "1 2 3 4");
    }

    #[test]
    fn test_mixed_operations() {
        let source = "The quick brown fox jumps";
        let edits = vec![
            TextEdit::insert(0.into(), "[".to_string()),
            TextEdit::replace(TextRange::new(4.into(), 9.into()), "slow".to_string()),
            TextEdit::delete(TextRange::new(20.into(), 25.into())),
            TextEdit::insert(25.into(), "]".to_string()),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "[The slow brown fox ]");
    }

    #[test]
    fn test_ten_consecutive_edits() {
        let source = "0123456789";
        let edits: Vec<_> = (0..10)
            .map(|i| {
                TextEdit::replace(
                    TextRange::new((i as u32).into(), (i as u32 + 1).into()),
                    format!("_{}_", i),
                )
            })
            .collect();
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "_0__1__2__3__4__5__6__7__8__9_");
    }

    // ========================================================================
    // Adjacent Edits Tests
    // ========================================================================

    #[test]
    fn test_adjacent_insertions() {
        let source = "ab";
        let edits = vec![
            TextEdit::insert(1.into(), "X".to_string()),
            TextEdit::insert(1.into(), "Y".to_string()),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        // Both happen at position 1, should be applied in order
        assert_eq!(result, "aXYb");
    }

    #[test]
    fn test_adjacent_replacements() {
        let source = "abcdef";
        let edits = vec![
            TextEdit::replace(TextRange::new(0.into(), 3.into()), "123".to_string()),
            TextEdit::replace(TextRange::new(3.into(), 6.into()), "456".to_string()),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "123456");
    }

    #[test]
    fn test_deletion_and_insertion_at_boundary() {
        let source = "abc_def";
        let edits = vec![
            TextEdit::delete(TextRange::new(3.into(), 4.into())),
            TextEdit::insert(4.into(), "-".to_string()),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "abc-def");
    }

    // ========================================================================
    // Edge Cases Tests
    // ========================================================================

    #[test]
    fn test_empty_source() {
        let source = "";
        let edit = TextEdit::insert(0.into(), "Hello".to_string());
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_no_edits() {
        let source = "unchanged text";
        let result = TextEdit::apply_edits(source, &[]);
        assert_eq!(result, "unchanged text");
    }

    #[test]
    fn test_replace_entire_content() {
        let source = "old content";
        let edit = TextEdit::replace(
            TextRange::new(0.into(), 11.into()),
            "completely new content".into(),
        );
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "completely new content");
    }

    #[test]
    fn test_delete_entire_content() {
        let source = "everything goes";
        let edit = TextEdit::delete(TextRange::new(0.into(), 15.into()));
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "");
    }

    #[test]
    fn test_empty_replacement() {
        let source = "Hello___World";
        let edit = TextEdit::replace(TextRange::new(5.into(), 8.into()), "".to_string());
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "HelloWorld");
    }

    #[test]
    fn test_zero_length_range() {
        let source = "ab";
        let edit = TextEdit::new(TextRange::empty(1.into()), "X".to_string());
        assert!(edit.is_insertion());
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "aXb");
    }

    // ========================================================================
    // Error Cases Tests
    // ========================================================================

    #[test]
    #[should_panic(expected = "Overlapping edits")]
    fn test_overlapping_edits_at_start() {
        let source = "abcdef";
        let edits = vec![
            TextEdit::replace(TextRange::new(0.into(), 4.into()), "ABCD".to_string()),
            TextEdit::replace(TextRange::new(2.into(), 6.into()), "CDEF".to_string()),
        ];
        TextEdit::apply_edits(source, &edits);
    }

    #[test]
    #[should_panic(expected = "Overlapping edits")]
    fn test_overlapping_edits_nested() {
        let source = "abcdefgh";
        let edits = vec![
            TextEdit::replace(TextRange::new(2.into(), 6.into()), "XXXX".to_string()),
            TextEdit::replace(TextRange::new(3.into(), 5.into()), "YY".to_string()),
        ];
        TextEdit::apply_edits(source, &edits);
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn test_edit_beyond_end() {
        let source = "short";
        let edit = TextEdit::replace(TextRange::new(0.into(), 100.into()), "x".to_string());
        TextEdit::apply_edits(source, &[edit]);
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn test_insert_beyond_end() {
        let source = "short";
        let edit = TextEdit::insert(100.into(), "x".to_string());
        TextEdit::apply_edits(source, &[edit]);
    }

    #[test]
    fn test_try_apply_overlapping_returns_error() {
        let source = "abcdef";
        let edits = vec![
            TextEdit::replace(TextRange::new(0.into(), 4.into()), "ABCD".to_string()),
            TextEdit::replace(TextRange::new(2.into(), 6.into()), "CDEF".to_string()),
        ];
        let result = TextEdit::try_apply_edits(source, &edits);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Overlapping"));
    }

    #[test]
    fn test_try_apply_out_of_bounds_returns_error() {
        let source = "short";
        let edit = TextEdit::replace(TextRange::new(0.into(), 100.into()), "x".to_string());
        let result = TextEdit::try_apply_edits(source, &[edit]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("out of bounds"));
    }

    #[test]
    fn test_try_apply_success() {
        let source = "Hello, world!";
        let edit = TextEdit::replace(
            TextRange::new(7.into(), 12.into()),
            "universe".into(),
        );
        let result = TextEdit::try_apply_edits(source, &[edit]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, universe!");
    }

    // ========================================================================
    // Unicode Tests
    // ========================================================================

    #[test]
    fn test_unicode_characters() {
        let source = "Hello, 世界!";
        // 世界 is 6 bytes (3 bytes each)
        let edit = TextEdit::replace(
            TextRange::new(7.into(), 13.into()),
            "World".into(),
        );
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_unicode_emoji() {
        let source = "Hello 👋 World";
        // 👋 is 4 bytes
        let edit = TextEdit::delete(TextRange::new(6.into(), 10.into()));
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello  World");
    }

    #[test]
    fn test_unicode_insertion() {
        let source = "Helloworld";
        let edit = TextEdit::insert(5.into(), " 🌍 ".to_string());
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Hello 🌍 world");
    }

    #[test]
    fn test_mixed_unicode_and_ascii() {
        let source = "café résumé";
        let edits = vec![
            TextEdit::replace(TextRange::new(0.into(), 5.into()), "coffee".to_string()),
            TextEdit::replace(TextRange::new(6.into(), 12.into()), "resume".to_string()),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "coffee resume");
    }

    #[test]
    fn test_cyrillic_text() {
        let source = "Привет, мир!";
        let edit = TextEdit::replace(
            TextRange::new(16.into(), 22.into()),
            "world".into(),
        );
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "Привет, world!");
    }

    // ========================================================================
    // Type Check Tests
    // ========================================================================

    #[test]
    fn test_is_insertion() {
        let insert = TextEdit::insert(5.into(), "text".to_string());
        assert!(insert.is_insertion());
        assert!(!insert.is_deletion());
        assert!(!insert.is_replacement());
    }

    #[test]
    fn test_is_deletion() {
        let delete = TextEdit::delete(TextRange::new(0.into(), 5.into()));
        assert!(!delete.is_insertion());
        assert!(delete.is_deletion());
        assert!(!delete.is_replacement());
    }

    #[test]
    fn test_is_replacement() {
        let replace = TextEdit::replace(
            TextRange::new(0.into(), 5.into()),
            "new".into(),
        );
        assert!(!replace.is_insertion());
        assert!(!replace.is_deletion());
        assert!(replace.is_replacement());
    }

    #[test]
    fn test_empty_new_text_is_deletion() {
        let edit = TextEdit::new(TextRange::new(0.into(), 5.into()), String::new());
        assert!(edit.is_deletion());
    }

    #[test]
    fn test_empty_range_is_insertion() {
        let edit = TextEdit::new(TextRange::empty(5.into()), "text".to_string());
        assert!(edit.is_insertion());
    }

    // ========================================================================
    // Position Tests
    // ========================================================================

    #[test]
    fn test_start_and_end() {
        let edit = TextEdit::replace(
            TextRange::new(10.into(), 20.into()),
            "text".into(),
        );
        assert_eq!(edit.start(), TextSize::from(10));
        assert_eq!(edit.end(), TextSize::from(20));
    }

    // ========================================================================
    // Ordering Tests
    // ========================================================================

    #[test]
    fn test_edit_ordering() {
        let edit1 = TextEdit::new(TextRange::new(0.into(), 5.into()), "a".to_string());
        let edit2 = TextEdit::new(TextRange::new(10.into(), 15.into()), "b".to_string());
        let edit3 = TextEdit::new(TextRange::new(0.into(), 10.into()), "c".to_string());

        assert!(edit1 < edit2);
        assert!(edit1 < edit3); // Same start, but edit1 ends earlier
    }

    #[test]
    fn test_edit_ordering_same_start() {
        let edit1 = TextEdit::new(TextRange::new(5.into(), 10.into()), "a".to_string());
        let edit2 = TextEdit::new(TextRange::new(5.into(), 15.into()), "b".to_string());

        assert!(edit1 < edit2);
    }

    // ========================================================================
    // Real-world Scenarios Tests
    // ========================================================================

    #[test]
    fn test_java_method_rename() {
        let source = "public void oldMethod() { oldMethod(); }";
        let edits = vec![
            TextEdit::replace(
                TextRange::new(12.into(), 21.into()),
                "newMethod".into(),
            ),
            TextEdit::replace(
                TextRange::new(26.into(), 35.into()),
                "newMethod".into(),
            ),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "public void newMethod() { newMethod(); }");
    }

    #[test]
    fn test_import_statement_addition() {
        let source = "package com.example;\n\npublic class Test {}";
        let edit = TextEdit::insert(21.into(), "import java.util.List;\n".to_string());
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(
            result,
            "package com.example;\nimport java.util.List;\n\npublic class Test {}"
        );
    }

    #[test]
    fn test_comment_removal() {
        let source = "// TODO: fix this\ncode();";
        let edit = TextEdit::delete(TextRange::new(0.into(), 18.into()));
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "code();");
    }

    #[test]
    fn test_multiline_string_replacement() {
        let source = "String text = \"line1\nline2\nline3\";";
        let edit = TextEdit::replace(
            TextRange::new(14.into(), 32.into()),
            "\"single line\"".into(),
        );
        let result = TextEdit::apply_edits(source, &[edit]);
        assert_eq!(result, "String text = \"single line\";");
    }

    #[test]
    fn test_whitespace_normalization() {
        let source = "a    b    c    d";
        let edits = vec![
            TextEdit::replace(TextRange::new(1.into(), 5.into()), " ".to_string()),
            TextEdit::replace(TextRange::new(6.into(), 10.into()), " ".to_string()),
            TextEdit::replace(TextRange::new(11.into(), 15.into()), " ".to_string()),
        ];
        let result = TextEdit::apply_edits(source, &edits);
        assert_eq!(result, "a b c d");
    }

    // ========================================================================
    // Large-scale Tests
    // ========================================================================

    #[test]
    fn test_many_small_edits() {
        let source = "a".repeat(100);
        let edits: Vec<_> = (0..100)
            .step_by(2)
            .map(|i| TextEdit::insert((i as u32).into(), ".".to_string()))
            .collect();
        let result = TextEdit::apply_edits(&source, &edits);
        // Should have dots between every other 'a'
        assert!(result.contains(".a"));
    }

    #[test]
    fn test_large_document_edit() {
        let source = "x".repeat(10000);
        let edit = TextEdit::replace(
            TextRange::new(5000.into(), 5010.into()),
            "REPLACED".into(),
        );
        let result = TextEdit::apply_edits(&source, &[edit]);
        assert_eq!(result.len(), 10000 - 10 + 8); // Original - deleted + inserted
        assert!(result.contains("REPLACED"));
    }

    // ========================================================================
    // Clone and Equality Tests
    // ========================================================================

    #[test]
    fn test_edit_clone() {
        let edit1 = TextEdit::replace(
            TextRange::new(0.into(), 5.into()),
            "text".into(),
        );
        let edit2 = edit1.clone();
        assert_eq!(edit1, edit2);
    }

    #[test]
    fn test_edit_equality() {
        let edit1 = TextEdit::replace(
            TextRange::new(0.into(), 5.into()),
            "text".into(),
        );
        let edit2 = TextEdit::replace(
            TextRange::new(0.into(), 5.into()),
            "text".into(),
        );
        assert_eq!(edit1, edit2);
    }

    #[test]
    fn test_edit_inequality() {
        let edit1 = TextEdit::replace(
            TextRange::new(0.into(), 5.into()),
            "text1".into(),
        );
        let edit2 = TextEdit::replace(
            TextRange::new(0.into(), 5.into()),
            "text2".into(),
        );
        assert_ne!(edit1, edit2);
    }
}
