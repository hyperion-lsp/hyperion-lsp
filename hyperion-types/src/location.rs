// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use serde::{Deserialize, Serialize};

/// A half-open byte range in a source file.
///
/// This is the canonical range carried by extracted and indexed symbols. It is
/// intentionally independent of parser and LSP coordinate types: parser ranges
/// use `rowan::TextSize`, while LSP positions use UTF-16 line/column pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

impl TextRange {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub const fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub const fn contains(&self, offset: u32) -> bool {
        offset >= self.start && offset < self.end
    }
}
