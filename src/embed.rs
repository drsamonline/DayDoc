//! Embeddings, and the swap point for the real model.
//!
//! DayDoc ships no model yet. What ships is the pipeline: a deterministic
//! embedder, an on-disk vector store and cosine ranking, all behind one trait.
//! Dropping in all-MiniLM-L6-v2 later means implementing `Embedder` in this file
//! and nothing else — the storage, the ranking and the auto-link suggestions do
//! not change.
//!
//! The bundled `HashEmbedder` is a signed feature-hash bag of words. It is not a
//! semantic model and is not advertised as one: it finds notes that share
//! vocabulary, which is what the index already does, so auto-linking is
//! deliberately *not* enabled on it by default (see `link.rs`).

use rusqlite::params;

use crate::error::{DayDocError, Result};
use crate::vault::Vault;

pub const DEFAULT_DIMS: usize = 256;
pub const MIN_SIMILARITY: f32 = 0.55;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS note_vectors (
    rel_path TEXT PRIMARY KEY,
    dims INTEGER NOT NULL,
    vec BLOB NOT NULL
);";

pub trait Embedder {
    fn dimensions(&self) -> usize;
    fn embed(&self, text: &str) -> Vec<f32>;
    /// Whether this embedder understands meaning, or only vocabulary.
    fn is_semantic(&self) -> bool {
        false
    }
}

/// Deterministic, dependency-free, and honest about what it is.
#[derive(Debug, Clone, Copy)]
pub struct HashEmbedder {
    dims: usize,
}

impl HashEmbedder {
    pub fn new(dims: usize) -> HashEmbedder {
        HashEmbedder {
            dims: dims.max(8),
        }
    }
}

impl Default for HashEmbedder {
    fn default() -> Self {
        HashEmbedder::new(DEFAULT_DIMS)
    }
}

fn tokens(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 3)
        .map(|word| word.to_string())
        .collect()
}

impl Embedder for HashEmbedder {
    fn dimensions(&self) -> usize {
        self.dims
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0f32; self.dims];
        for token in tokens(text) {
            let digest = blake3::hash(token.as_bytes());
            let bytes = digest.as_bytes();
            let index = (u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize) % self.dims;
            // The sign bit keeps unrelated words from always adding up.
            let sign = if bytes[8] & 1 == 0 { 1.0 } else { -1.0 };
            vector[index] += sign;
        }
        normalise(&mut vector);
        vector
    }
}

fn normalise(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

/// Cosine similarity, for vectors that are already unit length or not.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

fn to_blob(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn from_blob(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Store one note's vector, replacing any earlier one.
pub fn store(vault: &Vault, rel_path: &str, vector: &[f32]) -> Result<()> {
    let conn = vault.db()?;
    conn.execute_batch(SCHEMA)?;
    conn.execute(
        "INSERT INTO note_vectors (rel_path, dims, vec) VALUES (?1, ?2, ?3)
         ON CONFLICT(rel_path) DO UPDATE SET dims = excluded.dims, vec = excluded.vec",
        params![rel_path, vector.len() as i64, to_blob(vector)],
    )
    .map_err(DayDocError::Db)?;
    Ok(())
}

/// Every stored vector.
pub fn all(vault: &Vault) -> Result<Vec<(String, Vec<f32>)>> {
    let conn = vault.db()?;
    conn.execute_batch(SCHEMA)?;
    let mut statement = conn
        .prepare("SELECT rel_path, vec FROM note_vectors")
        .map_err(DayDocError::Db)?;
    let rows = statement
        .query_map([], |row| {
            let rel: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((rel, from_blob(&blob)))
        })
        .map_err(DayDocError::Db)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(DayDocError::Db)?);
    }
    Ok(out)
}

/// Embed every note in the vault and store the vectors.
pub fn rebuild<E: Embedder>(vault: &Vault, embedder: &E) -> Result<usize> {
    let notes = vault.scan()?.notes;
    let conn = vault.db()?;
    conn.execute_batch(SCHEMA)?;
    conn.execute("DELETE FROM note_vectors", [])
        .map_err(DayDocError::Db)?;
    drop(conn);

    let mut count = 0usize;
    for note in &notes {
        let vector = embedder.embed(&note.search_text());
        store(vault, &note.rel_path, &vector)?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn the_same_text_always_gives_the_same_vector() {
        let embedder = HashEmbedder::default();
        let first = embedder.embed("we decided to adopt okta for sso");
        let second = embedder.embed("we decided to adopt okta for sso");
        assert_eq!(first, second);
        assert_eq!(first.len(), DEFAULT_DIMS);
        let norm: f32 = first.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.001, "not normalised: {norm}");
    }

    #[test]
    fn shared_vocabulary_scores_higher_than_unrelated_text() {
        let embedder = HashEmbedder::default();
        let okta = embedder.embed("okta sso enterprise pilot admin tooling");
        let more_okta = embedder.embed("okta sso pilot admin tooling pricing");
        let payroll = embedder.embed("payroll statutory filing interest penalty");
        assert!(cosine(&okta, &more_okta) > cosine(&okta, &payroll));
    }

    #[test]
    fn vectors_round_trip_through_the_database() {
        let vault = temp_vault();
        let embedder = HashEmbedder::default();
        let vector = embedder.embed("okta sso");
        store(&vault, "inbox/a.md", &vector).expect("store");
        let loaded = all(&vault).expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, "inbox/a.md");
        assert_eq!(loaded[0].1.len(), vector.len());
        assert!((cosine(&loaded[0].1, &vector) - 1.0).abs() < 0.001);
    }

    #[test]
    fn rebuilding_covers_every_note() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01A\ntype: idea\ntitle: Okta\n---\nbody about sso\n")
            .expect("a");
        vault
            .write("inbox/b.md", "---\nid: 01B\ntype: idea\ntitle: Payroll\n---\nbody about tax\n")
            .expect("b");
        let count = rebuild(&vault, &HashEmbedder::default()).expect("rebuild");
        assert_eq!(count, 2);
        assert_eq!(all(&vault).expect("load").len(), 2);
    }

    #[test]
    fn cosine_refuses_mismatched_shapes() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0]), 0.0);
        assert_eq!(cosine(&[], &[]), 0.0);
    }
}
