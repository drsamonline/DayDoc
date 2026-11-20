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

fn corpus() -> Vec<String> {
    let mut values: Vec<String> = [
        "plain".to_string(),
        "true".to_string(),
        "false".to_string(),
        "yes".to_string(),
        "no".to_string(),
        "on".to_string(),
        "off".to_string(),
        "null".to_string(),
        "Null".to_string(),
        "~".to_string(),
        "123".to_string(),
        "1.5".to_string(),
        "-1".to_string(),
        "0x1f".to_string(),
        "1e9".to_string(),
        "2026-11-20".to_string(),
        "12:30".to_string(),
        "- leading dash".to_string(),
        "-".to_string(),
        "--".to_string(),
        "?".to_string(),
        "?question".to_string(),
        "!".to_string(),
        "!bang".to_string(),
        "*star".to_string(),
        "&amp".to_string(),
        "%pct".to_string(),
        "|pipe".to_string(),
        ">gt".to_string(),
        "'single'".to_string(),
        "\"double\"".to_string(),
        "has: colon".to_string(),
        "has # hash".to_string(),
        "#starts-with-hash".to_string(),
        "@at".to_string(),
        "`backtick`".to_string(),
        "{brace}".to_string(),
        "[bracket]".to_string(),
        "a,b".to_string(),
        "trailing space ".to_string(),
        " leading space".to_string(),
        "two  spaces".to_string(),
        "tab\there".to_string(),
        "newline\nbreak".to_string(),
        "carriage\rreturn".to_string(),
        "ünïcode".to_string(),
        "मराठी निर्णय".to_string(),
        "हिन्दी".to_string(),
        "ગુજરાતી".to_string(),
        "தமிழ்".to_string(),
        "🎯 emoji".to_string(),
        "back\\slash".to_string(),
        "".to_string(),
        " ".to_string(),
    ]
    .to_vec();
    // A value long enough to cross any line-length assumption.
    values.push("a".repeat(300));
    values
}

#[test]
fn every_awkward_value_survives_render_and_parse() {
    let mut failures: Vec<String> = Vec::new();

    let corpus = corpus();
    for value in &corpus {
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
        // An empty optional field is written as absent, which reads back as None.
        if !value.is_empty() && parsed.owner != fm.owner {
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
        corpus.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn a_rendered_frontmatter_is_stable_when_rendered_again() {
    // render -> parse -> render must be identical, or every save rewrites the file.
    for value in &corpus() {
        let first = sample(value).render();
        let parsed = Frontmatter::parse(&first).expect("parse");
        let second = parsed.render();
        assert_eq!(first, second, "unstable for {value:?}");
    }
}
