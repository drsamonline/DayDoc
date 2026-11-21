//! all-MiniLM-L6-v2 against the numbers onnxruntime produces.
//!
//! The reference ids and the first six dimensions were computed with
//! onnxruntime 1.31 on the fp32 export of
//! `sentence-transformers/all-MiniLM-L6-v2`, using mean pooling over the
//! attention mask and L2 normalisation — the same recipe `embed::MiniLm` uses.
//!
//! These tests need the model, which is 86 MB and not in the repository. Run
//! `scripts/fetch-model.sh` first; without it they report that they were skipped
//! rather than passing quietly.

use std::path::PathBuf;

use daydoc::tokenizer::Vocab;

fn model_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("DAYDOC_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("DAYDOC_HOME").unwrap_or_else(|_| {
        let base = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{base}/.daydoc")
    });
    PathBuf::from(home).join("models")
}

fn vocab() -> Option<Vocab> {
    let path = model_dir().join("vocab.txt");
    if !path.is_file() {
        eprintln!("skipped: no vocab.txt in {}", model_dir().display());
        return None;
    }
    Some(Vocab::load(&path).expect("load vocab"))
}

#[test]
fn the_tokenizer_matches_onnxruntime() {
    let Some(vocab) = vocab() else { return };
    let encoded = vocab.encode("we decided to adopt okta for sso", 128);
    assert_eq!(
        &encoded.ids[..11],
        &[101, 2057, 2787, 2000, 11092, 7929, 2696, 2005, 7020, 2080, 102],
        "token ids differ from the reference"
    );
    assert_eq!(encoded.ids[11], 0, "padding should follow [SEP]");
    assert_eq!(encoded.mask.iter().filter(|bit| **bit == 1).count(), 11);

    let encoded = vocab.encode("payroll statutory filing interest penalty", 128);
    assert_eq!(
        &encoded.ids[..7],
        &[101, 26854, 15201, 15242, 3037, 6531, 102]
    );
}

#[cfg(feature = "model")]
#[test]
fn the_embedding_matches_onnxruntime() {
    let Some(vocab) = vocab() else { return };
    let model_path = model_dir().join("model.onnx");
    if !model_path.is_file() {
        eprintln!("skipped: no model.onnx in {}", model_dir().display());
        return;
    }
    let embedder = daydoc::embed::MiniLm::load(&model_path, &model_dir().join("vocab.txt"))
        .expect("load the model");
    assert_eq!(daydoc::embed::Embedder::dimensions(&embedder), 384);

    // Reference: onnxruntime 1.31, fp32 export, mean pool + L2 normalise.
    let reference = [
        -0.029701, 0.064115, 0.014484, 0.044242, -0.025221, 0.011277,
    ];
    let vector = embedder
        .embed_checked("we decided to adopt okta for sso")
        .expect("embed");
    assert_eq!(vector.len(), 384);
    for (index, expected) in reference.iter().enumerate() {
        let got = vector[index];
        assert!(
            (got - expected).abs() < 1e-4,
            "dimension {index}: expected {expected}, got {got}"
        );
    }

    let norm: f32 = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-4, "not normalised: {norm}");

    // And it behaves like a semantic embedder: closer to its own topic than to
    // an unrelated one.
    let payroll = embedder
        .embed_checked("payroll statutory filing interest penalty")
        .expect("embed");
    let okta = embedder
        .embed_checked("okta sso enterprise pilot admin tooling")
        .expect("embed");
    assert!(
        daydoc::embed::cosine(&vector, &okta) > daydoc::embed::cosine(&vector, &payroll)
    );
}
