//! SOP extraction, Tier 0.
//!
//! Three or more notes describing the same ordered sequence of steps become a
//! draft SOP. Ordered n-grams, not bag-of-words: "rotate the key" then "restart
//! the worker" is a different procedure from the reverse.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::error::Result;
use crate::ids;
use crate::note::Note;
use crate::types::NoteType;
use crate::vault::{now_iso, today, Vault};

/// Minimum notes that must share a sequence before we propose anything.
pub const MIN_SUPPORT: usize = 3;
/// Minimum steps a note needs to count as a procedure.
pub const MIN_STEPS: usize = 3;

#[derive(Debug, Clone, Serialize)]
pub struct SopProposal {
    pub title: String,
    pub notes: Vec<String>,
    pub steps: Vec<String>,
    pub support: usize,
}

impl SopProposal {
    pub fn render(&self) -> String {
        let mut out = format!(
            "{}  ({} notes agree, {} steps)\n",
            self.title,
            self.support,
            self.steps.len()
        );
        for (i, step) in self.steps.iter().enumerate() {
            out.push_str(&format!("  {}. {}\n", i + 1, step));
        }
        out.push_str("  sources:\n");
        for note in &self.notes {
            out.push_str(&format!("    - {note}\n"));
        }
        out
    }
}

/// Ordered steps from a note: numbered items and bullets, in document order.
pub fn extract_steps(note: &Note) -> Vec<String> {
    let mut steps = Vec::new();
    for line in note.body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let content = if let Some(rest) = trimmed.strip_prefix("- [ ]") {
            rest.trim()
        } else if let Some(rest) = trimmed.strip_prefix("- [x]") {
            rest.trim()
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            rest.trim()
        } else if let Some(rest) = trimmed.strip_prefix("* ") {
            rest.trim()
        } else {
            let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
            let rest = &trimmed[digits.len()..];
            if !digits.is_empty() && (rest.starts_with(". ") || rest.starts_with(") ")) {
                rest[2..].trim()
            } else {
                continue;
            }
        };
        if !content.is_empty() {
            steps.push(content.to_string());
        }
    }
    steps
}

fn normalize(step: &str) -> String {
    step.to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Ordered trigrams of normalised steps.
fn signatures(steps: &[String]) -> BTreeSet<String> {
    let normalized: Vec<String> = steps.iter().map(|s| normalize(s)).collect();
    let mut out = BTreeSet::new();
    if normalized.len() < 3 {
        return out;
    }
    for window in normalized.windows(3) {
        if window.iter().all(|w| !w.is_empty()) {
            out.insert(window.join(" > "));
        }
    }
    out
}

struct DisjointSet {
    parent: Vec<usize>,
}

impl DisjointSet {
    fn new(n: usize) -> Self {
        DisjointSet {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

/// Propose SOP drafts from the vault. Never creates anything: the caller shows
/// the proposals and the user accepts one.
pub fn propose(vault: &Vault) -> Result<Vec<SopProposal>> {
    let mut candidates: Vec<(String, Vec<String>)> = Vec::new();
    for note in vault.scan()?.notes {
        if note.note_type() == NoteType::Decision {
            continue;
        }
        let steps = extract_steps(&note);
        if steps.len() >= MIN_STEPS {
            candidates.push((note.rel_path, steps));
        }
    }
    if candidates.len() < MIN_SUPPORT {
        return Ok(Vec::new());
    }

    let sig_sets: Vec<BTreeSet<String>> = candidates
        .iter()
        .map(|(_, steps)| signatures(steps))
        .collect();

    let mut by_signature: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, set) in sig_sets.iter().enumerate() {
        for signature in set {
            by_signature.entry(signature.as_str()).or_default().push(index);
        }
    }

    let mut groups = DisjointSet::new(candidates.len());
    for members in by_signature.values() {
        if members.len() >= MIN_SUPPORT {
            for pair in members.windows(2) {
                groups.union(pair[0], pair[1]);
            }
        }
    }

    let mut clusters: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..candidates.len() {
        let root = groups.find(index);
        clusters.entry(root).or_default().push(index);
    }

    let mut proposals = Vec::new();
    for members in clusters.into_values() {
        if members.len() < MIN_SUPPORT {
            continue;
        }
        // The medoid: the note sharing the most signatures with the rest.
        let mut best = members[0];
        let mut best_overlap = 0usize;
        for &index in &members {
            let overlap = members
                .iter()
                .filter(|&&other| other != index)
                .map(|&other| sig_sets[index].intersection(&sig_sets[other]).count())
                .sum::<usize>();
            if overlap > best_overlap {
                best_overlap = overlap;
                best = index;
            }
        }
        let steps = candidates[best].1.clone();
        let notes: Vec<String> = members
            .iter()
            .map(|&i| candidates[i].0.clone())
            .collect();
        let first = steps.first().cloned().unwrap_or_else(|| "procedure".to_string());
        proposals.push(SopProposal {
            title: format!("SOP — {}", title_case(&first)),
            notes,
            steps,
            support: members.len(),
        });
    }
    proposals.sort_by_key(|proposal| std::cmp::Reverse(proposal.support));
    Ok(proposals)
}

fn title_case(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Accept a proposal: write it as an SOP note with provenance and a
/// verification date, so staleness has something to measure.
pub fn accept(vault: &Vault, proposal: &SopProposal) -> Result<Note> {
    let slug = ids::slug(&proposal.title, 50);
    let rel = format!("sops/{}-{}.md", today(), slug);
    let mut note = Note::parse(&rel, "---\ntype: sop\n---\n")?;
    note.frontmatter.id = ids::new_ulid();
    note.frontmatter.note_type = NoteType::Sop.as_str().to_string();
    note.frontmatter.title = proposal.title.clone();
    note.frontmatter.status = Some("active".to_string());
    note.frontmatter.last_verified = Some(today());
    note.frontmatter.created = Some(now_iso());
    note.frontmatter.updated = Some(now_iso());
    note.frontmatter.links = proposal.notes.clone();

    let mut body = String::from("## Steps\n\n");
    for (i, step) in proposal.steps.iter().enumerate() {
        body.push_str(&format!("{}. {}\n", i + 1, step));
    }
    body.push_str("\n## Provenance\n\n");
    body.push_str(&format!(
        "Proposed from {} notes that describe the same ordered sequence:\n\n",
        proposal.support
    ));
    for source in &proposal.notes {
        body.push_str(&format!("- {source}\n"));
    }
    note.body = body;

    vault.create(&note.rel_path, &note.render())?;
    vault.audit("sop-accepted", &note.rel_path, &format!("support={}", proposal.support))?;
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    fn procedure(day: &str) -> String {
        format!(
            "---\ntype: reference\ntitle: Key rotation {day}\n---\n## Steps\n\n1. open the console\n2. rotate the signing key\n3. restart the worker\n4. post in the channel\n"
        )
    }

    #[test]
    fn steps_are_extracted_in_order() {
        let note = Note::parse("x.md", &procedure("a")).expect("parse");
        let steps = extract_steps(&note);
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0], "open the console");
        assert_eq!(steps[3], "post in the channel");
    }

    #[test]
    fn three_notes_with_the_same_sequence_propose_an_sop() {
        let vault = temp_vault();
        for (i, day) in ["mon", "tue", "wed"].iter().enumerate() {
            vault
                .write(&format!("reference/rot-{i}.md"), &procedure(day))
                .expect("write");
        }
        let proposals = propose(&vault).expect("propose");
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].support, 3);
        assert_eq!(proposals[0].steps.len(), 4);
    }

    #[test]
    fn reordered_steps_do_not_match() {
        let vault = temp_vault();
        vault.write("reference/a.md", &procedure("a")).expect("write");
        vault.write("reference/b.md", &procedure("b")).expect("write");
        let reversed = "---\ntype: reference\n---\n## Steps\n\n1. post in the channel\n2. restart the worker\n3. rotate the signing key\n4. open the console\n";
        vault.write("reference/c.md", reversed).expect("write");
        let proposals = propose(&vault).expect("propose");
        assert!(proposals.is_empty(), "reversed sequence should not cluster");
    }

    #[test]
    fn accepting_writes_an_sop_note_with_provenance() {
        let vault = temp_vault();
        for (i, day) in ["mon", "tue", "wed"].iter().enumerate() {
            vault
                .write(&format!("reference/rot-{i}.md"), &procedure(day))
                .expect("write");
        }
        let proposal = propose(&vault).expect("propose").remove(0);
        let note = accept(&vault, &proposal).expect("accept");
        assert_eq!(note.note_type(), NoteType::Sop);
        assert!(note.body.contains("## Provenance"));
        assert!(note.frontmatter.last_verified.is_some());
        assert_eq!(note.frontmatter.links.len(), 3);
    }
}
