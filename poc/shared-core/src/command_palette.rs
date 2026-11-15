//! Command palette and Quick Open: fuzzy-match a query against a list.
//!
//! Quick Open (Ctrl+P) searches file names; the command palette
//! (Ctrl+Shift+P) searches command names. Both use the same matcher:
//! the query's characters must appear in order in the candidate
//! (subsequence match), and candidates are ranked by how tight the
//! match is — matches at the start and matches with fewer gaps rank
//! higher.

/// One searchable entry: a command or a file.
#[derive(Debug, Clone)]
pub struct PaletteEntry {
    /// Text shown in the list.
    pub title: String,
    /// Smaller text shown under the title (for example a file path).
    pub detail: String,
    /// Identifier the UI acts on (a command name or a file path).
    pub action: String,
}

/// Score a subsequence match: `Some(score)` when every character of
/// `query` appears in `candidate` in order (case-insensitive), with a
/// higher score for tighter, earlier matches. `None` when there is no
/// match. An empty query matches everything with score 0.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let query: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    let candidate_chars: Vec<char> = candidate.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut candidate_index = 0;
    let mut score: i64 = 0;
    let mut first_match: Option<usize> = None;
    let mut previous_match: Option<usize> = None;
    for wanted in &query {
        let mut found = None;
        while candidate_index < candidate_chars.len() {
            if candidate_chars[candidate_index] == *wanted {
                found = Some(candidate_index);
                candidate_index += 1;
                break;
            }
            candidate_index += 1;
        }
        let matched = found?;
        if first_match.is_none() {
            first_match = Some(matched);
            // Matches at the start of the candidate are better.
            score += 100 - matched as i64;
        }
        if let Some(previous) = previous_match {
            // Consecutive characters are better than gappy matches.
            let gap = matched - previous;
            score += if gap == 1 {
                10
            } else {
                -((gap as i64) - 1) * 2
            };
        }
        previous_match = Some(matched);
    }
    Some(score)
}

/// Filter `entries` by `query` and return them best-first.
/// Matching is done against the title, then the detail.
pub fn filter_entries<'a>(query: &str, entries: &'a [PaletteEntry]) -> Vec<&'a PaletteEntry> {
    let mut scored: Vec<(i64, &PaletteEntry)> = entries
        .iter()
        .filter_map(|entry| {
            fuzzy_score(query, &entry.title)
                .or_else(|| fuzzy_score(query, &entry.detail))
                .map(|score| (score, entry))
        })
        .collect();
    // Stable sort keeps the original order for equal scores.
    scored.sort_by(|left, right| right.0.cmp(&left.0));
    scored.into_iter().map(|(_, entry)| entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matching_ranks_tight_matches_first() {
        assert!(fuzzy_score("qp", "quick_open").is_some());
        assert!(fuzzy_score("qp", "quit").is_none());
        let tight = fuzzy_score("tab", "tab_close").unwrap();
        let loose = fuzzy_score("tab", "toggle_file_tree_and_tab").unwrap();
        assert!(tight > loose);
    }

    #[test]
    fn japanese_query_matches() {
        assert!(fuzzy_score("みか", "みかのメモ").is_some());
        assert!(fuzzy_score("メモ", "みかのメモ").is_some());
    }

    #[test]
    fn filter_returns_best_first() {
        let entries = vec![
            PaletteEntry {
                title: "toggle_file_tree".to_string(),
                detail: String::new(),
                action: "toggle_file_tree".to_string(),
            },
            PaletteEntry {
                title: "tab_close".to_string(),
                detail: String::new(),
                action: "tab_close".to_string(),
            },
        ];
        let results = filter_entries("tab", &entries);
        assert_eq!(results[0].title, "tab_close");
    }
}
