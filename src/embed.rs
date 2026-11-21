//! Embeddings, and the swap point for the real model.
//!
//! DayDoc ships no model yet. What ships is the pipeline: a deterministic
//! embedder, an on-disk vector store and cosine ranking, all behind one trait.
//! Dropping in all-MiniLM-L6-v2 later means implementing `Embedder` in this file
//! and nothing else — the storage, the ranking and the auto-link suggestions do
//! not change.
//!
//! Two embedders ship:
//!
//! - `MiniLm`, when `model.onnx` and `vocab.txt` are present. all-MiniLM-L6-v2
//!   through `rten`, mean-pooled and L2-normalised, 384 dimensions. This one
//!   understands meaning, and its output is checked against onnxruntime in
//!   `tests/minilm.rs`.
//! - `HashEmbedder`, the fallback: a signed feature-hash bag of words. It finds
//!   notes that share vocabulary, which the index already does, so it says so
//!   (`is_semantic` is false) and the UI never calls it semantic.
//!
//! The model is not bundled: `scripts/fetch-model.sh` downloads it, and
//! `daydoc embed` reports which one is in use.

use rusqlite::{params, Connection};

use crate::error::{DayDocError, Result};
use crate::vault::Vault;

pub const DEFAULT_DIMS: usize = 256;
pub const MINILM_DIMS: usize = 384;
pub const MINILM_MAX_TOKENS: usize = 128;
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

/// Where the model files are looked for: `embed_model_dir`, else `<state>/models`.
pub fn model_dir(vault: &Vault) -> std::path::PathBuf {
    let configured = vault.config().embed_model_dir.trim();
    if configured.is_empty() {
        vault.state_dir().join("models")
    } else {
        std::path::PathBuf::from(configured)
    }
}

/// The two files the model needs, if both are there.
pub fn model_files(vault: &Vault) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let dir = model_dir(vault);
    let model = dir.join("model.onnx");
    let vocab = dir.join("vocab.txt");
    if model.is_file() && vocab.is_file() {
        Some((model, vocab))
    } else {
        None
    }
}

/// The best embedder this vault can use.
pub enum AnyEmbedder {
    #[cfg(feature = "model")]
    MiniLm(MiniLm),
    Hash(HashEmbedder),
}

impl AnyEmbedder {
    pub fn for_vault(vault: &Vault) -> Result<AnyEmbedder> {
        #[cfg(feature = "model")]
        {
            if let Some((model, vocab)) = model_files(vault) {
                return Ok(AnyEmbedder::MiniLm(MiniLm::load(&model, &vocab)?));
            }
        }
        let _ = vault;
        Ok(AnyEmbedder::Hash(HashEmbedder::default()))
    }

    pub fn name(&self) -> &'static str {
        match self {
            #[cfg(feature = "model")]
            AnyEmbedder::MiniLm(_) => "all-MiniLM-L6-v2",
            AnyEmbedder::Hash(_) => "feature hash (no model installed)",
        }
    }
}

impl Embedder for AnyEmbedder {
    fn dimensions(&self) -> usize {
        match self {
            #[cfg(feature = "model")]
            AnyEmbedder::MiniLm(inner) => inner.dimensions(),
            AnyEmbedder::Hash(inner) => inner.dimensions(),
        }
    }

    fn is_semantic(&self) -> bool {
        match self {
            #[cfg(feature = "model")]
            AnyEmbedder::MiniLm(inner) => inner.is_semantic(),
            AnyEmbedder::Hash(inner) => inner.is_semantic(),
        }
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        match self {
            #[cfg(feature = "model")]
            AnyEmbedder::MiniLm(inner) => inner.embed(text),
            AnyEmbedder::Hash(inner) => inner.embed(text),
        }
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

fn store_with(conn: &Connection, rel_path: &str, vector: &[f32]) -> Result<()> {
    conn.execute(
        "INSERT INTO note_vectors (rel_path, dims, vec) VALUES (?1, ?2, ?3)
         ON CONFLICT(rel_path) DO UPDATE SET dims = excluded.dims, vec = excluded.vec",
        params![rel_path, vector.len() as i64, to_blob(vector)],
    )
    .map_err(DayDocError::Db)?;
    Ok(())
}

/// Store one note's vector, replacing any earlier one.
pub fn store(vault: &Vault, rel_path: &str, vector: &[f32]) -> Result<()> {
    let conn = vault.db()?;
    conn.execute_batch(SCHEMA)?;
    store_with(&conn, rel_path, vector)
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
    let mut conn = vault.db()?;
    conn.execute_batch(SCHEMA)?;

    let mut count = 0usize;
    {
        let tx = conn.transaction().map_err(DayDocError::Db)?;
        tx.execute("DELETE FROM note_vectors", [])
            .map_err(DayDocError::Db)?;
        for note in &notes {
            let vector = embedder.embed(&note.search_text());
            store_with(&tx, &note.rel_path, &vector)?;
            count += 1;
        }
        tx.commit().map_err(DayDocError::Db)?;
    }
    Ok(count)
}

/// all-MiniLM-L6-v2 through `rten`.
#[cfg(feature = "model")]
pub struct MiniLm {
    model: rten::Model,
    vocab: crate::tokenizer::Vocab,
    input_ids: rten::NodeId,
    attention_mask: rten::NodeId,
    token_type_ids: rten::NodeId,
    output: rten::NodeId,
}

#[cfg(feature = "model")]
impl MiniLm {
    pub fn load(model_path: &std::path::Path, vocab_path: &std::path::Path) -> Result<MiniLm> {
        let model = rten::Model::load_file(model_path).map_err(|err| {
            DayDocError::Config(format!(
                "could not load the model at {}: {err}",
                model_path.display()
            ))
        })?;
        let vocab = crate::tokenizer::Vocab::load(vocab_path)?;
        let node = |name: &str| {
            model.node_id(name).map_err(|err| {
                DayDocError::Config(format!("the model has no input called {name}: {err}"))
            })
        };
        Ok(MiniLm {
            input_ids: node("input_ids")?,
            attention_mask: node("attention_mask")?,
            token_type_ids: node("token_type_ids")?,
            output: node("last_hidden_state")?,
            model,
            vocab,
        })
    }

    /// Fails loudly, for callers that can report it.
    pub fn embed_checked(&self, text: &str) -> Result<Vec<f32>> {
        use rten_tensor::{Layout, NdTensor};

        let encoded = self.vocab.encode(text, MINILM_MAX_TOKENS);
        let mask = encoded.mask.clone();
        let shape = [1usize, MINILM_MAX_TOKENS];
        let ids = NdTensor::<i32, 2>::from_data(shape, encoded.ids);
        let attention = NdTensor::<i32, 2>::from_data(shape, encoded.mask);
        let types = NdTensor::<i32, 2>::zeros(shape);

        let outputs = self
            .model
            .run(
                vec![
                    (self.input_ids, ids.into()),
                    (self.attention_mask, attention.into()),
                    (self.token_type_ids, types.into()),
                ],
                &[self.output],
                None,
            )
            .map_err(|err| DayDocError::Config(format!("the model failed: {err}")))?;

        let value = outputs
            .into_iter()
            .next()
            .ok_or_else(|| DayDocError::Config("the model returned nothing".to_string()))?;
        // A dynamic-rank view: the output is [1, tokens, 384], and reading it
        // through the view avoids any question of storage layout.
        let hidden = value
            .as_tensor_view::<f32>()
            .ok_or_else(|| DayDocError::Config("the output was not a float tensor".to_string()))?;
        let shape = hidden.shape().to_vec();
        if shape.len() != 3 {
            return Err(DayDocError::Config(format!(
                "expected a 3-dimensional output, got {shape:?}"
            )));
        }
        let dims = shape[2];
        if dims != MINILM_DIMS {
            return Err(DayDocError::Config(format!(
                "expected {MINILM_DIMS} dimensions, got {dims}"
            )));
        }
        let tokens = shape[1].min(mask.len());
        let data = hidden
            .data()
            .ok_or_else(|| DayDocError::Config("the output was not contiguous".to_string()))?;

        // Mean-pool over the tokens the mask keeps, then normalise.
        let mut pooled = vec![0.0f32; dims];
        let mut counted = 0.0f32;
        for position in 0..tokens {
            if mask[position] == 0 {
                continue;
            }
            counted += 1.0;
            let start = position * dims;
            if start + dims > data.len() {
                break;
            }
            for (index, value) in pooled.iter_mut().enumerate() {
                *value += data[start + index];
            }
        }
        if counted > 0.0 {
            for value in pooled.iter_mut() {
                *value /= counted;
            }
        }
        normalise(&mut pooled);
        Ok(pooled)
    }
}

#[cfg(feature = "model")]
impl Embedder for MiniLm {
    fn dimensions(&self) -> usize {
        MINILM_DIMS
    }

    fn is_semantic(&self) -> bool {
        true
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        // The trait is infallible because ranking callers have nowhere to put an
        // error; a failure here is reported by `embed_checked` and by
        // `daydoc embed`, not swallowed silently by them.
        self.embed_checked(text).unwrap_or_else(|_| vec![0.0; MINILM_DIMS])
    }
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
