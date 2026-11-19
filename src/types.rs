//! Shared vocabulary: what a note is, and what a decision's state is.

use std::fmt;

/// The note types the intent classifier can assign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoteType {
    Daily,
    Meeting,
    Decision,
    Task,
    Idea,
    Sop,
    ProjectUpdate,
    Reference,
    Inbox,
}

impl NoteType {
    pub const ALL: [NoteType; 9] = [
        NoteType::Daily,
        NoteType::Meeting,
        NoteType::Decision,
        NoteType::Task,
        NoteType::Idea,
        NoteType::Sop,
        NoteType::ProjectUpdate,
        NoteType::Reference,
        NoteType::Inbox,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            NoteType::Daily => "daily",
            NoteType::Meeting => "meeting",
            NoteType::Decision => "decision",
            NoteType::Task => "task",
            NoteType::Idea => "idea",
            NoteType::Sop => "sop",
            NoteType::ProjectUpdate => "project-update",
            NoteType::Reference => "reference",
            NoteType::Inbox => "inbox",
        }
    }

    pub fn parse(raw: &str) -> Option<NoteType> {
        let s = raw.trim().to_ascii_lowercase();
        match s.as_str() {
            "daily" => Some(NoteType::Daily),
            "meeting" => Some(NoteType::Meeting),
            "decision" => Some(NoteType::Decision),
            "task" => Some(NoteType::Task),
            "idea" => Some(NoteType::Idea),
            "sop" => Some(NoteType::Sop),
            "project-update" | "project_update" | "projectupdate" | "project" => {
                Some(NoteType::ProjectUpdate)
            }
            "reference" => Some(NoteType::Reference),
            "inbox" => Some(NoteType::Inbox),
            _ => None,
        }
    }

    /// Folder a freshly captured note lands in.
    pub fn folder(self) -> &'static str {
        match self {
            NoteType::Daily => "daily",
            NoteType::Meeting => "meetings",
            NoteType::Decision => "decisions",
            NoteType::Task => "tasks",
            NoteType::Idea => "ideas",
            NoteType::Sop => "sops",
            NoteType::ProjectUpdate => "projects",
            NoteType::Reference => "reference",
            NoteType::Inbox => "inbox",
        }
    }

    /// Staleness threshold key in `daydoc.toml`.
    pub fn stale_key(self) -> Option<&'static str> {
        match self {
            NoteType::Sop => Some("sop"),
            NoteType::Reference => Some("reference"),
            NoteType::ProjectUpdate => Some("project-update"),
            NoteType::Meeting => Some("meeting"),
            _ => None,
        }
    }
}

impl fmt::Display for NoteType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The decision lifecycle. `accepted` never goes stale — it is superseded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecisionStatus {
    Proposed,
    Accepted,
    Superseded,
    Rejected,
}

impl DecisionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionStatus::Proposed => "proposed",
            DecisionStatus::Accepted => "accepted",
            DecisionStatus::Superseded => "superseded",
            DecisionStatus::Rejected => "rejected",
        }
    }

    pub fn parse(raw: &str) -> Option<DecisionStatus> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "proposed" => Some(DecisionStatus::Proposed),
            "accepted" => Some(DecisionStatus::Accepted),
            "superseded" => Some(DecisionStatus::Superseded),
            "rejected" => Some(DecisionStatus::Rejected),
            _ => None,
        }
    }

    /// May `self` become `to`? History is appended, never rewritten.
    pub fn can_transition_to(self, to: DecisionStatus) -> bool {
        use DecisionStatus::*;
        matches!(
            (self, to),
            (Proposed, Accepted)
                | (Proposed, Rejected)
                | (Accepted, Superseded)
                | (Superseded, Accepted)
        )
    }
}

impl fmt::Display for DecisionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_types_round_trip() {
        for t in NoteType::ALL {
            assert_eq!(NoteType::parse(t.as_str()), Some(t));
        }
    }

    #[test]
    fn lifecycle_rules() {
        use DecisionStatus::*;
        assert!(Proposed.can_transition_to(Accepted));
        assert!(Proposed.can_transition_to(Rejected));
        assert!(Accepted.can_transition_to(Superseded));
        assert!(Superseded.can_transition_to(Accepted));
        assert!(!Rejected.can_transition_to(Accepted));
        assert!(!Accepted.can_transition_to(Rejected));
    }
}
