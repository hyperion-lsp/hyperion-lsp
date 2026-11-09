// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Oppen scan phase - calculates sizing information for groups
//!
//! This implements the proper Oppen algorithm with O(n) time complexity.
//! The key insight is that we process tokens left-to-right and use a stack
//! to defer size computation for Begin tokens until we see their matching End.
//!
//! Algorithm:
//! 1. Process tokens left-to-right, tracking cumulative "right total"
//! 2. When we see Begin, push it onto scan stack with current right_total
//! 3. When we see End, pop the stack and compute: size = right_total - saved_right_total
//! 4. This avoids re-scanning group contents for each Begin token

use crate::ir::{DocToken, Document};

/// Size information for a token or group
#[derive(Debug, Clone, Copy)]
pub struct Size {
    /// Size when rendered flat (no breaks)
    /// Set to INFINITY if the group contains hardlines
    pub flat: i32,

    /// Size when rendered flat, ignoring internal hardlines
    /// Used for groups that should measure width without treating formatting breaks
    /// as infinite width (e.g., assignments with lambda bodies)
    pub flat_structural: i32,
}

impl Size {
    /// A size large enough to force breaking (larger than any line width)
    pub const INFINITY: i32 = i32::MAX / 2; // Use half to avoid overflow in additions

    #[inline]
    pub const fn new(flat: i32) -> Self {
        Self {
            flat,
            flat_structural: flat,
        }
    }

    #[inline]
    pub const fn zero() -> Self {
        Self {
            flat: 0,
            flat_structural: 0,
        }
    }

    #[inline]
    pub const fn infinity() -> Self {
        Self {
            flat: Self::INFINITY,
            flat_structural: Self::INFINITY,
        }
    }

    /// Create a size that has infinite flat (forces breaks) but preserves structural size
    #[inline]
    pub const fn with_hardline(flat_structural: i32) -> Self {
        Self {
            flat: Self::INFINITY,
            flat_structural,
        }
    }
}

/// Entry on the scan stack for tracking Begin tokens
#[derive(Debug)]
struct ScanEntry {
    /// Index of the Begin token in the document
    token_index: usize,
    /// Right total when this Begin was encountered
    right_total_at_begin: i32,
    /// Structural right total when this Begin was encountered
    right_total_structural_at_begin: i32,
}

/// Scanner that computes sizes for document tokens in O(n) time
pub struct Scanner<'a> {
    tokens: &'a [DocToken],
    /// Computed sizes for each token
    sizes: Vec<Size>,
    /// Stack of Begin tokens awaiting size computation
    scan_stack: Vec<ScanEntry>,
    /// Running total of sizes (used for deferred computation)
    /// Reset to 0 when encountering hardlines to prevent overflow
    right_total: i32,
    /// Running total for structural sizes (continues through hardlines)
    right_total_structural: i32,
}

impl<'a> Scanner<'a> {
    pub fn new(document: &'a Document) -> Self {
        let len = document.tokens.len();
        Self {
            tokens: &document.tokens,
            sizes: vec![Size::zero(); len],
            scan_stack: Vec::with_capacity(32), // Reasonable nesting depth
            right_total: 0,
            right_total_structural: 0,
        }
    }

    /// Compute sizes for all tokens in O(n) time
    pub fn scan(mut self) -> Vec<Size> {
        for i in 0..self.tokens.len() {
            match &self.tokens[i] {
                DocToken::Text(s) => {
                    let size = i32::try_from(s.chars().count()).unwrap_or(0);
                    self.sizes[i] = Size::new(size);
                    self.right_total += size;
                    self.right_total_structural += size;
                }

                DocToken::Break(b) => {
                    let size = i32::try_from(b.flat_text.chars().count()).unwrap_or(0);
                    self.sizes[i] = Size::new(size);
                    self.right_total += size;
                    self.right_total_structural += size;
                }

                DocToken::Begin(_) => {
                    // Push onto scan stack - we'll compute size when we see End
                    self.scan_stack.push(ScanEntry {
                        token_index: i,
                        right_total_at_begin: self.right_total,
                        right_total_structural_at_begin: self.right_total_structural,
                    });
                    // Placeholder size (will be updated when End is found)
                    self.sizes[i] = Size::zero();
                }

                DocToken::End => {
                    self.sizes[i] = Size::zero();

                    // Pop the matching Begin and compute its size
                    if let Some(entry) = self.scan_stack.pop() {
                        let structural_size =
                            self.right_total_structural - entry.right_total_structural_at_begin;

                        if self.sizes[entry.token_index].flat == Size::INFINITY {
                            // Group contains hardline - preserve structural size
                            self.sizes[entry.token_index].flat_structural = structural_size;
                        } else {
                            // No hardline - both sizes are the same
                            let group_size = self.right_total - entry.right_total_at_begin;
                            self.sizes[entry.token_index] = Size::new(group_size);
                        }
                    }
                }

                DocToken::Conditional(c) => {
                    let size = i32::try_from(c.flat_text.chars().count()).unwrap_or(0);
                    self.sizes[i] = Size::new(size);
                    self.right_total += size;
                    self.right_total_structural += size;
                }

                DocToken::Hardline => {
                    // Hardline forces a break - mark all pending groups as "must break"
                    // But track structural size as 0 (the hardline itself doesn't add width)
                    self.sizes[i] = Size::with_hardline(0);
                    self.propagate_infinity();
                }

                DocToken::Literal(s) => {
                    if s.contains('\n') {
                        // Multi-line literal forces break
                        self.sizes[i] = Size::infinity();
                        self.propagate_infinity();
                    } else {
                        let size = i32::try_from(s.chars().count()).unwrap_or(0);
                        self.sizes[i] = Size::new(size);
                        self.right_total += size;
                        self.right_total_structural += size;
                    }
                }

                DocToken::WidthReservation(width) => {
                    let size = i32::try_from(*width).unwrap_or(Size::INFINITY);
                    self.sizes[i] = Size::new(size);
                    self.right_total = self.right_total.saturating_add(size);
                    self.right_total_structural = self.right_total_structural.saturating_add(size);
                }
            }
        }

        // Any remaining unclosed groups get their sizes computed
        // (shouldn't happen with well-formed input, but handle gracefully)
        for entry in &self.scan_stack {
            let structural_size =
                self.right_total_structural - entry.right_total_structural_at_begin;

            if self.sizes[entry.token_index].flat == Size::INFINITY {
                self.sizes[entry.token_index].flat_structural = structural_size;
            } else {
                let group_size = self.right_total - entry.right_total_at_begin;
                self.sizes[entry.token_index] = Size::new(group_size);
            }
        }

        self.sizes
    }

    /// When we encounter a Hardline or multi-line Literal, all enclosing groups
    /// must break. We mark them with INFINITY size.
    fn propagate_infinity(&mut self) {
        // All Begin tokens on the stack contain this Hardline, so they must break
        for entry in &self.scan_stack {
            self.sizes[entry.token_index] = Size::infinity();
        }
        // Reset right_total since we're forcing breaks anyway
        // This prevents integer overflow on very large documents
        self.right_total = 0;
        // Clear the stack entries' right_total_at_begin to match
        for entry in &mut self.scan_stack {
            entry.right_total_at_begin = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FormatConfig;
    use crate::ir::DocumentBuilder;

    #[test]
    fn test_scan_text() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.text("hello");

        let doc = builder.finish();
        let scanner = Scanner::new(&doc);
        let sizes = scanner.scan();

        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].flat, 5);
    }

    #[test]
    fn test_scan_group() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.group(|b| {
            b.text("a");
            b.space();
            b.text("b");
        });

        let doc = builder.finish();
        let scanner = Scanner::new(&doc);
        let sizes = scanner.scan();

        // Group contains: "a" (1) + " " (1) + "b" (1) = 3
        assert_eq!(sizes[0].flat, 3); // Begin token has group size
    }

    #[test]
    fn test_scan_nested_groups() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.group(|b| {
            b.text("outer");
            b.space();
            b.group(|b2| {
                b2.text("inner");
                b2.space();
                b2.text("text");
            });
        });

        let doc = builder.finish();
        let scanner = Scanner::new(&doc);
        let sizes = scanner.scan();

        // Inner group: "inner" (5) + " " (1) + "text" (4) = 10
        // Outer group: "outer" (5) + " " (1) + inner(10) = 16
        // tokens: Begin, "outer", Break, Begin, "inner", Break, "text", End, End

        // Find the outer Begin (index 0) and inner Begin (index 3)
        assert_eq!(sizes[0].flat, 16); // Outer group
        assert_eq!(sizes[3].flat, 10); // Inner group
    }

    #[test]
    fn test_scan_hardline_forces_break() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.group(|b| {
            b.text("before");
            b.hardline();
            b.text("after");
        });

        let doc = builder.finish();
        let scanner = Scanner::new(&doc);
        let sizes = scanner.scan();

        // Begin token should have INFINITY because it contains a hardline
        assert_eq!(sizes[0].flat, Size::INFINITY);
    }

    #[test]
    fn test_scan_deeply_nested_is_still_linear() {
        // This would be O(n²) with the naive algorithm
        // With proper Oppen, it's O(n)

        // Create 100 levels of nesting - helper must be defined before use
        fn build_nested(b: &mut DocumentBuilder, depth: usize) {
            if depth == 0 {
                b.text("leaf");
            } else {
                b.group(|b2| {
                    b2.text("x");
                    build_nested(b2, depth - 1);
                });
            }
        }

        let mut builder = DocumentBuilder::new(FormatConfig::default());
        build_nested(&mut builder, 100);

        let doc = builder.finish();
        let scanner = Scanner::new(&doc);
        let sizes = scanner.scan();

        // Just verify it completes without stack overflow or timeout
        // The outermost group should have size = 100 * 1 + 4 = 104
        // (100 "x" chars + "leaf")
        assert!(sizes[0].flat > 0);
        assert!(sizes[0].flat < Size::INFINITY);
    }

    #[test]
    fn test_scan_empty_group() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.group(|_| {
            // Empty group
        });

        let doc = builder.finish();
        let scanner = Scanner::new(&doc);
        let sizes = scanner.scan();

        assert_eq!(sizes[0].flat, 0); // Empty group has size 0
    }

    #[test]
    fn test_scan_multiple_groups() {
        let mut builder = DocumentBuilder::new(FormatConfig::default());
        builder.group(|b| {
            b.text("aaa");
        });
        builder.group(|b| {
            b.text("bb");
        });

        let doc = builder.finish();
        let scanner = Scanner::new(&doc);
        let sizes = scanner.scan();

        // First group: Begin, "aaa", End
        // Second group: Begin, "bb", End
        assert_eq!(sizes[0].flat, 3); // First group
        assert_eq!(sizes[3].flat, 2); // Second group
    }
}
