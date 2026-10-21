//! The screener, spam and unsubscribing, through deck-mail: who a sender is to you, why a mail
//! was screened or looks like spam, and how to leave a mailing list. deck-mail keeps the rules
//! (~/.config/deck-mail/senders) and does the talking to the outside.

use crate::archive::deck_mail;
use anyhow::{Context, Result};
use serde::Deserialize;

/// The address in a From header: `Ana <ana@example.org>` or `ana@example.org`.
pub fn address(from: &str) -> String {
    match (from.rfind('<'), from.rfind('>')) {
        (Some(a), Some(b)) if a < b => from[a + 1..b].trim().to_lowercase(),
        _ => from.trim().to_lowercase(),
    }
}

/// `allow` or `block` a sender, or `forget` them; returns what deck-mail tagged, for undo: each
/// change (`-screener`) with the messages it changed.
pub fn sender(kind: &str, address: &str) -> Result<Vec<(String, Vec<String>)>> {
    let out = deck_mail(&["screener", kind, address])?;
    if out.iter().all(u8::is_ascii_whitespace) {
        return Ok(vec![]);
    }
    serde_json::from_slice(&out).context("reading deck-mail screener")
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct Why {
    pub sender: String,
    #[serde(default)]
    pub new_sender: bool,
    #[serde(default)]
    pub reasons: Vec<String>,
}

/// Why these messages were screened or are suspect, the ones deck-mail's filter remembers.
pub fn why(ids: &[String]) -> Result<Vec<Why>> {
    let mut args = vec!["why"];
    args.extend(ids.iter().map(String::as_str));
    serde_json::from_slice(&deck_mail(&args)?).context("reading deck-mail why")
}

#[derive(Deserialize, Debug, Clone)]
pub struct Unsubscribe {
    /// one-click, mailto, link or none.
    pub method: String,
    pub target: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
}

/// How the mail lets you leave its list; nothing is sent.
pub fn unsubscribe_check(id: &str) -> Result<Unsubscribe> {
    serde_json::from_slice(&deck_mail(&["unsubscribe", "--check", id])?)
        .context("reading deck-mail unsubscribe")
}

/// The one-click unsubscribe (RFC 8058): deck-mail posts it.
pub fn unsubscribe(id: &str) -> Result<()> {
    deck_mail(&["unsubscribe", id]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(address("Ana Anić <Ana@Example.org>"), "ana@example.org");
        assert_eq!(address("ana@example.org"), "ana@example.org");
        assert_eq!(address("\"a <b>\" <c@d.e>"), "c@d.e");
    }
}
