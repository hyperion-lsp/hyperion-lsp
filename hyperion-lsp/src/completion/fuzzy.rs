// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Fuzzy Matching for Completions
//!
//! Supports intelligent fuzzy matching including:
//! - Prefix matching: "arr" → "ArrayList"
//! - CamelCase abbreviation: "aL" → "ArrayList", "sBld" → "StringBuilder"
//! - Substring matching: "List" → "ArrayList"
//!
//! Scoring prioritizes:
//! 1. Exact matches (highest)
//! 2. Prefix matches
//! 3. CamelCase matches
//! 4. Substring matches (lowest)

/// Fuzzy match result with score
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    /// Match score (higher is better)
    pub score: i32,
    /// Whether this is a camelCase match
    pub is_camel_case: bool,
}

/// Perform fuzzy matching on a candidate string
///
/// Returns None if no match, Some(FuzzyMatch) with score if matched
///
/// # Examples
///
/// ```
/// use hyperion_lsp::completion::fuzzy::fuzzy_match;
///
/// assert!(fuzzy_match("sBld", "StringBuilder").is_some());
/// assert!(fuzzy_match("aL", "ArrayList").is_some());
/// assert!(fuzzy_match("hMap", "HashMap").is_some());
/// assert!(fuzzy_match("xyz", "ArrayList").is_none());
/// ```
pub fn fuzzy_match(pattern: &str, candidate: &str) -> Option<FuzzyMatch> {
    if pattern.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            is_camel_case: false,
        });
    }

    let pattern_lower = pattern.to_lowercase();
    let candidate_lower = candidate.to_lowercase();

    // 1. Exact match (highest score)
    if pattern == candidate {
        return Some(FuzzyMatch {
            score: 1000,
            is_camel_case: false,
        });
    }

    // 2. Prefix match (high score)
    if candidate_lower.starts_with(&pattern_lower) {
        return Some(FuzzyMatch {
            score: 900 - pattern.len() as i32, // Shorter patterns rank higher
            is_camel_case: false,
        });
    }

    // 3. CamelCase abbreviation match (medium-high score)
    if let Some(score) = camel_case_match(pattern, candidate) {
        return Some(FuzzyMatch {
            score,
            is_camel_case: true,
        });
    }

    // 4. Substring match (low score)
    if candidate_lower.contains(&pattern_lower) {
        // Position-based scoring: earlier matches rank higher
        let pos = candidate_lower.find(&pattern_lower).unwrap();
        return Some(FuzzyMatch {
            score: (100 - pos as i32).max(1),
            is_camel_case: false,
        });
    }

    // No match
    None
}

/// Check if pattern matches candidate as camelCase abbreviation
///
/// Examples:
/// - "aL" matches "ArrayList" (a + L)
/// - "sBld" matches "StringBuilder" (s + B + ld)
/// - "hMap" matches "HashMap" (h + M + ap)
fn camel_case_match(pattern: &str, candidate: &str) -> Option<i32> {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let candidate_chars: Vec<char> = candidate.chars().collect();

    let mut pattern_idx = 0;
    let mut candidate_idx = 0;
    let mut score = 500; // Base score for camelCase matches

    while pattern_idx < pattern_chars.len() && candidate_idx < candidate_chars.len() {
        let p_char = pattern_chars[pattern_idx];
        let c_char = candidate_chars[candidate_idx];

        if p_char.to_lowercase().eq(c_char.to_lowercase()) {
            // Characters match
            pattern_idx += 1;
            candidate_idx += 1;

            // Bonus for matching uppercase letters (camelCase boundaries)
            if c_char.is_uppercase() {
                score += 10;
            }
        } else if c_char.is_uppercase() && pattern_idx < pattern_chars.len() {
            // We're at a camelCase boundary in candidate
            // Check if next pattern char matches this uppercase char
            let next_p_char = pattern_chars[pattern_idx];
            if next_p_char.to_lowercase().eq(c_char.to_lowercase()) {
                pattern_idx += 1;
                candidate_idx += 1;
                score += 20; // Extra bonus for matching at boundaries
            } else {
                candidate_idx += 1;
            }
        } else {
            candidate_idx += 1;
        }
    }

    // Match succeeds if we consumed entire pattern
    if pattern_idx == pattern_chars.len() {
        // Penalize if we didn't consume much of the candidate
        let consumption = (pattern_idx as f32 / candidate_chars.len() as f32 * 100.0) as i32;
        Some(score + consumption)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let result = fuzzy_match("ArrayList", "ArrayList");
        assert!(result.is_some());
        assert_eq!(result.unwrap().score, 1000);
    }

    #[test]
    fn test_prefix_match() {
        let result = fuzzy_match("Array", "ArrayList");
        assert!(result.is_some());
        assert!(result.unwrap().score > 800);
    }

    #[test]
    fn test_camel_case_match() {
        let result = fuzzy_match("aL", "ArrayList");
        assert!(result.is_some());
        assert!(result.unwrap().is_camel_case);

        let result = fuzzy_match("sBld", "StringBuilder");
        assert!(result.is_some());
        assert!(result.unwrap().is_camel_case);

        let result = fuzzy_match("hMap", "HashMap");
        assert!(result.is_some());
        assert!(result.unwrap().is_camel_case);
    }

    #[test]
    fn test_substring_match() {
        // Test that substrings match (regardless of match type)
        let result = fuzzy_match("rray", "ArrayList");
        assert!(result.is_some());
        // Score should be lower than exact or prefix matches
        let match_result = result.unwrap();
        assert!(
            match_result.score < 900,
            "Should score lower than prefix match"
        );
    }

    #[test]
    fn test_no_match() {
        let result = fuzzy_match("xyz", "ArrayList");
        assert!(result.is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let result = fuzzy_match("arraylist", "ArrayList");
        assert!(result.is_some());

        let result = fuzzy_match("ARRAYLIST", "ArrayList");
        assert!(result.is_some());
    }

    #[test]
    fn test_empty_pattern() {
        let result = fuzzy_match("", "ArrayList");
        assert!(result.is_some());
        assert_eq!(result.unwrap().score, 0);
    }
}
