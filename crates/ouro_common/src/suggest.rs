// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Fuzzy-match helper for suggesting similarly-named operations.

/// Build an error for an unsupported operation, optionally suggesting
/// a similarly-named candidate when one is close enough by edit distance.
///
/// # Arguments
/// * `operation_name` - The operation name that was not recognized
/// * `supported` - Slice of supported operation names to match against
///
/// # Returns
/// An `anyhow::Error` with a suggestion if a close match (Levenshtein distance ≤ 3) exists.
pub fn unsupported_operation_error(operation_name: &str, supported: &[&str]) -> anyhow::Error {
    let suggestion = find_closest(operation_name, supported, 3);
    match suggestion {
        Some(candidate) => anyhow::anyhow!(
            "Unsupported operation: '{}'. Did you mean '{}'?",
            operation_name,
            candidate
        ),
        None => anyhow::anyhow!("Unsupported operation: '{}'", operation_name),
    }
}

/// Find the closest string by Levenshtein distance, returning it only when
/// the distance is at most `max_distance`.
///
/// # Arguments
/// * `input` - The string to match against candidates
/// * `candidates` - Slice of candidate strings to compare
/// * `max_distance` - Maximum allowed Levenshtein distance (inclusive)
///
/// # Returns
/// The closest candidate within `max_distance`, or `None`.
pub fn find_closest<'a>(
    input: &str,
    candidates: &[&'a str],
    max_distance: usize,
) -> Option<&'a str> {
    candidates
        .iter()
        .map(|c| (*c, levenshtein(input, c)))
        .filter(|(_, d)| *d <= max_distance)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c)
}

/// Classic Levenshtein distance (edit distance) between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();
    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_close_match() {
        let err = unsupported_operation_error(
            "get_shadows",
            &["get_shadow", "list_shadow_ids", "delete_shadow"],
        );
        let msg = format!("{}", err);
        assert!(msg.contains("Did you mean 'get_shadow'?"), "got: {msg}");
    }

    #[test]
    fn no_suggestion_when_too_far() {
        let err =
            unsupported_operation_error("totally_unknown", &["get_shadow", "list_shadow_ids"]);
        let msg = format!("{}", err);
        assert!(!msg.contains("Did you mean"), "got: {msg}");
        assert!(
            msg.contains("Unsupported operation: 'totally_unknown'"),
            "got: {msg}"
        );
    }

    #[test]
    fn levenshtein_basic_cases() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", "xyz"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("same", "same"), 0);
    }
}
