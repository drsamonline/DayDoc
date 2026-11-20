//! Hostile input, through the parsers a user's editor can reach.
//!
//! A notes app reads files that other programs wrote, edited and half-saved.
//! None of these inputs may panic, and each must produce an answer a caller can
//! act on: a note, or an error it can report.

use daydoc::calendar;
use daydoc::frontmatter::{self, Frontmatter};
use daydoc::note::Note;

fn parses(raw: &str) -> Result<Note, String> {
    Note::parse("inbox/x.md", raw).map_err(|err| err.to_string())
}

#[test]
fn empty_and_near_empty_files_are_safe() {
    for raw in [
        "",
        "\n",
        "   \n\n",
        "---",
        "---\n",
        "---\n---",
        "---\n---\n",
        "\u{feff}",
        "\0\0\0",
    ] {
        // Either a note or a clean error; never a panic.
        let _ = parses(raw);
        let _ = frontmatter::split(raw);
    }
}

#[test]
fn a_byte_order_mark_does_not_hide_the_frontmatter() {
    let (block, body) = frontmatter::split("\u{feff}---\nid: 01A\ntype: idea\n---\nbody\n").expect("split");
    assert!(block.is_some(), "the BOM swallowed the opening fence");
    assert_eq!(body.trim(), "body");
}

#[test]
fn windows_line_endings_parse_the_same_as_unix() {
    let unix = "---\nid: 01A\ntype: idea\ntitle: Same\n---\nbody\n";
    let dos = "---\r\nid: 01A\r\ntype: idea\r\ntitle: Same\r\n---\r\nbody\r\n";
    let a = parses(unix).expect("unix");
    let b = parses(dos).expect("dos");
    assert_eq!(a.frontmatter.id, b.frontmatter.id);
    assert_eq!(a.frontmatter.title, b.frontmatter.title);
    assert_eq!(a.body.trim(), b.body.trim());
}

#[test]
fn an_unclosed_frontmatter_is_an_error_not_a_panic() {
    let err = parses("---\nid: 01A\nno closing fence\n").expect_err("should refuse");
    assert!(err.to_lowercase().contains("yaml") || err.to_lowercase().contains("closed"), "{err}");
}

#[test]
fn broken_yaml_is_an_error_with_a_reason() {
    for raw in [
        "---\nid: [unclosed\n---\nbody\n",
        "---\n- just\n- a list\n---\nbody\n",
        "---\nid: \"unterminated\n---\nbody\n",
        "---\n\tid: tab-indented\n---\nbody\n",
    ] {
        let outcome = parses(raw);
        assert!(outcome.is_err(), "accepted broken yaml: {raw:?}");
        assert!(!outcome.unwrap_err().is_empty(), "an error with no message");
    }
}

#[test]
fn a_body_containing_fences_is_not_reparsed() {
    let raw = "---\nid: 01A\ntype: idea\n---\nbefore\n---\nafter\n";
    let note = parses(raw).expect("parse");
    assert_eq!(note.frontmatter.id, "01A");
    assert!(note.body.contains("after"), "{}", note.body);
}

#[test]
fn duplicate_and_unknown_keys_keep_the_last_value_and_the_extras() {
    let fm = Frontmatter::parse("id: 01A\nid: 01B\ncustom: kept\n").expect("parse");
    assert_eq!(fm.id, "01B");
    assert_eq!(fm.extra.get("custom").map(String::as_str), Some("kept"));
}

#[test]
fn unicode_keys_and_values_round_trip() {
    let mut fm = Frontmatter::default();
    fm.id = "01A".to_string();
    fm.note_type = "idea".to_string();
    fm.title = "ठरलं: ओक्टा वापरायचं".to_string();
    fm.people = vec!["प्रिया".to_string()];
    fm.extra.insert("टिप्पणी".to_string(), "मराठी".to_string());

    let rendered = fm.render();
    let back = Frontmatter::parse(&rendered).expect("reparse");
    assert_eq!(back.title, fm.title);
    assert_eq!(back.people, fm.people);
    assert_eq!(back.extra.get("टिप्पणी"), fm.extra.get("टिप्पणी"));
}

#[test]
fn a_very_long_line_does_not_blow_up() {
    let long = "x".repeat(200_000);
    let raw = format!("---\nid: 01A\ntype: idea\ntitle: {long}\n---\n{long}\n");
    let note = parses(&raw).expect("parse");
    assert_eq!(note.frontmatter.title.len(), long.len());
}

#[test]
fn binary_noise_is_refused_without_panicking() {
    let noise: String = (0u8..=255).map(|byte| byte as char).collect();
    let _ = parses(&noise);
    let _ = Frontmatter::parse(&noise);
    let _ = calendar::parse(&noise);
}

#[test]
fn the_calendar_parser_survives_malformed_files() {
    let cases = [
        "",
        "BEGIN:VCALENDAR\n",
        "BEGIN:VEVENT\n",                        // never closed
        "END:VEVENT\n",                          // closed without opening
        "BEGIN:VEVENT\nEND:VEVENT\n",            // no summary
        "BEGIN:VEVENT\nSUMMARY:\nEND:VEVENT\n",  // empty summary
        "BEGIN:VEVENT\nDTSTART:\nEND:VEVENT\n",  // empty date
        "BEGIN:VEVENT\nSUMMARY:A\nDTSTART:99999999999999\nEND:VEVENT\n",
        "SUMMARY:outside any event\n",
        "BEGIN:VEVENT\nSUMMARY:A\n  folded\n\tfolded again\nDTSTART:20261120\nEND:VEVENT\n",
    ];
    for raw in cases {
        let events = calendar::parse(raw);
        // Whatever comes back, every event must be usable.
        for event in events {
            assert!(!event.summary.is_empty(), "empty summary from {raw:?}");
            if let Some(date) = event.date {
                assert_eq!(date.len(), 10, "malformed date {date:?} from {raw:?}");
            }
        }
    }
}

#[test]
fn a_calendar_with_crlf_and_a_folded_summary_still_reads() {
    let raw = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:Atlas migration\r\n review\r\nDTSTART;TZID=Asia/Kolkata:20261120T090000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = calendar::parse(raw);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].date.as_deref(), Some("2026-11-20"));
    assert!(events[0].summary.contains("Atlas migration"));
}
