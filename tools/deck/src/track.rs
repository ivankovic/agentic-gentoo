//! The upstream state of each local patch, kept in the host's `upstream.toml`.

use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Unsent,
    Reported,
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
    pub const ALL: [State; 11] = [
        State::Unsent,
        State::Reported,
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
            State::Reported => "reported",
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

    /// Whether there is anything left to send: not when it came from upstream, is upstream
    /// already, or is no longer going there.
    pub fn sendable(self) -> bool {
        matches!(
            self,
            State::Unsent | State::Reported | State::Sent | State::InReview | State::ChangesRequested
        )
    }

    pub fn help(self) -> &'static str {
        match self {
            State::Unsent => "ours, not offered upstream yet",
            State::Reported => "the bug is reported upstream, the patch not offered yet",
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
            State::Unsent
                | State::Reported
                | State::Sent
                | State::InReview
                | State::ChangesRequested
                | State::Accepted
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
    /// `category/package/file`, as `deck patches patches` prints it.
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
# one. Edit with `deck patches` (the TUI), `deck patches mark` or by hand: deck changes only the
# patch it is asked about, and keeps comments and keys of yours.

";

impl Tracking {
    pub fn load(path: &Path) -> Result<Tracking> {
        if !path.exists() {
            return Ok(Tracking::default());
        }
        let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Adds an event to the patch ID in the file at PATH, and returns the file's tracking as it
    /// then is. The file is read again for it, and changed in place: what another `deck patches`
    /// or a hand edit put there since it was loaded stays, comments and keys deck doesn't know
    /// included. A patch new to the file goes at its end.
    pub fn add_event(path: &Path, id: &str, e: &Event) -> Result<Tracking> {
        edit(path, id, |patch| {
            let mut t = Table::new();
            t["at"] = value(&e.at);
            t["state"] = value(e.state.label());
            if !e.url.is_empty() {
                t["url"] = value(&e.url);
            }
            if !e.note.is_empty() {
                t["note"] = value(&e.note);
            }
            let events = patch.entry("event").or_insert(Item::ArrayOfTables(ArrayOfTables::new()));
            events.as_array_of_tables_mut().context("its `event` is not a list of [[patch.event]] tables")?.push(t);
            Ok(())
        })
    }

    /// Sets the upstream project of the patch ID in the file at PATH; as `add_event` otherwise.
    pub fn set_upstream(path: &Path, id: &str, url: &str) -> Result<Tracking> {
        edit(path, id, |patch| {
            if url.is_empty() {
                patch.remove("upstream");
            } else {
                patch["upstream"] = value(url);
            }
            Ok(())
        })
    }
}

/// Reads the file at PATH as it is now, changes the `[[patch]]` with this id (added if it has
/// none) and writes it back, the rest of the text untouched.
fn edit(path: &Path, id: &str, change: impl FnOnce(&mut Table) -> Result<()>) -> Result<Tracking> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HEADER.to_string(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let mut doc: DocumentMut = text.parse().with_context(|| format!("parsing {}", path.display()))?;
    // A file with nothing but comments, or none yet: they stay above the first patch.
    let mut above = String::new();
    if doc.as_table().is_empty() {
        doc = DocumentMut::new();
        above = text;
        if !above.is_empty() && !above.ends_with('\n') {
            above.push('\n');
        }
    }
    let patches = doc
        .entry("patch")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .with_context(|| format!("{}: `patch` is not a list of [[patch]] tables", path.display()))?;
    let found = patches.iter().position(|p| p.get("id").and_then(Item::as_str) == Some(id));
    let at = match found {
        Some(i) => i,
        None => {
            let mut t = Table::new();
            t["id"] = value(id);
            patches.push(t);
            patches.len() - 1
        }
    };
    change(patches.get_mut(at).unwrap()).with_context(|| format!("{}: {id}", path.display()))?;
    let text = format!("{above}{doc}");
    // What is written is what deck reads: checked before the file is replaced.
    let tracking: Tracking = toml::from_str(&text).with_context(|| format!("{}: not written, it would not read back", path.display()))?;
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(tracking)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(at: &str, state: State, note: &str) -> Event {
        Event { at: at.into(), state, url: String::new(), note: note.into() }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("deck-track-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("upstream.toml")
    }

    #[test]
    fn round_trips() {
        let path = scratch("new");
        Tracking::set_upstream(&path, "a/b/0001.patch", "https://example.org/b").unwrap();
        let t = Tracking::add_event(&path, "a/b/0001.patch", &event("2026-10-16T10:00:00+02:00", State::InReview, "x")).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(HEADER), "{text}");
        assert!(text.contains("[[patch.event]]"), "{text}");
        assert!(text.contains("state = \"in-review\""), "{text}");
        assert_eq!(t.patches[0].upstream, "https://example.org/b");
        assert_eq!(t.patches[0].last().unwrap().state, State::InReview);
        let back = Tracking::load(&path).unwrap();
        assert_eq!(back.patches[0].last().unwrap().note, "x");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    const BY_HAND: &str = "\
# Local patches, with a note of my own.

[[patch]]
id = \"a/b/0001.patch\"
upstream = \"https://example.org/b\"  # the mirror, not the forge
reviewer = \"someone\"

[[patch.event]]
at = \"2026-10-16T10:00:00+02:00\"
state = \"sent\"

# Waiting for their release.
[[patch]]
id = \"c/d/0002.patch\"

[[patch.event]]
at = \"2026-10-17T10:00:00+02:00\"
state = \"unsent\"
";

    #[test]
    fn a_save_keeps_what_was_written_meanwhile() {
        let path = scratch("meanwhile");
        fs::write(&path, BY_HAND).unwrap();
        // Loaded, as the TUI does at its start.
        let stale = Tracking::load(&path).unwrap();
        assert_eq!(stale.patches[0].events.len(), 1);
        // Meanwhile another `deck patches` adds an event, and a line is added by hand.
        Tracking::add_event(&path, "a/b/0001.patch", &event("2026-10-18T10:00:00+02:00", State::InReview, "")).unwrap();
        let edited = fs::read_to_string(&path).unwrap().replace("# Waiting", "# Still waiting");
        fs::write(&path, edited).unwrap();
        // The first one's change, from what it loaded then.
        let t = Tracking::add_event(&path, "c/d/0002.patch", &event("2026-10-19T10:00:00+02:00", State::Sent, "a \"note\"")).unwrap();
        let states = |i: usize| t.patches[i].events.iter().map(|e| e.state).collect::<Vec<_>>();
        assert_eq!(states(0), [State::Sent, State::InReview]);
        assert_eq!(states(1), [State::Unsent, State::Sent]);
        assert_eq!(t.patches[1].last().unwrap().note, "a \"note\"");
        let text = fs::read_to_string(&path).unwrap();
        for kept in ["# Local patches, with a note of my own.", "# the mirror, not the forge", "reviewer = \"someone\"", "# Still waiting for their release."] {
            assert!(text.contains(kept), "{kept} is gone:\n{text}");
        }
        // Each event under its own patch, in the order they happened.
        let order: Vec<usize> = ["0001.patch", "\"sent\"", "\"in-review\"", "0002.patch", "\"unsent\"", "2026-10-19"]
            .iter()
            .map(|x| text.find(x).unwrap())
            .collect();
        assert!(order.is_sorted(), "{text}");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_new_patch_and_its_upstream_leave_the_rest_alone() {
        let path = scratch("new-patch");
        fs::write(&path, BY_HAND).unwrap();
        Tracking::set_upstream(&path, "e/f/0003.patch", "https://example.org/f").unwrap();
        let t = Tracking::add_event(&path, "e/f/0003.patch", &event("2026-10-20T10:00:00+02:00", State::Unsent, "")).unwrap();
        assert_eq!(t.patches.len(), 3);
        assert_eq!(t.patches[2].upstream, "https://example.org/f");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(BY_HAND), "{text}");
        // An upstream taken back is a key removed, not an empty one.
        Tracking::set_upstream(&path, "a/b/0001.patch", "").unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("the mirror"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_file_that_does_not_parse_is_left_as_it_is() {
        let path = scratch("broken");
        fs::write(&path, "[[patch]]\nid = \n").unwrap();
        assert!(Tracking::add_event(&path, "a/b/0001.patch", &event("2026-10-20T10:00:00+02:00", State::Unsent, "")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[[patch]]\nid = \n");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
