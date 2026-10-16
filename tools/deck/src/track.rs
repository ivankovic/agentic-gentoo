//! The upstream state of each local patch, kept in the host's `upstream.toml`.

use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Unsent,
    Sent,
    InReview,
    ChangesRequested,
    Accepted,
    Released,
    Backport,
    Rejected,
    Withdrawn,
    Dropped,
}

impl State {
    pub const ALL: [State; 10] = [
        State::Unsent,
        State::Sent,
        State::InReview,
        State::ChangesRequested,
        State::Accepted,
        State::Released,
        State::Backport,
        State::Rejected,
        State::Withdrawn,
        State::Dropped,
    ];

    pub fn label(self) -> &'static str {
        match self {
            State::Unsent => "unsent",
            State::Sent => "sent",
            State::InReview => "in-review",
            State::ChangesRequested => "changes-requested",
            State::Accepted => "accepted",
            State::Released => "released",
            State::Backport => "backport",
            State::Rejected => "rejected",
            State::Withdrawn => "withdrawn",
            State::Dropped => "dropped",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            State::Unsent => "ours, not offered upstream yet",
            State::Sent => "offered upstream, no response yet",
            State::InReview => "upstream is reviewing it",
            State::ChangesRequested => "upstream asked for changes",
            State::Accepted => "merged upstream, not in a release yet",
            State::Released => "in an upstream release; drop it on the next bump",
            State::Backport => "taken from upstream; drop it on the next bump",
            State::Rejected => "upstream said no for good; ours to carry",
            State::Withdrawn => "we took it back",
            State::Dropped => "no longer carried here",
        }
    }

    /// Whether the patch still waits on someone, us or upstream.
    pub fn open(self) -> bool {
        matches!(
            self,
            State::Unsent | State::Sent | State::InReview | State::ChangesRequested | State::Accepted
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Event {
    /// RFC 3339, local time.
    pub at: String,
    pub state: State,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tracked {
    /// `category/package/file`, as `deck patches` prints it.
    pub id: String,
    /// The upstream project the patch is meant for.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub upstream: String,
    #[serde(default, rename = "event")]
    pub events: Vec<Event>,
}

impl Tracked {
    pub fn last(&self) -> Option<&Event> {
        self.events.last()
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Tracking {
    #[serde(default, rename = "patch")]
    pub patches: Vec<Tracked>,
}

const HEADER: &str = "\
# Local patches on their way upstream, kept by deck (tools/deck).
# One [[patch]] per patch; each [[patch.event]] is a change of state, and the last is the current
# one. Edit with `deck`, `deck mark` or by hand.

";

impl Tracking {
    pub fn load(path: &Path) -> Result<Tracking> {
        if !path.exists() {
            return Ok(Tracking::default());
        }
        let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&mut self, path: &Path) -> Result<()> {
        self.patches.sort_by(|a, b| a.id.cmp(&b.id));
        let text = format!("{HEADER}{}", toml::to_string_pretty(self)?);
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
    }

    pub fn entry(&mut self, id: &str) -> &mut Tracked {
        let i = match self.patches.iter().position(|t| t.id == id) {
            Some(i) => i,
            None => {
                self.patches.push(Tracked {
                    id: id.to_string(),
                    upstream: String::new(),
                    events: vec![],
                });
                self.patches.len() - 1
            }
        };
        &mut self.patches[i]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let mut t = Tracking::default();
        let e = t.entry("a/b/0001.patch");
        e.upstream = "https://example.org/b".into();
        e.events.push(Event {
            at: "2026-10-16T10:00:00+02:00".into(),
            state: State::InReview,
            url: String::new(),
            note: "x".into(),
        });
        let text = toml::to_string_pretty(&t).unwrap();
        assert!(text.contains("[[patch.event]]"), "{text}");
        assert!(text.contains("state = \"in-review\""), "{text}");
        let back: Tracking = toml::from_str(&text).unwrap();
        assert_eq!(back.patches[0].last().unwrap().state, State::InReview);
    }
}
