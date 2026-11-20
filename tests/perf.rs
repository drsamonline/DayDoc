//! Performance guards for the two things that get slow first: building the
//! index and searching it.
//!
//! The budgets are deliberately loose — a CI runner is not a laptop, and this is
//! a guard against an order-of-magnitude regression, not a benchmark. Notes are
//! written straight to disk so the measurement is of the index and the search,
//! not of 500 fsyncs.

use std::time::Instant;

use daydoc::Vault;

fn scratch_vault(notes: usize) -> Vault {
    let base = std::env::temp_dir().join(format!(
        "daydoc-perf-{}-{}",
        std::process::id(),
        notes
    ));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("vault");
    std::fs::create_dir_all(&root).expect("create vault");
    let vault = Vault::with_state(&root, &base.join("state")).expect("open vault");

    let body = "we decided to adopt okta for sso across all products. \
                the pilot runs for thirty days and covers admin tooling, \
                pricing, and the migration plan for the directory.";
    for index in 0..notes {
        let folder = if index % 7 == 0 { "decisions" } else { "meetings" };
        let path = root.join(folder).join(format!("note-{index:04}.md"));
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        let text = format!(
            "---\nid: 01{:024}\ntype: {}\ntitle: Note {index} about okta sso\n---\n{body}\n",
            index,
            if index % 7 == 0 { "meeting" } else { "meeting" }
        );
        std::fs::write(&path, text).expect("write note");
    }
    vault
}

#[test]
fn five_hundred_notes_index_and_search_within_budget() {
    let notes = 500;
    let vault = scratch_vault(notes);

    let started = Instant::now();
    let indexed = daydoc::index::reindex(&vault).expect("reindex");
    let index_time = started.elapsed();
    assert_eq!(indexed, notes);
    assert!(
        index_time.as_secs_f64() < 5.0,
        "indexing {notes} notes took {index_time:?}"
    );

    let started = Instant::now();
    let hits = daydoc::search::search(&vault, "okta admin tooling", &Default::default())
        .expect("search");
    let search_time = started.elapsed();
    assert!(!hits.is_empty(), "search found nothing");
    assert!(
        search_time.as_secs_f64() < 2.0,
        "searching {notes} notes took {search_time:?}"
    );

    // The index is used when it is fresh, so the second search is the fast path.
    let started = Instant::now();
    let again = daydoc::search::search(&vault, "pricing migration", &Default::default())
        .expect("search");
    let second = started.elapsed();
    assert!(!again.is_empty());
    assert!(
        second.as_secs_f64() < 2.0,
        "second search took {second:?}"
    );

    println!(
        "index {notes} notes: {index_time:?}; search: {search_time:?}; again: {second:?}"
    );
}

#[test]
fn a_stale_scan_over_five_hundred_notes_stays_linear() {
    let vault = scratch_vault(500);
    let started = Instant::now();
    let flags = daydoc::stale::scan(&vault).expect("scan");
    let elapsed = started.elapsed();
    // Nothing here is old enough to flag, and the scan must not be quadratic.
    assert!(flags.is_empty());
    assert!(elapsed.as_secs_f64() < 5.0, "stale scan took {elapsed:?}");
    println!("stale scan over 500 notes: {elapsed:?}");
}
