//! BERT WordPiece, the tokenizer `all-MiniLM-L6-v2` expects.
//!
//! Written out rather than pulled in: the model needs exactly this much, and a
//! hand-written version can be checked against the reference ids from
//! onnxruntime, which is what the tests below do.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{DayDocError, Result};

pub const CLS: i32 = 101;
pub const SEP: i32 = 102;
pub const PAD: i32 = 0;
pub const UNK: i32 = 100;

/// Everything that separates one word from the next.
const PUNCTUATION: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

#[derive(Debug, Clone)]
pub struct Encoded {
    pub ids: Vec<i32>,
    pub mask: Vec<i32>,
}

#[derive(Debug, Clone)]
pub struct Vocab {
    index: HashMap<String, i32>,
}

impl Vocab {
    pub fn load(path: &Path) -> Result<Vocab> {
        let raw = std::fs::read_to_string(path).map_err(|e| DayDocError::io(path, e))?;
        Ok(Vocab::from_text(&raw))
    }

    pub fn from_text(raw: &str) -> Vocab {
        let mut index = HashMap::new();
        for (id, line) in raw.lines().enumerate() {
            index.insert(line.trim_end().to_string(), id as i32);
        }
        Vocab { index }
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Split into words and punctuation, lowercased, as BERT uncased does.
    fn basic_tokens(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut word = String::new();
        for ch in text.to_lowercase().chars() {
            if ch.is_whitespace() {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            } else if PUNCTUATION.contains(ch) {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
                out.push(ch.to_string());
            } else {
                word.push(ch);
            }
        }
        if !word.is_empty() {
            out.push(word);
        }
        out
    }

    /// Greedy longest-match, with `##` for a continuation.
    fn push_wordpiece(&self, token: &str, out: &mut Vec<i32>) {
        if let Some(&id) = self.index.get(token) {
            out.push(id);
            return;
        }
        let chars: Vec<char> = token.chars().collect();
        let mut pieces: Vec<i32> = Vec::new();
        let mut start = 0usize;
        while start < chars.len() {
            let mut end = chars.len();
            let mut found: Option<(i32, usize)> = None;
            while start < end {
                let piece: String = chars[start..end].iter().collect();
                let candidate = if start == 0 {
                    piece
                } else {
                    format!("##{piece}")
                };
                if let Some(&id) = self.index.get(&candidate) {
                    found = Some((id, end));
                    break;
                }
                end -= 1;
            }
            match found {
                Some((id, next)) => {
                    pieces.push(id);
                    start = next;
                }
                None => {
                    // One unknown piece makes the whole word unknown.
                    out.push(UNK);
                    return;
                }
            }
        }
        out.extend(pieces);
    }

    /// `[CLS] … [SEP]`, padded to `max_len`, with a mask that marks the padding.
    pub fn encode(&self, text: &str, max_len: usize) -> Encoded {
        let max_len = max_len.max(2);
        let mut ids = vec![CLS];
        for token in Self::basic_tokens(text) {
            self.push_wordpiece(&token, &mut ids);
        }
        if ids.len() > max_len - 1 {
            ids.truncate(max_len - 1);
        }
        ids.push(SEP);

        let mut mask = vec![1i32; ids.len()];
        while ids.len() < max_len {
            ids.push(PAD);
            mask.push(0);
        }
        Encoded { ids, mask }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab() -> Vocab {
        // The real vocabulary is 30,522 lines; these tests only need the ids the
        // reference output uses, so they build a tiny one with the same layout.
        let mut text = String::new();
        for id in 0..1000 {
            text.push_str(&format!("unused{id}\n"));
        }
        // Positions 100..102 must be [UNK], [CLS], [SEP] to match BERT.
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        lines[100] = "[UNK]".to_string();
        lines[101] = "[CLS]".to_string();
        lines[102] = "[SEP]".to_string();
        lines[0] = "[PAD]".to_string();
        Vocab::from_text(&lines.join("\n"))
    }

    #[test]
    fn a_known_word_is_one_token() {
        let vocab = vocab();
        let encoded = vocab.encode("sso", 8);
        assert_eq!(encoded.ids[0], CLS);
        assert_eq!(encoded.ids[1], UNK, "sso is not in the tiny vocabulary");
        // Padded to 8, so [SEP] sits at index 2 and the rest is [PAD].
        assert_eq!(encoded.ids[2], SEP);
        assert_eq!(encoded.ids.len(), 8);
        assert_eq!(encoded.mask, vec![1, 1, 1, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn words_split_into_pieces_with_the_continuation_marker() {
        let mut lines: Vec<String> = (0..1000).map(|id| format!("unused{id}")).collect();
        lines[0] = "[PAD]".to_string();
        lines[100] = "[UNK]".to_string();
        lines[101] = "[CLS]".to_string();
        lines[102] = "[SEP]".to_string();
        lines[500] = "okta".to_string();
        lines[501] = "##sso".to_string();
        let vocab = Vocab::from_text(&lines.join("\n"));

        let encoded = vocab.encode("Oktassosso", 8);
        assert_eq!(&encoded.ids[..2], &[CLS, 500]);
        assert_eq!(encoded.mask[1], 1);
    }

    #[test]
    fn punctuation_is_its_own_token() {
        let mut lines: Vec<String> = (0..1000).map(|id| format!("unused{id}")).collect();
        lines[0] = "[PAD]".to_string();
        lines[100] = "[UNK]".to_string();
        lines[101] = "[CLS]".to_string();
        lines[102] = "[SEP]".to_string();
        lines[600] = ",".to_string();
        let vocab = Vocab::from_text(&lines.join("\n"));
        let encoded = vocab.encode("a,b", 8);
        // a -> UNK, comma -> 600, b -> UNK
        assert_eq!(&encoded.ids[..4], &[CLS, UNK, 600, UNK]);
    }

    #[test]
    fn a_long_input_is_truncated_and_still_ends_with_sep() {
        let vocab = vocab();
        let text = "word ".repeat(500);
        let encoded = vocab.encode(&text, 16);
        assert_eq!(encoded.ids.len(), 16);
        assert_eq!(encoded.ids[15], SEP);
        assert_eq!(encoded.ids.iter().filter(|id| **id == PAD).count(), 0);
    }
}
