//! Project auto-clustering, Tier 0.
//!
//! Five or more notes in seven days that share entities and a client domain are
//! probably one project. DayDoc proposes; the user accepts. Nothing is created
//! and no note is re-filed without a click.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::error::Result;
use crate::ids;
use crate::note::Note;
use crate::types::NoteType;
use crate::vault::{now_iso, today, Vault};

pub const MIN_NOTES: usize = 5;
pub const WINDOW_DAYS: i64 = 7;

/// Domains that say nothing about a client.
const NOISE_DOMAINS: [&str; 10] = [
    "gmail.com",
    "google.com",
    "googlemail.com",
    "github.com",
    "outlook.com",
    "hotmail.com",
    "yahoo.com",
    "example.com",
    "localhost",
    "notion.so",
];

#[derive(Debug, Clone, Serialize)]
pub struct ClusterProposal {
    pub name: String,
    pub notes: Vec<String>,
    pub people: Vec<String>,
    pub domain: Option<String>,
    pub window_days: i64,
}

impl ClusterProposal {
    pub fn render(&self) -> String {
        let mut out = format!(
            "{}  ({} notes in {} days)\n",
            self.name,
            self.notes.len(),
            self.window_days
        );
        if let Some(domain) = &self.domain {
            out.push_str(&format!("  domain: {domain}\n"));
        }
        if !self.people.is_empty() {
            out.push_str(&format!("  people: {}\n", self.people.join(", ")));
        }
        for note in &self.notes {
            out.push_str(&format!("  - {note}\n"));
        }
        out
    }
}

fn note_date(vault: &Vault, note: &Note) -> Option<chrono::NaiveDate> {
    for candidate in [
        note.frontmatter.created.as_deref(),
        note.frontmatter.updated.as_deref(),
        note.frontmatter.decided_at.as_deref(),
        note.frontmatter.last_verified.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let head = candidate.split('T').next().unwrap_or(candidate);
        if let Ok(date) = chrono::NaiveDate::parse_from_str(head, "%Y-%m-%d") {
            return Some(date);
        }
    }
    let path = vault.abs(&note.rel_path).ok()?;
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Local> = chrono::DateTime::from(modified);
    Some(datetime.date_naive())
}

/// The client domain in a note, from a URL or an email address.
pub fn domain_of(note: &Note) -> Option<String> {
    for token in note.search_text().split_whitespace() {
        let lowered = token.to_ascii_lowercase();
        let host = if let Some(rest) = lowered
            .strip_prefix("https://")
            .or_else(|| lowered.strip_prefix("http://"))
        {
            rest.split(['/', '?', '#', ')', '"', '>', ',', ';'])
                .next()
                .unwrap_or("")
                .to_string()
        } else if let Some((_, domain)) = lowered.split_once('@') {
            if domain.contains('.') && !domain.contains('/') {
                domain
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
                    .next()
                    .unwrap_or("")
                    .to_string()
            } else {
                continue;
            }
        } else {
            continue;
        };
        let host = host.trim_start_matches("www.").trim_end_matches('.');
        let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
        if labels.len() < 2 {
            continue;
        }
        let tld = labels[labels.len() - 1];
        if tld.len() < 2 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        // Keep a second-level suffix with the name: acme.co.in, not co.in.
        let mut take = 2usize;
        if labels.len() >= 3 {
            let sld = labels[labels.len() - 2];
            if matches!(sld, "co" | "com" | "org" | "net" | "ac" | "gov" | "edu" | "or" | "ne") {
                take = 3;
            }
        }
        let candidate = labels[labels.len() - take..].join(".");
        if NOISE_DOMAINS.contains(&candidate.as_str()) {
            continue;
        }
        return Some(candidate);
    }
    None
}

/// Propose project folders for notes that already look like one project.
pub fn propose(vault: &Vault) -> Result<Vec<ClusterProposal>> {
    let mut groups: BTreeMap<String, Vec<(String, chrono::NaiveDate, Vec<String>)>> =
        BTreeMap::new();

    for note in vault.scan()?.notes {
        if note.note_type() == NoteType::Decision {
            continue;
        }
        // Already filed: not a proposal, a fact.
        if !note.frontmatter.projects.is_empty() {
            continue;
        }
        let date = match note_date(vault, &note) {
            Some(date) => date,
            None => continue,
        };
        let domain = domain_of(&note);
        let key = match (&domain, note.frontmatter.people.first()) {
            (Some(domain), _) => format!("d:{domain}"),
            (None, Some(person)) => format!("p:{}", person.to_ascii_lowercase()),
            (None, None) => continue,
        };
        groups.entry(key).or_default().push((
            note.rel_path.clone(),
            date,
            note.frontmatter.people.clone(),
        ));
    }

    let mut proposals = Vec::new();
    for (key, mut members) in groups {
        if members.len() < MIN_NOTES {
            continue;
        }
        members.sort_by_key(|(_, date, _)| *date);

        // Largest window of WINDOW_DAYS containing the most notes.
        let mut best: Vec<usize> = Vec::new();
        let mut start = 0usize;
        for end in 0..members.len() {
            while (members[end].1 - members[start].1).num_days() > WINDOW_DAYS {
                start += 1;
            }
            if end - start + 1 > best.len() {
                best = (start..=end).collect();
            }
        }
        if best.len() < MIN_NOTES {
            continue;
        }

        let domain = key.strip_prefix("d:").map(|d| d.to_string());
        let mut people: Vec<String> = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        for index in &best {
            notes.push(members[*index].0.clone());
            for person in &members[*index].2 {
                if !people.contains(person) {
                    people.push(person.clone());
                }
            }
        }
        people.sort();
        let name = match (&domain, people.first()) {
            (Some(domain), _) => title_case(domain),
            (None, Some(person)) => format!("{} project", title_case(person)),
            (None, None) => continue,
        };
        proposals.push(ClusterProposal {
            name,
            notes,
            people,
            domain,
            window_days: WINDOW_DAYS,
        });
    }

    proposals.sort_by(|a, b| b.notes.len().cmp(&a.notes.len()));
    Ok(proposals)
}

fn title_case(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Accept a proposal: write the project note and file its members.
pub fn accept(vault: &Vault, proposal: &ClusterProposal) -> Result<Note> {
    let slug = ids::slug(&proposal.name, 40);
    let rel = format!("projects/{}-{}.md", today(), slug);

    let mut note = Note::parse(&rel, "---\n---\n")?;
    note.frontmatter.id = ids::new_ulid();
    note.frontmatter.note_type = NoteType::ProjectUpdate.as_str().to_string();
    note.frontmatter.title = format!("Project — {}", proposal.name);
    note.frontmatter.status = Some("active".to_string());
    note.frontmatter.projects = vec![proposal.name.clone()];
    note.frontmatter.people = proposal.people.clone();
    note.frontmatter.last_verified = Some(today());
    note.frontmatter.created = Some(now_iso());
    note.frontmatter.updated = Some(now_iso());

    let mut body = String::from("## Status\n\n_On track._\n\n## Source notes\n\n");
    for member in &proposal.notes {
        body.push_str(&format!("- {member}\n"));
    }
    if let Some(domain) = &proposal.domain {
        body.push_str(&format!("\n## Client\n\n{domain}\n"));
    }
    note.body = body;

    if !vault.exists(&rel) {
        vault.create(&rel, &note.render())?;
    }

    let mut filed = 0;
    for member in &proposal.notes {
        if let Ok(mut member_note) = vault.load(member) {
            if !member_note
                .frontmatter
                .projects
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&proposal.name))
            {
                member_note.frontmatter.projects.push(proposal.name.clone());
                member_note.frontmatter.updated = Some(now_iso());
                vault.save(&member_note)?;
                filed += 1;
            }
        }
    }
    vault.audit(
        "project-clustered",
        &rel,
        &format!("{} notes filed as {}", filed, proposal.name),
    )?;
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    fn seed_notes(vault: &Vault, count: usize, body: &str) {
        for i in 0..count {
            vault
                .write(
                    &format!("meetings/acme-{i}.md"),
                    &format!(
                        "---\ntype: meeting\ntitle: Acme sync {i}\ncreated: {}\n---\n{body}\n",
                        today()
                    ),
                )
                .expect("write");
        }
    }

    #[test]
    fn five_notes_sharing_a_client_domain_become_a_proposal() {
        let vault = temp_vault();
        seed_notes(&vault, 5, "Notes from https://acme.co.in/portal and the call.");
        let proposals = propose(&vault).expect("propose");
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].notes.len(), 5);
        assert_eq!(proposals[0].domain.as_deref(), Some("acme.co.in"));
        assert_eq!(proposals[0].name, "Acme.co.in");
    }

    #[test]
    fn four_notes_are_not_enough() {
        let vault = temp_vault();
        seed_notes(&vault, 4, "Notes from https://acme.co.in/portal");
        assert!(propose(&vault).expect("propose").is_empty());
    }

    #[test]
    fn personal_email_domains_are_ignored() {
        let vault = temp_vault();
        seed_notes(&vault, 6, "mail bob@gmail.com about it");
        assert!(propose(&vault).expect("propose").is_empty());
    }

    #[test]
    fn notes_spread_beyond_the_window_do_not_cluster() {
        let vault = temp_vault();
        for i in 0..6 {
            let date = (chrono::Local::now().date_naive() - chrono::Duration::days(i * 5))
                .format("%Y-%m-%d")
                .to_string();
            vault
                .write(
                    &format!("meetings/spread-{i}.md"),
                    &format!(
                        "---\ntype: meeting\ntitle: Spread {i}\ncreated: {date}\n---\nhttps://acme.co.in/\n"
                    ),
                )
                .expect("write");
        }
        assert!(propose(&vault).expect("propose").is_empty());
    }

    #[test]
    fn accepting_writes_a_project_and_files_the_members() {
        let vault = temp_vault();
        seed_notes(&vault, 5, "https://acme.co.in/portal");
        let proposal = propose(&vault).expect("propose").remove(0);
        let project = accept(&vault, &proposal).expect("accept");
        assert_eq!(project.note_type(), NoteType::ProjectUpdate);
        assert!(project.body.contains("## Source notes"));

        let member = vault.load("meetings/acme-0.md").expect("member");
        assert_eq!(member.frontmatter.projects, vec!["Acme.co.in".to_string()]);

        // Filed notes no longer propose.
        assert!(propose(&vault).expect("propose").is_empty());
    }
}
