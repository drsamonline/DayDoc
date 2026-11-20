//! Vault statistics: the numbers a user checks before trusting a vault.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::error::Result;
use crate::ledger;
use crate::note::Note;
use crate::stale;
use crate::vault::Vault;

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub vault: String,
    pub notes: usize,
    pub by_type: BTreeMap<String, usize>,
    pub decisions: usize,
    pub decisions_by_status: BTreeMap<String, usize>,
    pub stale: usize,
    pub open_tasks: usize,
    pub words: usize,
    pub attachments: usize,
    pub people: Vec<(String, usize)>,
    pub projects: Vec<(String, usize)>,
    pub oldest: Option<String>,
    pub newest: Option<String>,
}

fn note_date(note: &Note) -> Option<String> {
    for candidate in [
        note.frontmatter.created.as_deref(),
        note.frontmatter.updated.as_deref(),
        note.frontmatter.decided_at.as_deref(),
        note.frontmatter.last_verified.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if candidate.len() >= 10 {
            return Some(candidate[..10].to_string());
        }
    }
    None
}

fn open_tasks(note: &Note) -> usize {
    note.body
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("- [ ]") || trimmed.starts_with("* [ ]")
        })
        .count()
}

fn attachment_count(vault: &Vault) -> usize {
    let root = vault.root().join("assets");
    if !root.is_dir() {
        return 0;
    }
    let mut count = 0usize;
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                match entry.metadata() {
                    Ok(meta) if meta.is_dir() => stack.push(entry.path()),
                    Ok(meta) if meta.is_file() => count += 1,
                    _ => {}
                }
            }
        }
    }
    count
}

fn top(counts: BTreeMap<String, usize>) -> Vec<(String, usize)> {
    let mut pairs: Vec<(String, usize)> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    pairs.truncate(10);
    pairs
}

pub fn compute(vault: &Vault) -> Result<Stats> {
    let notes = vault.scan()?.notes;
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut people: BTreeMap<String, usize> = BTreeMap::new();
    let mut projects: BTreeMap<String, usize> = BTreeMap::new();
    let mut words = 0usize;
    let mut tasks = 0usize;
    let mut dates: Vec<String> = Vec::new();

    for note in &notes {
        *by_type
            .entry(note.note_type().as_str().to_string())
            .or_insert(0) += 1;
        for person in &note.frontmatter.people {
            *people.entry(person.clone()).or_insert(0) += 1;
        }
        if let Some(owner) = &note.frontmatter.owner {
            *people.entry(owner.clone()).or_insert(0) += 1;
        }
        for project in &note.frontmatter.projects {
            *projects.entry(project.clone()).or_insert(0) += 1;
        }
        words += note.body.split_whitespace().count();
        tasks += open_tasks(note);
        if let Some(date) = note_date(note) {
            dates.push(date);
        }
    }

    let mut decisions_by_status: BTreeMap<String, usize> = BTreeMap::new();
    let cards = ledger::cards(vault)?;
    for card in &cards {
        *decisions_by_status
            .entry(card.status.as_str().to_string())
            .or_insert(0) += 1;
    }

    dates.sort();
    let stale_count = stale::scan(vault)?.len();

    Ok(Stats {
        vault: vault.root().display().to_string(),
        notes: notes.len(),
        by_type,
        decisions: cards.len(),
        decisions_by_status,
        stale: stale_count,
        open_tasks: tasks,
        words,
        attachments: attachment_count(vault),
        people: top(people),
        projects: top(projects),
        oldest: dates.first().cloned(),
        newest: dates.last().cloned(),
    })
}

impl Stats {
    pub fn render(&self) -> String {
        let mut out = format!("vault       {}\n", self.vault);
        out.push_str(&format!(
            "notes       {}  ({} words, {} attachments)\n",
            self.notes, self.words, self.attachments
        ));
        for (kind, count) in &self.by_type {
            out.push_str(&format!("  {kind:<14}{count}\n"));
        }
        out.push_str(&format!(
            "decisions   {}  {}\n",
            self.decisions,
            self.decisions_by_status
                .iter()
                .map(|(status, count)| format!("{status} {count}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        out.push_str(&format!("stale       {}\n", self.stale));
        out.push_str(&format!("open tasks  {}\n", self.open_tasks));
        if let (Some(oldest), Some(newest)) = (&self.oldest, &self.newest) {
            out.push_str(&format!("dates       {oldest} to {newest}\n"));
        }
        if !self.people.is_empty() {
            out.push_str(&format!(
                "people      {}\n",
                self.people
                    .iter()
                    .map(|(name, count)| format!("{name} ({count})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !self.projects.is_empty() {
            out.push_str(&format!(
                "projects    {}\n",
                self.projects
                    .iter()
                    .map(|(name, count)| format!("{name} ({count})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::NewDecision;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn stats_count_notes_decisions_and_tasks() {
        let vault = temp_vault();
        ledger::create(
            &vault,
            &NewDecision::new("Use Okta for SSO")
                .accepted()
                .with_owner("alice"),
        )
        .expect("decision");
        vault
            .write(
                "projects/atlas.md",
                "---\nid: 01A\ntype: project-update\ntitle: Atlas\nprojects: [Atlas]\n---\n- [ ] ship it\n- [ ] test it\n",
            )
            .expect("note");

        let stats = compute(&vault).expect("stats");
        assert_eq!(stats.notes, 2);
        assert_eq!(stats.decisions, 1);
        assert_eq!(stats.decisions_by_status.get("accepted"), Some(&1));
        assert_eq!(stats.open_tasks, 2);
        assert_eq!(stats.by_type.get("decision"), Some(&1));
        assert!(stats.people.iter().any(|(name, _)| name == "alice"));
        assert!(stats.projects.iter().any(|(name, _)| name == "Atlas"));
        assert!(stats.render().contains("decisions   1"));
    }

    #[test]
    fn an_empty_vault_is_zeroes_not_an_error() {
        let vault = temp_vault();
        let stats = compute(&vault).expect("stats");
        assert_eq!(stats.notes, 0);
        assert_eq!(stats.decisions, 0);
        assert_eq!(stats.open_tasks, 0);
    }
}
