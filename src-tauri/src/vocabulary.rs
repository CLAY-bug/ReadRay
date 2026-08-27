use crate::learning_records;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::OnceLock;
use tauri::AppHandle;

// 由 scripts/gen-vocabulary.mjs 从 hermitdave/FrequencyWords（MIT 授权，
// OpenSubtitles 语料）生成的纯词表：每行一词，按真实使用频率降序。
const VOCABULARY_DATA: &str = include_str!("vocabulary_data.txt");

const FUZZY_MIN_QUERY_CHARS: usize = 4;
const MAX_FUZZY_EDIT_DISTANCE: usize = 2;
pub(crate) const MAX_SUGGESTION_LIMIT: usize = 10;

fn vocabulary_terms() -> &'static [&'static str] {
    static TERMS: OnceLock<Vec<&'static str>> = OnceLock::new();
    TERMS.get_or_init(|| {
        VOCABULARY_DATA
            .lines()
            .filter(|line| !line.is_empty())
            .collect()
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabularySuggestion {
    pub term: String,
    pub from_history: bool,
}

fn is_suggestable_query(query: &str) -> bool {
    query.chars().count() >= 2
        && !query.contains(char::is_whitespace)
        && query
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '\'' || c == '-')
}

/// 词表补全：前缀命中保持词频序；查询至少 4 个字符且前缀不足时，用受限
/// 编辑距离（≤2）给拼写纠错候选，距离升序、同级保持词频序。
pub fn suggest_vocabulary_terms(query: &str, limit: usize) -> Vec<&'static str> {
    let lowered = query.to_lowercase();
    if !is_suggestable_query(&lowered) {
        return Vec::new();
    }
    suggest_base_terms(&lowered, limit)
}

fn suggest_base_terms(lowered: &str, limit: usize) -> Vec<&'static str> {
    let terms = vocabulary_terms();
    let mut prefix_hits: Vec<&'static str> = terms
        .iter()
        .copied()
        .filter(|term| term.starts_with(lowered))
        .collect();
    if prefix_hits.len() >= limit || lowered.chars().count() < FUZZY_MIN_QUERY_CHARS {
        prefix_hits.truncate(limit);
        return prefix_hits;
    }

    let query_chars: Vec<char> = lowered.chars().collect();
    let prefix_set: HashSet<&str> = prefix_hits.iter().copied().collect();
    let mut fuzzy: Vec<(usize, usize, &'static str)> = terms
        .iter()
        .enumerate()
        .filter(|(_, term)| !prefix_set.contains(**term))
        .filter_map(|(index, term)| {
            // 先做廉价长度剪枝，避免为绝大多数词分配 Vec<char>。
            if term.chars().count().abs_diff(query_chars.len()) > MAX_FUZZY_EDIT_DISTANCE {
                return None;
            }
            let term_chars: Vec<char> = term.chars().collect();
            bounded_edit_distance(&query_chars, &term_chars, MAX_FUZZY_EDIT_DISTANCE)
                .map(|distance| (distance, index, *term))
        })
        .collect();
    fuzzy.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

    for (_, _, term) in fuzzy {
        if prefix_hits.len() >= limit {
            break;
        }
        prefix_hits.push(term);
    }
    prefix_hits
}

/// 带 band 剪枝的编辑距离：超过 max 时返回 None，避免全量 DP。
fn bounded_edit_distance(query: &[char], term: &[char], max: usize) -> Option<usize> {
    if query.len().abs_diff(term.len()) > max {
        return None;
    }

    let mut previous: Vec<usize> = (0..=term.len()).collect();
    for (i, query_char) in query.iter().enumerate() {
        let mut current = vec![usize::MAX; term.len() + 1];
        current[0] = i + 1;
        // j 表示 term 的前 j 个字符；|i - j| <= max 的带外格子不可达。
        let band_start = i.saturating_sub(max).max(1);
        let band_end = (i + max).min(term.len());
        for j in band_start..=band_end {
            let substitution =
                previous[j - 1].saturating_add(usize::from(query_char != &term[j - 1]));
            let deletion = previous[j].saturating_add(1);
            let insertion = current[j - 1].saturating_add(1);
            current[j] = substitution.min(deletion).min(insertion);
        }
        previous = current;
    }

    let distance = previous[term.len()];
    (distance <= max).then_some(distance)
}

/// 输入补全 command：学习记录历史前缀命中优先（最近查过在前），词表补足。
/// 词表侧已前置 is_suggestable_query，这里不再重复校验。
#[tauri::command]
pub fn suggest_vocabulary_terms_command(
    app: AppHandle,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<VocabularySuggestion>, String> {
    let limit = limit.unwrap_or(5).clamp(1, MAX_SUGGESTION_LIMIT);
    let trimmed = query.trim();
    let lowered = trimmed.to_lowercase();
    if !is_suggestable_query(&lowered) {
        return Ok(Vec::new());
    }

    let mut suggestions: Vec<VocabularySuggestion> = Vec::with_capacity(limit);
    let mut seen: HashSet<String> = HashSet::new();

    if let Ok(history) = learning_records::prefix_learning_target_texts(&app, trimmed, limit) {
        for term in history {
            if seen.insert(term.to_lowercase()) {
                suggestions.push(VocabularySuggestion {
                    term,
                    from_history: true,
                });
            }
        }
    }

    for term in suggest_base_terms(&lowered, limit) {
        if suggestions.len() >= limit {
            break;
        }
        if seen.insert(term.to_string()) {
            suggestions.push(VocabularySuggestion {
                term: term.to_string(),
                from_history: false,
            });
        }
    }

    suggestions.truncate(limit);
    Ok(suggestions)
}

#[cfg(test)]
mod tests {
    use super::{bounded_edit_distance, suggest_vocabulary_terms};

    #[test]
    fn vocabulary_terms_are_loaded_in_frequency_order() {
        let terms = super::vocabulary_terms();
        assert!(terms.len() > 40_000);
        assert_eq!(terms[0], "you");
        assert!(terms.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(terms
            .iter()
            .all(|term| term.chars().next().is_some_and(|c| c.is_ascii_lowercase())));
    }

    #[test]
    fn prefix_suggestions_follow_frequency_order() {
        let suggestions = suggest_vocabulary_terms("th", 5);
        assert_eq!(suggestions.len(), 5);
        assert_eq!(suggestions[0], "the");
        assert!(suggestions.iter().all(|term| term.starts_with("th")));
    }

    #[test]
    fn fuzzy_correction_suggests_near_misses() {
        let suggestions = suggest_vocabulary_terms("apreciate", 6);
        assert!(suggestions.contains(&"appreciate"));
        // 前缀命中优先于模糊候选
        let exact = suggest_vocabulary_terms("apprec", 3);
        assert!(exact.iter().all(|term| term.starts_with("apprec")));
    }

    #[test]
    fn reject_unsuggestable_queries() {
        assert!(suggest_vocabulary_terms("", 5).is_empty());
        assert!(suggest_vocabulary_terms("a", 5).is_empty());
        assert!(suggest_vocabulary_terms("hello world", 5).is_empty());
        assert!(suggest_vocabulary_terms("市场", 5).is_empty());
        assert!(suggest_vocabulary_terms("v4", 5).is_empty());
        assert!(suggest_vocabulary_terms("don't", 5).len() > 0);
        assert!(suggest_vocabulary_terms("mother-in-law", 5).len() > 0);
    }

    #[test]
    fn bounded_edit_distance_respects_max() {
        let query: Vec<char> = "apreciate".chars().collect();
        let term: Vec<char> = "appreciate".chars().collect();
        assert_eq!(bounded_edit_distance(&query, &term, 2), Some(1));
        assert_eq!(bounded_edit_distance(&query, &term, 0), None);
        let far: Vec<char> = "zzzzzzzzzzzz".chars().collect();
        assert_eq!(bounded_edit_distance(&query, &far, 2), None);
    }
}
