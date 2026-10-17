//! The searches along the top, from `~/.config/mmm/searches`: one `Name = notmuch query` a line.

use std::path::PathBuf;

pub struct Search {
    pub name: String,
    pub query: String,
}

const DEFAULT: &str = "\
Inbox = tag:inbox
Unread = tag:unread
Starred = tag:flagged
Waiting = tag:pdf-waiting
Sent = tag:sent
Drafts = tag:draft
Everything = *
Bin = tag:deleted
";

pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("mmm/searches")
}

pub fn parse(text: &str) -> Vec<Search> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(n, q)| Search {
            name: n.trim().to_string(),
            query: q.trim().to_string(),
        })
        .filter(|s| !s.name.is_empty() && !s.query.is_empty())
        .collect()
}

pub fn searches() -> Vec<Search> {
    let found = std::fs::read_to_string(path())
        .map(|t| parse(&t))
        .unwrap_or_default();
    if found.is_empty() { parse(DEFAULT) } else { found }
}
