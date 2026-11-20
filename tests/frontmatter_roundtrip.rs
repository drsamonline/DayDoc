//! The round-trip invariant: what DayDoc writes, DayDoc must read back exactly.
//!
//! Titles, owners and tags come from whatever a user typed, so the corpus here is
//! every shape that breaks a naive YAML emitter: YAML keywords, numbers that look
//! like strings, indicators at the start of a scalar, quotes, colons, hashes,
//! tabs, trailing spaces, and text in scripts that are not Latin.

use daydoc::frontmatter::Frontmatter;

fn sample(value: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    fm.id = "01J8ZZ3WQ4T9R6M2XK7B5N0PVD".to_string();
    fm.note_type = "decision".to_string();
    fm.title = value.to_string();
    fm.owner = Some(value.to_string());
    fm.people = vec![value.to_string(), "second".to_string()];
    fm.tags = vec![value.to_string()];
    fm
}

const CORPUS: &[&str] = &[
    "plain",
    "true",
    "false",
    "yes",
    "no",
    "on",
    "off",
    "null",
    "Null",
    "~",
    "123",
    "1.5",
    "-1",
    "0x1f",
    "1e9",
    "2026-11-20",
    "12:30",
    "- leading dash",
    "-",
    "--",
    "?",
    "?question",
    "!",
    "!bang",
    "*star",
    "&amp",
    "%pct",
    "|pipe",
    ">gt",
    "'single'",
    "\"double\"",
    "has: colon",
    "has # hash",
    "#starts-with-hash",
    "@at",
    "`backtick`",
    "{brace}",
    "[bracket]",
    "a,b",
    "trailing space ",
    " leading space",
    "two  spaces",
    "tab\there",
    "newline\nbreak",
    "carriage\rreturn",
    "ünïcode",
    "मराठी निर्णय",
    "हिन्दी",
    "ગુજરાતી",
    "தமிழ்",
    "🎯 emoji",
    "back\\slash",
    "",
    " ",
    "a".repeat(300).as_str(),
];

#[test]
fn every_awkward_value_survives_render_and_parse() {
    let mut failures: Vec<String> = Vec::new();

    for value in CORPUS {
        let fm = sample(value);
        let rendered = fm.render();
        let parsed = match Frontmatter::parse(&rendered) {
            Ok(parsed) => parsed,
            Err(err) => {
                failures.push(format!("{value:?} did not parse back: {err}\n{rendered}"));
                continue;
            }
        };
        if parsed.title != fm.title {
            failures.push(format!(
                "title changed: {:?} -> {:?}\n{rendered}",
                fm.title, parsed.title
            ));
        }
        if parsed.owner != fm.owner {
            failures.push(format!(
                "owner changed: {:?} -> {:?}\n{rendered}",
                fm.owner, parsed.owner
            ));
        }
        if parsed.people != fm.people {
            failures.push(format!(
                "people changed: {:?} -> {:?}\n{rendered}",
                fm.people, parsed.people
            ));
        }
        if parsed.tags != fm.tags {
            failures.push(format!(
                "tags changed: {:?} -> {:?}\n{rendered}",
                fm.tags, parsed.tags
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} values did not survive a round trip:\n\n{}",
        failures.len(),
        CORPUS.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn a_rendered_frontmatter_is_stable_when_rendered_again() {
    // render -> parse -> render must be identical, or every save rewrites the file.
    for value in CORPUS {
        let first = sample(value).render();
        let parsed = Frontmatter::parse(&first).expect("parse");
        let second = parsed.render();
        assert_eq!(first, second, "unstable for {value:?}");
    }
}
