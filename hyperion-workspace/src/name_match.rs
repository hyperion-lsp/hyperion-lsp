// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Identifier matching for completion: prefix and camel-hump.
//!
//! The one definition of "what the user typed matches this name", shared by
//! the index-side type search and the completion generators so a type and a
//! member are matched by the same rule.

/// How well a typed pattern matches a candidate name. Lower is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NameMatch {
    /// Identical, including case.
    Exact,
    /// Case-sensitive prefix (`Arr` -> `ArrayList`).
    Prefix,
    /// Case-insensitive prefix (`arr` -> `ArrayList`).
    PrefixIgnoreCase,
    /// Camel-hump initials (`gEm` -> `getEmail`, `HM` -> `HashMap`).
    CamelHump,
}

/// Classify how `pattern` matches `candidate`; `None` when it does not.
/// An empty pattern matches everything as a case-sensitive prefix.
#[must_use]
pub fn match_name(pattern: &str, candidate: &str) -> Option<NameMatch> {
    if pattern.is_empty() {
        return Some(NameMatch::Prefix);
    }
    if pattern == candidate {
        return Some(NameMatch::Exact);
    }
    if candidate.starts_with(pattern) {
        return Some(NameMatch::Prefix);
    }
    if starts_with_ignore_case(candidate, pattern) {
        return Some(NameMatch::PrefixIgnoreCase);
    }
    camel_hump_matches(pattern, candidate).then_some(NameMatch::CamelHump)
}

fn starts_with_ignore_case(candidate: &str, pattern: &str) -> bool {
    let mut candidate_chars = candidate.chars();
    pattern.chars().all(|p| {
        candidate_chars
            .next()
            .is_some_and(|c| c.to_lowercase().eq(p.to_lowercase()))
    })
}

/// Camel-hump match: the first character matches the first character of the
/// candidate, and every further pattern character either continues directly
/// after the previous match or starts a later word (an uppercase letter, a
/// digit, or the character after `_`/`$`).
#[must_use]
pub fn camel_hump_matches(pattern: &str, candidate: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let candidate: Vec<char> = candidate.chars().collect();
    if pattern.is_empty() {
        return true;
    }
    hump_from(&pattern, 0, &candidate, 0)
}

fn hump_from(pattern: &[char], pattern_index: usize, candidate: &[char], from: usize) -> bool {
    if pattern_index == pattern.len() {
        return true;
    }
    let wanted = pattern[pattern_index];
    let matches_at = |position: usize| {
        candidate[position].to_lowercase().eq(wanted.to_lowercase())
            && hump_from(pattern, pattern_index + 1, candidate, position + 1)
    };
    if pattern_index == 0 {
        // The first character anchors the match to the start of the name.
        return !candidate.is_empty() && matches_at(0);
    }
    (from..candidate.len())
        .filter(|&position| position == from || is_word_start(candidate, position))
        .any(matches_at)
}

fn is_word_start(candidate: &[char], position: usize) -> bool {
    let current = candidate[position];
    if position == 0 {
        return true;
    }
    let previous = candidate[position - 1];
    current.is_uppercase()
        || (current.is_ascii_digit() && !previous.is_ascii_digit())
        || previous == '_'
        || previous == '$'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_matches() {
        assert_eq!(match_name("getEmail", "getEmail"), Some(NameMatch::Exact));
        assert_eq!(match_name("Arr", "ArrayList"), Some(NameMatch::Prefix));
        assert_eq!(
            match_name("arr", "ArrayList"),
            Some(NameMatch::PrefixIgnoreCase)
        );
        assert_eq!(match_name("gEm", "getEmail"), Some(NameMatch::CamelHump));
        assert_eq!(match_name("HM", "HashMap"), Some(NameMatch::CamelHump));
        assert_eq!(match_name("hMap", "HashMap"), Some(NameMatch::CamelHump));
        assert_eq!(match_name("sBld", "StringBuilder"), None);
        assert_eq!(
            match_name("sB", "StringBuilder"),
            Some(NameMatch::CamelHump)
        );
    }

    #[test]
    fn hump_requires_the_first_letter_and_word_boundaries() {
        assert!(!camel_hump_matches("mail", "getEmail"));
        assert!(!camel_hump_matches("xM", "HashMap"));
        assert!(camel_hump_matches("gEma", "getEmail"));
        assert!(camel_hump_matches("TE", "TIMEOUT_ERROR"));
        assert!(!camel_hump_matches("gEmz", "getEmail"));
    }
}
