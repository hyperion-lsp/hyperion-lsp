// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Parser- and protocol-neutral semantic highlighting facts.

use crate::TextRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticHighlightKind {
    Class,
    Interface,
    Enum,
    TypeParameter,
    Method,
    Property,
    Variable,
    Parameter,
    Keyword,
    Modifier,
    Comment,
    String,
    Number,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SemanticHighlightModifiers {
    pub declaration: bool,
    pub definition: bool,
    pub readonly: bool,
    pub static_member: bool,
    pub abstract_member: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticHighlight {
    pub range: TextRange,
    pub kind: SemanticHighlightKind,
    pub modifiers: SemanticHighlightModifiers,
}

impl SemanticHighlight {
    pub const fn new(range: TextRange, kind: SemanticHighlightKind) -> Self {
        Self {
            range,
            kind,
            modifiers: SemanticHighlightModifiers {
                declaration: false,
                definition: false,
                readonly: false,
                static_member: false,
                abstract_member: false,
            },
        }
    }

    pub const fn declaration(mut self) -> Self {
        self.modifiers.declaration = true;
        self
    }

    pub const fn readonly(mut self) -> Self {
        self.modifiers.readonly = true;
        self
    }
}
