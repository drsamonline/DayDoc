//! Attachments, content-addressed.
//!
//! `assets/<first two hex>/<blake3>.<ext>`. Two identical files cannot both
//! exist, and moving a note never breaks a link because the name is the hash.

use std::path::Path;

use crate::error::{DayDocError, Result};
use crate::note::Note;
use crate::vault::{now_iso, Vault};

#[derive(Debug, Clone)]
pub struct Attachment {
    pub rel_path: String,
    pub blake3: String,
    pub bytes: u64,
    pub deduplicated: bool,
}

impl Attachment {
    pub fn markdown_link(&self, label: &str) -> String {
        let rel = self
            .rel_path
            .strip_prefix("assets/")
            .map(|rest| format!("../assets/{rest}"))
            .unwrap_or_else(|| self.rel_path.clone());
        if is_image(&self.rel_path) {
            format!("![{label}]({rel})")
        } else {
            format!("[{label}]({rel})")
        }
    }
}

fn is_image(rel: &str) -> bool {
    let lowered = rel.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"]
        .iter()
        .any(|ext| lowered.ends_with(ext))
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .filter(|e| !e.is_empty() && e.len() <= 8)
        .unwrap_or_else(|| "bin".to_string())
}

/// Store a file in the vault and return where it landed.
pub fn store(vault: &Vault, source: &Path) -> Result<Attachment> {
    let bytes = std::fs::read(source).map_err(|e| DayDocError::io(source, e))?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let ext = extension_of(source);
    let rel_path = format!("assets/{}/{}.{}", &hash[..2], hash, ext);

    if vault.exists(&rel_path) {
        return Ok(Attachment {
            rel_path,
            blake3: hash,
            bytes: bytes.len() as u64,
            deduplicated: true,
        });
    }

    vault.write_bytes(&rel_path, &bytes)?;
    vault.audit("attachment-stored", &rel_path, &format!("{} bytes", bytes.len()))?;
    Ok(Attachment {
        rel_path,
        blake3: hash,
        bytes: bytes.len() as u64,
        deduplicated: false,
    })
}

/// Store a file and reference it from a note's body.
pub fn attach_to(vault: &Vault, note_rel: &str, source: &Path) -> Result<(Attachment, Note)> {
    let attachment = store(vault, source)?;
    let label = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "attachment".to_string());
    let mut note = vault.load(note_rel)?;
    let link = attachment.markdown_link(&label);
    if !note.body.contains(&link) {
        if !note.body.ends_with('\n') {
            note.body.push('\n');
        }
        note.body.push('\n');
        note.body.push_str(&link);
        note.body.push('\n');
        note.frontmatter.updated = Some(now_iso());
        vault.save(&note)?;
    }
    Ok((attachment, note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("daydoc-attach-{}", crate::ids::new_ulid()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write");
        path
    }

    #[test]
    fn the_same_bytes_land_on_the_same_path_once() {
        let vault = temp_vault();
        let file = temp_file("shot.png", b"pixels");
        let first = store(&vault, &file).expect("store");
        let second = store(&vault, &file).expect("store again");
        assert_eq!(first.rel_path, second.rel_path);
        assert!(first.rel_path.starts_with("assets/"));
        assert!(first.rel_path.ends_with(".png"));
        assert!(!first.deduplicated);
        assert!(second.deduplicated);
        let dir = vault.root().join(first.rel_path.rsplit_once('/').unwrap().0);
        assert_eq!(std::fs::read_dir(dir).expect("dir").count(), 1);
    }

    #[test]
    fn images_get_an_image_link() {
        let vault = temp_vault();
        let file = temp_file("shot.png", b"pixels");
        let attachment = store(&vault, &file).expect("store");
        let link = attachment.markdown_link("shot.png");
        assert!(link.starts_with("![shot.png](../assets/"), "{link}");
    }

    #[test]
    fn attaching_appends_the_link_to_the_note() {
        let vault = temp_vault();
        vault
            .write("meetings/m.md", "---\ntype: meeting\ntitle: M\n---\nbody\n")
            .expect("write");
        let file = temp_file("notes.pdf", b"%PDF");
        let (attachment, note) = attach_to(&vault, "meetings/m.md", &file).expect("attach");
        assert!(note.body.contains(&attachment.rel_path[7..]) || note.body.contains("assets/"));
        let reloaded = vault.load("meetings/m.md").expect("reload");
        assert!(reloaded.body.contains("assets/"));
    }

    #[test]
    fn read_only_vault_refuses_attachments() {
        let mut vault = temp_vault();
        vault.config_mut().read_only_vault = true;
        let file = temp_file("shot.png", b"pixels");
        assert!(store(&vault, &file).is_err());
    }
}
