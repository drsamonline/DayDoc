//! TextRank, extractive. No model, no network: the summary is sentences the
//! user already wrote, ranked by how much of the rest of the text they connect
//! to.

use std::collections::BTreeSet;

const DAMPING: f32 = 0.85;
const ITERATIONS: usize = 30;

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?') {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                out.push(trimmed);
            }
            current.clear();
        }
    }
    let tail = current.trim().to_string();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn words(sentence: &str) -> BTreeSet<String> {
    sentence
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(|w| w.to_string())
        .collect()
}

fn similarity(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let overlap = a.intersection(b).count() as f32;
    if overlap == 0.0 {
        return 0.0;
    }
    let denom = (a.len() as f32).ln() + (b.len() as f32).ln();
    if denom <= 0.0 {
        0.0
    } else {
        overlap / denom
    }
}

/// Rank sentences by TextRank and return the top `max_sentences` in reading
/// order.
pub fn extractive_summary(text: &str, max_sentences: usize) -> String {
    let sentences = split_sentences(text);
    if sentences.len() <= max_sentences {
        return sentences.join(" ");
    }

    let sets: Vec<BTreeSet<String>> = sentences.iter().map(|s| words(s)).collect();
    let n = sentences.len();
    let mut weights = vec![vec![0.0f32; n]; n];
    for i in 0..n {
        for j in 0..n {
            if i != j {
                weights[i][j] = similarity(&sets[i], &sets[j]);
            }
        }
    }

    let mut scores = vec![1.0f32 / n as f32; n];
    for _ in 0..ITERATIONS {
        let mut next = vec![(1.0 - DAMPING) / n as f32; n];
        for i in 0..n {
            let total: f32 = weights[i].iter().sum();
            if total <= 0.0 {
                continue;
            }
            for j in 0..n {
                if i == j {
                    continue;
                }
                next[j] += DAMPING * scores[i] * weights[i][j] / total;
            }
        }
        scores = next;
    }

    let mut ranked: Vec<(usize, f32)> = scores.iter().copied().enumerate().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut chosen: Vec<usize> = ranked
        .iter()
        .take(max_sentences)
        .map(|(i, _)| *i)
        .collect();
    chosen.sort_unstable();
    chosen
        .iter()
        .map(|&i| sentences[i].clone())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_returned_whole() {
        assert_eq!(extractive_summary("One. Two.", 5), "One. Two.");
    }

    #[test]
    fn summary_prefers_connected_sentences() {
        let text = "The pricing review covered three vendors. \
                    Okta was the cheapest option for SSO across products. \
                    Entra ID required an enterprise agreement we do not have. \
                    The cafeteria closes at four. \
                    Okta also offered admin tooling that Entra did not.";
        let summary = extractive_summary(text, 2);
        assert!(summary.len() < text.len());
        assert!(summary.to_ascii_lowercase().contains("okta"));
    }

    #[test]
    fn never_returns_more_than_asked() {
        let text = "A one. B two. C three. D four. E five.";
        let summary = extractive_summary(text, 2);
        assert!(split_sentences(&summary).len() <= 2);
    }
}
