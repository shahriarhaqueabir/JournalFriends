#![allow(dead_code)] // Some keyword utilities are retained for future ranking strategies.

use std::collections::HashSet;
use std::sync::LazyLock;

static STOP_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let words = [
        "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by",
        "from", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had", "do",
        "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can", "need",
        "dare", "ought", "used", "i", "you", "he", "she", "it", "we", "they", "me", "him", "her",
        "us", "them", "my", "your", "his", "its", "our", "their", "mine", "yours", "hers", "ours",
        "theirs", "this", "that", "these", "those", "some", "any", "no", "every", "each", "all",
        "both", "few", "many", "much", "several", "not", "so", "very", "too", "quite", "just",
        "about", "then", "now", "here", "there", "when", "where", "why", "how", "what", "which",
        "who", "whom", "whose",
    ];
    words.iter().cloned().collect()
});

/// Extract meaningful keywords from text, filtering stop words
pub fn extract_keywords(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
        .map(|w| w.trim().to_lowercase())
        .filter(|w| w.len() > 2 && !STOP_WORDS.contains(w.as_str()))
        .collect()
}

/// Return distinct search terms in first-seen order, capped to bound the
/// number of journal queries performed for one chat message.
pub fn unique_keywords(text: &str, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    extract_keywords(text)
        .into_iter()
        .filter(|word| seen.insert(word.clone()))
        .take(limit)
        .collect()
}

/// Extract keywords with frequency counts, sorted by frequency descending
pub fn extract_keywords_with_frequency(text: &str) -> Vec<(String, usize)> {
    let mut freq: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for kw in extract_keywords(text) {
        *freq.entry(kw).or_insert(0) += 1;
    }
    let mut result: Vec<_> = freq.into_iter().collect();
    result.sort_by_key(|b| std::cmp::Reverse(b.1));
    result
}

/// Compute a Jaccard similarity score between two text blocks based on keyword overlap
pub fn keyword_similarity(a: &str, b: &str) -> f64 {
    let ka: HashSet<String> = extract_keywords(a).into_iter().collect();
    let kb: HashSet<String> = extract_keywords(b).into_iter().collect();
    if ka.is_empty() || kb.is_empty() {
        return 0.0;
    }
    let intersection = ka.intersection(&kb).count() as f64;
    let union = ka.union(&kb).count() as f64;
    intersection / union
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_keywords_deduplicates_in_order_and_obeys_limit() {
        assert_eq!(
            unique_keywords("journal habits journal goals tasks notes plans", 5),
            vec!["journal", "habits", "goals", "tasks", "notes"]
        );
    }
}
