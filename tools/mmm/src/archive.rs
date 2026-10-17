//! The family archive in Nextcloud, through deck-mail: what became of each PDF in mail, and the
//! choice of where one goes. mmm only chooses; deck-mail's sync does the sending.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::process::{Command, Stdio};

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// In Nextcloud already.
    Sent,
    /// Chosen; the next sync sends it.
    Chosen,
    #[serde(rename = "left out")]
    LeftOut,
    /// No one has decided on it yet.
    Waiting,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Pdf {
    pub message: String,
    pub sha256: String,
    pub state: State,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Where it is in Nextcloud, once sent.
    #[serde(default)]
    pub r#where: Option<String>,
    /// What the sender and file name rules make of it: a guess to confirm. ("-", "-") is "leave
    /// it out".
    pub suggested: Option<(String, String)>,
}

impl Pdf {
    /// The choice it has now, to put back on undo.
    pub fn choice(&self) -> Choice {
        match (&self.state, &self.category, &self.name) {
            (State::Chosen, Some(c), Some(n)) => Choice::To(c.clone(), n.clone()),
            (State::LeftOut, ..) => Choice::Out,
            _ => Choice::Forget,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    To(String, String),
    Out,
    /// Undecided again: it waits.
    Forget,
}

#[derive(Deserialize)]
struct Pdfs {
    categories: Vec<String>,
    pdfs: Vec<Pdf>,
}

fn deck_mail(args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("deck-mail")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("running deck-mail")?;
    if !out.status.success() {
        bail!(
            "{}",
            String::from_utf8_lossy(&out.stderr)
                .trim()
                .trim_start_matches("deck-mail: ")
        );
    }
    Ok(out.stdout)
}

/// The archive's categories, and the PDFs of these messages.
pub fn pdfs(msgs: &[String]) -> Result<(Vec<String>, Vec<Pdf>)> {
    let mut args = vec!["pdfs"];
    args.extend(msgs.iter().map(String::as_str));
    let p: Pdfs = serde_json::from_slice(&deck_mail(&args)?).context("reading deck-mail pdfs")?;
    Ok((p.categories, p.pdfs))
}

pub fn choose(sha: &str, choice: &Choice) -> Result<()> {
    match choice {
        Choice::To(c, n) => deck_mail(&["file", sha, c, n]),
        Choice::Out => deck_mail(&["file", sha, "-"]),
        Choice::Forget => deck_mail(&["file", sha, "--forget"]),
    }
    .map(|_| ())
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// An attachment deck-mail would look at as a PDF.
pub fn is_pdf(name: &str, ctype: &str) -> bool {
    ctype == "application/pdf" || name.to_ascii_lowercase().ends_with(".pdf")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_deck_mail() {
        let p: Pdfs = serde_json::from_str(
            r#"{"categories": ["Računi"], "pdfs": [
              {"message": "a@b", "file": "x.pdf", "sha256": "ab", "state": "waiting",
               "suggested": ["Računi", "HEP - račun"]},
              {"message": "a@b", "file": "y.pdf", "sha256": "cd", "state": "left out", "suggested": null},
              {"message": "a@b", "file": "z.pdf", "sha256": "ef", "state": "sent",
               "where": "2026/10/17-Racuni.pdf", "suggested": null}]}"#,
        )
        .unwrap();
        assert_eq!(p.pdfs[0].state, State::Waiting);
        assert_eq!(p.pdfs[0].suggested, Some(("Računi".into(), "HEP - račun".into())));
        assert_eq!(p.pdfs[1].choice(), Choice::Out);
        assert_eq!(p.pdfs[2].r#where.as_deref(), Some("2026/10/17-Racuni.pdf"));
        assert_eq!(sha256(b"abc").len(), 64);
    }
}
