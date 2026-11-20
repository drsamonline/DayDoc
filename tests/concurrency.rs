//! Concurrent writers, and readers that must never see a half-written note.
//!
//! DayDoc is a file-based app that a CLI, a watcher and a phone can drive at the
//! same time. The write-ahead log and the rename are what make that safe; these
//! tests are the proof, not the assumption.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use daydoc::Vault;

fn scratch(name: &str) -> Vault {
    let base = std::env::temp_dir().join(format!("daydoc-conc-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("vault");
    std::fs::create_dir_all(&root).expect("vault dir");
    Vault::with_state(&root, &base.join("state")).expect("open vault")
}

fn payload(writer: usize, round: usize) -> String {
    // A marker at both ends: a torn write would show one without the other.
    format!(
        "---\nid: 01{:020}\ntype: idea\ntitle: w{writer} r{round}\n---\nstart-{writer}-{round}\n{}\nend-{writer}-{round}\n",
        writer * 1000 + round,
        "body line\n".repeat(20)
    )
}

#[test]
fn many_writers_to_different_notes_all_land_intact() {
    let vault = Arc::new(scratch("different"));
    let barrier = Arc::new(Barrier::new(8));
    let mut handles = Vec::new();

    for writer in 0..8 {
        let vault = Arc::clone(&vault);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for round in 0..10 {
                let rel = format!("inbox/w{writer}-{round}.md");
                vault
                    .write(&rel, &payload(writer, round))
                    .expect("write under contention");
            }
        }));
    }
    for handle in handles {
        handle.join().expect("thread");
    }

    for writer in 0..8 {
        for round in 0..10 {
            let rel = format!("inbox/w{writer}-{round}.md");
            let text = vault.read(&rel).expect("read back");
            assert_eq!(text, payload(writer, round), "{rel} is not what was written");
        }
    }
    // Every staged write committed and was cleaned up.
    let wal = std::fs::read_dir(vault.wal_dir())
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(wal, 0, "staged writes left behind");
}

#[test]
fn many_writers_to_one_note_never_mix_payloads() {
    let vault = Arc::new(scratch("same"));
    let barrier = Arc::new(Barrier::new(6));
    let mut handles = Vec::new();

    for writer in 0..6 {
        let vault = Arc::clone(&vault);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for round in 0..6 {
                vault
                    .write("inbox/shared.md", &payload(writer, round))
                    .expect("write");
            }
        }));
    }
    for handle in handles {
        handle.join().expect("thread");
    }

    let text = vault.read("inbox/shared.md").expect("read");
    let mut valid = false;
    for writer in 0..6 {
        for round in 0..6 {
            if text == payload(writer, round) {
                valid = true;
            }
        }
    }
    assert!(valid, "the file is a mix of two writers:\n{text}");
    // And it still parses as a note.
    let note = vault.load("inbox/shared.md").expect("parse");
    assert_eq!(note.frontmatter.note_type, "idea");
}

#[test]
fn a_reader_never_sees_a_half_written_note() {
    let vault = Arc::new(scratch("torn"));
    let writing = Arc::new(AtomicBool::new(true));
    // Two participants: the writer and the reader. A third would deadlock.
    let barrier = Arc::new(Barrier::new(2));

    let writer = {
        let vault = Arc::clone(&vault);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            for round in 0..20 {
                vault
                    .write("inbox/hot.md", &payload(1, round))
                    .expect("write");
            }
        })
    };

    let reader = {
        let vault = Arc::clone(&vault);
        let writing = Arc::clone(&writing);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            // Every complete payload this writer can produce.
            let expected: Vec<String> = (0..20).map(|round| payload(1, round)).collect();
            let mut reads = 0usize;
            while writing.load(Ordering::Relaxed) || reads < 5 {
                if let Ok(text) = vault.read("inbox/hot.md") {
                    reads += 1;
                    // The only correct outcome: what was read is exactly one of
                    // the payloads, never a mixture and never a fragment.
                    assert!(
                        expected.iter().any(|payload| payload == &text),
                        "torn read:\n{text}"
                    );
                }
            }
            reads
        })
    };

    writer.join().expect("writer");
    writing.store(false, Ordering::Relaxed);
    let reads = reader.join().expect("reader");
    assert!(reads > 0, "the reader never read anything");
}

#[test]
fn a_write_and_a_delete_of_different_notes_do_not_interfere() {
    let vault = Arc::new(scratch("mixed"));
    vault
        .write("inbox/gone.md", &payload(9, 0))
        .expect("seed");
    let barrier = Arc::new(Barrier::new(2));

    let writer = {
        let vault = Arc::clone(&vault);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            for round in 0..10 {
                vault
                    .write("inbox/stays.md", &payload(2, round))
                    .expect("write");
            }
        })
    };

    let deleter = {
        let vault = Arc::clone(&vault);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            vault.trash("inbox/gone.md").expect("trash")
        })
    };

    writer.join().expect("writer");
    let trashed = deleter.join().expect("deleter");
    assert!(trashed.starts_with("_trash/"));
    assert!(!vault.exists("inbox/gone.md"));
    assert!(vault.exists("inbox/stays.md"));
}
