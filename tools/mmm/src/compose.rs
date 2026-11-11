//! Writing mail: a draft in the editor, a look at it before it goes, and a minute to take it back.
//!
//! The draft is a text file: headers (From, To, Cc, Bcc, Subject, Attach), a blank line, what you
//! write, and below it the mail you reply to or forward. The signature isn't in it: it's chosen
//! by the language of what you wrote, shown on the review screen, and goes between your text and
//! the quote. Sending is `deck-mail send` (msmtpq), a minute later, from a systemd user timer:
//! until then the mail can be taken back, and quitting mmm doesn't stop it. The outbox's files
//! say what became of it, and a mail still there when mmm starts is shown again.

use crate::notmuch::{self, Message};
use crate::render;
use anyhow::{Context, Result, bail};
use chrono::{Local, TimeZone};
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;
use mail_builder::headers::text::Text;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use whatlang::{Detector, Lang};

/// The languages there are signatures for, in the order `l` steps through them.
pub const LANGS: &[(&str, &str)] = &[("hr", "Croatian"), ("en", "English"), ("de", "German")];

/// How long a mail waits after `y`, for `u` to take it back.
pub const UNDO_SECS: u64 = 60;

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Kind {
    New,
    Reply,
    Forward,
}

/// A draft is kept twice: as JSON beside the editor's file, which `e` resumes from, and as a
/// message in notmuch (folder mmm-drafts, tag `draft`), so it shows in the Drafts tab and, for
/// a reply, in its conversation. Gmail never sees it: mbsync only syncs the accounts' folders.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Draft {
    pub id: String,
    pub kind: Kind,
    /// The editor's file.
    pub file: PathBuf,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub attach: Vec<PathBuf>,
    /// What you wrote: the text above the quote.
    pub body: String,
    /// The line that starts the quote or the forwarded mail, as written in the draft; it's
    /// written again in the mail's language when that changes.
    pub lead: Option<String>,
    /// Below `lead`: the quoted or forwarded text.
    pub quote: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    /// The mail answered or forwarded: tagged `replied` or `passed` once this one is sent.
    pub original: Option<Original>,
    /// The signature's language: a code from `LANGS`, or none for no signature.
    #[serde(skip)]
    pub lang: Option<&'static str>,
    /// `lang` in the JSON.
    #[serde(default)]
    lang_code: Option<String>,
    /// Why that language: shown on the review screen.
    pub lang_why: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Original {
    pub id: String,
    pub from: String,
    pub timestamp: i64,
    /// The language its text is in, as a guess for this one's (not kept with a draft).
    #[serde(skip)]
    pub lang: Option<&'static str>,
}

/// The notmuch message id of a draft, and the draft id back from one.
pub fn draft_message_id(id: &str) -> String {
    format!("mmm-draft-{id}@mmm")
}

/// Only an id `new_id` makes: a Message-ID is the sender's to choose, and a draft id names files
/// (mmm-draft-../../x@mmm would reach out of the drafts folder).
pub fn draft_of(message_id: &str) -> Option<&str> {
    let id = message_id.strip_prefix("mmm-draft-")?.strip_suffix("@mmm")?;
    is_draft_id(id).then_some(id)
}

/// 20261018-113015-4242-0: date, time, process, counter.
fn is_draft_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.len() == 4
        && parts[0].len() == 8
        && parts[1].len() == 6
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("mmm")
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"))
        .join("mmm")
}

pub fn signature_file(lang: &str) -> PathBuf {
    config_dir().join("signatures").join(lang)
}

/// The signature for a language, as written in its file; `None` when there is no file.
pub fn signature(lang: &str) -> Option<String> {
    std::fs::read_to_string(signature_file(lang))
        .ok()
        .map(|s| s.trim_end().to_string())
        .filter(|s| !s.is_empty())
}

fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    format!(
        "{}-{}-{}",
        Local::now().format("%Y%m%d-%H%M%S"),
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

/// Who you are: `user.name` and the addresses in notmuch's config, the primary one first.
pub fn identities() -> Vec<String> {
    let get = |k: &str| {
        Command::new("notmuch")
            .args(["config", "get", k])
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    let name = get("user.name");
    let mut addrs = vec![get("user.primary_email")];
    addrs.extend(
        get("user.other_email")
            .split([';', '\n'])
            .map(|s| s.trim().to_string()),
    );
    addrs
        .into_iter()
        .filter(|a| !a.is_empty())
        .map(|a| {
            if name.is_empty() {
                a
            } else {
                format!("{name} <{a}>")
            }
        })
        .collect()
}

pub fn lang_code(l: Lang) -> Option<&'static str> {
    match l {
        Lang::Hrv => Some("hr"),
        Lang::Eng => Some("en"),
        Lang::Deu => Some("de"),
        _ => None,
    }
}

/// The language of a text, among the three, and whether whatlang is sure of it.
pub fn detect(text: &str) -> Option<(&'static str, bool)> {
    let words = text.split_whitespace().count();
    if words < 3 {
        return None;
    }
    let info = Detector::with_allowlist(vec![Lang::Hrv, Lang::Eng, Lang::Deu]).detect(text)?;
    // Short texts come out unreliable even when right; a dozen words of one language is enough.
    Some((
        lang_code(info.lang())?,
        info.is_reliable() || words >= 12 && info.confidence() > 0.5,
    ))
}

/// The plain text of a message, as quoted or forwarded: text parts as they are, HTML as text.
pub fn plain_text(m: &Message) -> String {
    render::plain_text(&m.body)
}

fn months(lang: &str) -> [&'static str; 12] {
    match lang {
        "de" => [
            "Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez.",
        ],
        _ => [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ],
    }
}

/// "On 16 Oct 2026 at 10:00, Ana <ana@x> wrote:", in the mail's language.
pub fn attribution(lang: Option<&str>, from: &str, ts: i64) -> String {
    let Some(t) = Local.timestamp_opt(ts, 0).single() else {
        return format!("{from}:");
    };
    use chrono::Datelike;
    let (d, m, y, hm) = (t.day(), t.month0() as usize, t.year(), t.format("%H:%M"));
    match lang.unwrap_or("en") {
        "hr" => format!("{d}. {}. {y}. u {hm}, {from} piše:", m + 1),
        "de" => format!("Am {d}. {} {y} um {hm} schrieb {from}:", months("de")[m]),
        _ => format!("On {d} {} {y} at {hm}, {from} wrote:", months("en")[m]),
    }
}

const FORWARD_LEAD: &str = "---------- Forwarded message ----------";

fn quoted(text: &str) -> String {
    text.trim_end()
        .lines()
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The JSON `notmuch reply` gives: the reply's headers, worked out by notmuch (who to, from
/// which of your addresses, Re:, In-Reply-To, References).
fn reply_headers(id: &str, all: bool) -> Result<HashMap<String, String>> {
    let out = Command::new("notmuch")
        .args([
            "reply",
            "--format=json",
            if all {
                "--reply-to=all"
            } else {
                "--reply-to=sender"
            },
            &format!("id:{}", notmuch::quote(id)),
        ])
        .stdin(Stdio::null())
        .output()
        .context("running notmuch reply")?;
    if !out.status.success() {
        bail!("notmuch reply: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).context("reading notmuch reply")?;
    Ok(v["reply-headers"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k.to_ascii_lowercase(), v.as_str()?.to_string())))
        .collect())
}

fn drafts_dir() -> PathBuf {
    state_dir().join("drafts")
}

impl Draft {
    fn empty(kind: Kind) -> Draft {
        let id = new_id();
        Draft {
            file: drafts_dir().join(format!("{id}.txt")),
            id,
            kind,
            from: identities().into_iter().next().unwrap_or_default(),
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            subject: String::new(),
            attach: vec![],
            body: String::new(),
            lead: None,
            quote: String::new(),
            in_reply_to: None,
            references: None,
            original: None,
            lang: None,
            lang_code: None,
            lang_why: String::new(),
        }
    }

    pub fn new_mail() -> Draft {
        Draft::empty(Kind::New)
    }

    pub fn reply(m: &Message, all: bool) -> Result<Draft> {
        let h = reply_headers(&m.id, all)?;
        let text = plain_text(m);
        let lang = detect(&text).map(|(l, _)| l);
        let mut d = Draft::empty(Kind::Reply);
        let get = |k: &str| h.get(k).cloned().unwrap_or_default();
        if !get("from").is_empty() {
            d.from = get("from");
        }
        d.to = get("to");
        d.cc = get("cc");
        d.subject = get("subject");
        d.in_reply_to = h.get("in-reply-to").cloned();
        d.references = h.get("references").cloned();
        d.lead = Some(attribution(lang, &m.from, m.timestamp));
        d.quote = quoted(&text);
        d.original = Some(Original {
            id: m.id.clone(),
            from: m.from.clone(),
            timestamp: m.timestamp,
            lang,
        });
        Ok(d)
    }

    /// A forward: the mail's text below, its attachments written out to go along.
    pub fn forward(m: &Message) -> Result<Draft> {
        let text = plain_text(m);
        let mut d = Draft::empty(Kind::Forward);
        let s = m.subject.trim();
        d.subject = if s.to_ascii_lowercase().starts_with("fwd:") {
            s.to_string()
        } else {
            format!("Fwd: {s}")
        };
        let date = Local
            .timestamp_opt(m.timestamp, 0)
            .single()
            .map(|t| t.format("%a, %-d %b %Y at %H:%M").to_string())
            .unwrap_or_default();
        d.lead = Some(FORWARD_LEAD.to_string());
        let mut head = format!(
            "From: {}\nDate: {date}\nSubject: {}\nTo: {}\n",
            m.from, m.subject, m.to
        );
        if !m.cc.is_empty() {
            head += &format!("Cc: {}\n", m.cc);
        }
        d.quote = format!("{head}\n{}", text.trim_end());
        let dir = drafts_dir().join(&d.id);
        for (part, name) in render::attachments(&m.body) {
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(safe_name(&name, part));
            std::fs::write(&path, notmuch::part(&m.id, part)?)?;
            d.attach.push(path);
        }
        d.original = Some(Original {
            id: m.id.clone(),
            from: m.from.clone(),
            timestamp: m.timestamp,
            lang: detect(&text).map(|(l, _)| l),
        });
        Ok(d)
    }

    /// The draft as the editor shows it, and the line the cursor starts on.
    pub fn text(&self) -> (String, usize) {
        // One line each: a subject or address from a mail with a line break in it would add
        // header lines of its own (Bcc:, Attach: ~/.ssh/...).
        let mut t = format!(
            "From: {}\nTo: {}\nCc: {}\nBcc: {}\nSubject: {}\n",
            one_line(&self.from),
            one_line(&self.to),
            one_line(&self.cc),
            one_line(&self.bcc),
            one_line(&self.subject)
        );
        for a in &self.attach {
            t += &format!("Attach: {}\n", one_line(&tilde(a)));
        }
        if self.attach.is_empty() {
            t += "Attach: \n";
        }
        t += "\n";
        let line = t.lines().count() + 1;
        let body = self.body.trim_end();
        t += body;
        t += "\n";
        if let Some(lead) = &self.lead {
            t += &format!("\n{lead}\n{}\n", self.quote);
        }
        (t, line)
    }

    /// Keeps the draft: its JSON, and its message in notmuch, made or brought up to date.
    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(drafts_dir())?;
        let mut kept = self.clone();
        kept.lang_code = self.lang.map(str::to_string);
        write_whole(
            &self.file.with_extension("json"),
            &serde_json::to_vec_pretty(&kept)?,
        )?;
        let raw = self.message(true)?;
        let q = format!("id:{}", notmuch::quote(&draft_message_id(&self.id)));
        let files = notmuch_files(&q)?;
        let run = |args: &[&str], input: Option<&[u8]>| -> Result<()> {
            let mut child = Command::new("notmuch")
                .args(args)
                .stdin(if input.is_some() {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .context("running notmuch")?;
            if let (Some(i), Some(mut w)) = (input, child.stdin.take()) {
                use std::io::Write;
                w.write_all(i)?;
            }
            let out = child.wait_with_output()?;
            if !out.status.success() {
                bail!(
                    "notmuch {}: {}",
                    args[0],
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Ok(())
        };
        match files.first() {
            Some(f) => {
                std::fs::write(f, &raw)?;
                run(&["reindex", &q], None)
            }
            None => run(
                &[
                    "insert",
                    "--folder=mmm-drafts",
                    "--create-folder",
                    "-new",
                    "-inbox",
                    "-unread",
                    "+draft",
                ],
                Some(&raw),
            ),
        }
    }

    /// A draft kept with `save`, back: from its JSON, or from the editor's file when that's
    /// newer (mmm stopped while the editor was open).
    pub fn load(id: &str) -> Result<Draft> {
        if !is_draft_id(id) {
            bail!("not a draft of mmm's: {id}");
        }
        let json = drafts_dir().join(format!("{id}.json"));
        let mut d: Draft = serde_json::from_slice(&std::fs::read(&json).context("no such draft")?)
            .context("reading the draft")?;
        // Its files from its id, not from what the JSON says: they are written and removed.
        d.file = drafts_dir().join(format!("{id}.txt"));
        d.id = id.to_string();
        d.lang = d
            .lang_code
            .as_deref()
            .and_then(|c| LANGS.iter().find(|(x, _)| *x == c).map(|(x, _)| *x));
        let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        if let (Some(t), Some(j)) = (modified(&d.file), modified(&json))
            && t > j
        {
            d.read()?;
        }
        Ok(d)
    }

    /// Sent or thrown away: its message out of notmuch (tagged deleted, the file gone, and
    /// `notmuch new` to forget it; a reply's draft left half there breaks its whole thread) and
    /// its files.
    pub fn forget(&self) -> Result<()> {
        let q = format!("id:{}", notmuch::quote(&draft_message_id(&self.id)));
        let files = notmuch_files(&q)?;
        if !files.is_empty() {
            notmuch::tag(&[draft_message_id(&self.id)], "+deleted")?;
        }
        for f in &files {
            let _ = std::fs::remove_file(f);
        }
        if !files.is_empty() {
            // Should this fail (a sync holds the database), showing the thread tries again.
            let _ = notmuch::forget_gone();
        }
        for p in [self.file.clone(), self.file.with_extension("json")] {
            let _ = std::fs::remove_file(p);
        }
        let _ = std::fs::remove_dir_all(self.file.with_extension(""));
        Ok(())
    }

    pub fn write(&self) -> Result<usize> {
        std::fs::create_dir_all(drafts_dir())?;
        let (text, line) = self.text();
        std::fs::write(&self.file, text)?;
        Ok(line)
    }

    /// Reads the editor's file back: the headers, your text, and the quote below the lead line
    /// (when it's still there; without it, everything is your text).
    pub fn read(&mut self) -> Result<()> {
        let text = std::fs::read_to_string(&self.file)?;
        self.parse(&text);
        Ok(())
    }

    pub fn parse(&mut self, text: &str) {
        let (head, rest) = match text.split_once("\n\n") {
            Some((h, r)) => (h, r),
            None => (text, ""),
        };
        self.attach.clear();
        for line in head.lines() {
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let v = v.trim().to_string();
            match k.trim().to_ascii_lowercase().as_str() {
                "from" => self.from = v,
                "to" => self.to = v,
                "cc" => self.cc = v,
                "bcc" => self.bcc = v,
                "subject" => self.subject = v,
                "attach" if !v.is_empty() => self.attach.push(untilde(&v)),
                _ => {}
            }
        }
        let lead_at = self
            .lead
            .as_ref()
            .and_then(|l| rest.lines().position(|x| x.trim_end() == l.as_str()));
        let lines: Vec<&str> = rest.lines().collect();
        match lead_at {
            Some(i) => {
                self.body = lines[..i].join("\n").trim_end().to_string();
                self.quote = lines[i + 1..].join("\n").trim_end().to_string();
            }
            None => {
                self.body = rest.trim_end().to_string();
                self.lead = None;
                self.quote.clear();
            }
        }
    }

    /// Picks the signature's language: what you wrote, if whatlang is sure; else the language you
    /// last wrote to the first recipient in; else the one of the mail you answer; else English.
    pub fn choose_lang(&mut self) {
        let detected = detect(&self.body);
        let first = addresses(&self.to)
            .ok()
            .and_then(|a| a.into_iter().next())
            .map(|(_, a)| a.to_ascii_lowercase());
        let remembered = first.as_ref().and_then(|a| remembered_langs().get(a).copied());
        let name = |l: &str| {
            LANGS
                .iter()
                .find(|(c, _)| *c == l)
                .map_or(l, |(_, n)| *n)
                .to_string()
        };
        (self.lang, self.lang_why) = match (detected, remembered, self.original.as_ref().and_then(|o| o.lang))
        {
            (Some((l, true)), ..) => (Some(l), format!("{} is what you wrote in", name(l))),
            (_, Some(l), _) => (
                Some(l),
                format!("you last wrote to {} in {}", first.unwrap_or_default(), name(l)),
            ),
            (Some((l, false)), ..) => (Some(l), format!("{} is what you wrote in, probably", name(l))),
            (None, None, Some(l)) => (Some(l), format!("the mail you answer is in {}", name(l))),
            _ => (Some("en"), "too little text to tell: English".into()),
        };
        self.relead();
    }

    pub fn set_lang(&mut self, lang: Option<&'static str>) {
        self.lang = lang;
        self.lang_why = "chosen with l".into();
        self.relead();
    }

    /// A reply's "On … wrote:" line, in the mail's language.
    fn relead(&mut self) {
        if let (Kind::Reply, Some(o), Some(_)) = (self.kind, &self.original, &self.lead) {
            self.lead = Some(attribution(self.lang, &o.from, o.timestamp));
        }
    }

    /// The text that goes: yours, the signature, then the quote or the forwarded mail.
    pub fn final_text(&self) -> String {
        let mut t = self.body.trim_end().to_string();
        if let Some(sig) = self.lang.and_then(signature) {
            t += "\n\n";
            t += &sig;
        }
        if let Some(lead) = &self.lead {
            t += &format!("\n\n{lead}\n{}", self.quote.trim_end());
        }
        t + "\n"
    }

    pub fn recipients(&self) -> Result<Vec<String>> {
        let mut all = vec![];
        for (field, v) in [("To", &self.to), ("Cc", &self.cc), ("Bcc", &self.bcc)] {
            all.extend(
                addresses(v)
                    .with_context(|| format!("{field}: {v}"))?
                    .into_iter()
                    .map(|(_, a)| a),
            );
        }
        all.dedup();
        Ok(all)
    }

    /// What's wrong (it can't go) and what's worth a look (it can).
    pub fn check(&self) -> (Vec<String>, Vec<String>) {
        let (mut errors, mut warnings) = (vec![], vec![]);
        match self.recipients() {
            Ok(r) if r.is_empty() => errors.push("no one to send it to: To is empty".into()),
            Ok(r) => {
                if let Some(bad) = r.iter().find(|a| {
                    !a.contains('@')
                        || a.starts_with('-')
                        || a.chars().any(|c| c.is_whitespace() || c.is_control())
                }) {
                    errors.push(format!("not an address: {bad}"));
                }
            }
            Err(e) => errors.push(format!("{e:#}")),
        }
        match addresses(&self.from) {
            Ok(a) if a.len() == 1 => {}
            _ => errors.push(format!("From must be one address: {}", self.from)),
        }
        for a in &self.attach {
            if !a.is_file() {
                errors.push(format!("no such file to attach: {}", tilde(a)));
            }
        }
        if self.subject.trim().is_empty() {
            warnings.push("no subject".into());
        }
        if self.body.trim().is_empty() {
            warnings.push("you wrote nothing".into());
        }
        let lower = self.body.to_lowercase();
        let mentions = [
            "attach",
            "enclosed",
            "privit",
            "prilog",
            "prilož",
            "anhang",
            "angehängt",
            "anbei",
            "beigefügt",
        ]
        .iter()
        .find(|w| lower.contains(*w));
        if let (Some(w), true) = (mentions, self.attach.is_empty()) {
            warnings.push(format!(
                "you mention an attachment (\"{w}…\") but nothing is attached"
            ));
        }
        if let Some(l) = self.lang
            && signature(l).is_none()
        {
            warnings.push(format!(
                "no signature for this language: {} is missing",
                tilde(&signature_file(l))
            ));
        }
        (errors, warnings)
    }

    /// The mail as it goes out, with LF line ends for msmtp.
    pub fn build(&self) -> Result<Vec<u8>> {
        self.message(false)
    }

    /// The mail, or with `draft` its draft: its own message id, no signature or attachments,
    /// and the headers as typed, half-written addresses too.
    fn message(&self, draft: bool) -> Result<Vec<u8>> {
        if draft {
            let mut b = MessageBuilder::new()
                .header("From", Text::new(self.from.clone()))
                .subject(self.subject.clone())
                .date(Local::now().timestamp())
                .message_id(draft_message_id(&self.id))
                .header("User-Agent", Text::new("mmm"));
            for (k, v) in [("To", &self.to), ("Cc", &self.cc), ("Bcc", &self.bcc)] {
                if !v.trim().is_empty() {
                    b = b.header(k, Text::new(v.clone()));
                }
            }
            if let Some(r) = &self.in_reply_to {
                b = b.in_reply_to(message_ids(r));
            }
            if let Some(r) = &self.references {
                b = b.references(message_ids(r));
            }
            let mut text = self.body.trim_end().to_string();
            for a in &self.attach {
                text += &format!("\n\n📎 {}", tilde(a));
            }
            if let Some(lead) = &self.lead {
                text += &format!("\n\n{lead}\n{}", self.quote.trim_end());
            }
            let raw = b.text_body(text + "\n").write_to_vec()?;
            return Ok(String::from_utf8_lossy(&raw).replace("\r\n", "\n").into_bytes());
        }
        let from = addresses(&self.from)?.into_iter().next().context("no From")?;
        let domain = from.1.rsplit('@').next().unwrap_or("localhost").to_string();
        let list = |v: &str| -> Result<Address<'static>> {
            Ok(Address::new_list(
                addresses(v)?
                    .into_iter()
                    .map(|(n, a)| match n {
                        Some(n) => Address::from((n, a)),
                        None => Address::from(a),
                    })
                    .collect(),
            ))
        };
        let from_addr = match from.0 {
            Some(n) => Address::from((n, from.1.clone())),
            None => Address::from(from.1.clone()),
        };
        let mut b = MessageBuilder::new()
            .from(from_addr)
            .subject(self.subject.clone())
            .date(Local::now().timestamp())
            .message_id(format!("{}@{domain}", self.id))
            .header("User-Agent", Text::new("mmm"))
            .text_body(self.final_text());
        if !self.to.trim().is_empty() {
            b = b.to(list(&self.to)?);
        }
        if !self.cc.trim().is_empty() {
            b = b.cc(list(&self.cc)?);
        }
        // Bcc goes as a recipient only, never as a header.
        if let Some(r) = &self.in_reply_to {
            b = b.in_reply_to(message_ids(r));
        }
        if let Some(r) = &self.references {
            b = b.references(message_ids(r));
        }
        for a in &self.attach {
            let name = a
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let bytes = std::fs::read(a).with_context(|| tilde(a))?;
            b = b.attachment(content_type(&name), name, bytes);
        }
        let raw = b.write_to_vec()?;
        Ok(String::from_utf8_lossy(&raw).replace("\r\n", "\n").into_bytes())
    }
}

fn message_ids(v: &str) -> Vec<String> {
    v.split_whitespace()
        .map(|i| i.trim_matches(['<', '>']).to_string())
        .filter(|i| !i.is_empty())
        .collect()
}

fn notmuch_files(query: &str) -> Result<Vec<PathBuf>> {
    let out = Command::new("notmuch")
        .args(["search", "--output=files", "--exclude=false", query])
        .stdin(Stdio::null())
        .output()
        .context("running notmuch")?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect())
}

fn content_type(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "txt" | "md" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "ics" => "text/calendar",
        _ => "application/octet-stream",
    }
}

fn safe_name(name: &str, part: u64) -> String {
    let n: String = name
        .chars()
        .map(|c| if c == '/' || c == '\0' { '_' } else { c })
        .collect::<String>()
        .trim_start_matches('.')
        .to_string();
    if n.is_empty() || n.starts_with('(') {
        format!("part-{part}")
    } else {
        n
    }
}

/// An address list as (name, address) pairs.
pub fn addresses(v: &str) -> Result<Vec<(Option<String>, String)>> {
    if v.trim().is_empty() {
        return Ok(vec![]);
    }
    let list = mailparse::addrparse(v).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut out = vec![];
    for a in list.iter() {
        match a {
            mailparse::MailAddr::Single(s) => out.push((s.display_name.clone(), s.addr.clone())),
            mailparse::MailAddr::Group(g) => {
                out.extend(g.addrs.iter().map(|s| (s.display_name.clone(), s.addr.clone())))
            }
        }
    }
    Ok(out)
}

/// Control characters (line breaks among them) as spaces.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

pub fn tilde(p: &Path) -> String {
    let h = home();
    match p.strip_prefix(&h) {
        Ok(rest) if !h.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

fn untilde(s: &str) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(s),
    }
}

/// Writes a file so that it is there whole or not at all: to a file beside it, flushed to the
/// disk, then renamed over it. A crash or a full disk leaves what was there before.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let name = path.file_name().context("a file to write")?.to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let written = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written.with_context(|| format!("writing {}", path.display()))
}

fn langs_file() -> PathBuf {
    state_dir().join("languages")
}

/// The language you last wrote to each address in: `address<TAB>code`, a line each.
fn remembered_langs() -> HashMap<String, &'static str> {
    std::fs::read_to_string(langs_file())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (a, c) = l.split_once('\t')?;
            let code = LANGS.iter().find(|(x, _)| *x == c.trim())?.0;
            Some((a.to_string(), code))
        })
        .collect()
}

fn remember_lang(recipients: &[String], lang: &'static str) {
    let mut all = remembered_langs();
    for r in recipients {
        all.insert(r.to_ascii_lowercase(), lang);
    }
    let mut lines: Vec<String> = all.into_iter().map(|(a, c)| format!("{a}\t{c}")).collect();
    lines.sort();
    let _ = std::fs::write(langs_file(), lines.join("\n") + "\n");
}

/// What became of a mail after `y`, as the outbox shows it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fate {
    /// In its minute, or being sent right now.
    Waiting,
    /// Handed to `deck-mail send`: out of the outbox, and in the log of what was sent.
    Sent,
    /// Still in the outbox with no timer to send it: a logout or a reboot inside its minute
    /// took the timer along.
    Lost,
    /// `deck-mail send` failed: the script left it in the outbox, as `.eml.sending`.
    Failed,
    /// Out of the outbox and not sent: taken back from somewhere else.
    Gone,
}

/// A mail in the outbox: it goes when `unit`'s timer fires, unless taken back before. Made by
/// `queue`, or found in the outbox when mmm starts; what it says is read from the outbox's
/// files, so it holds for a mail queued by an mmm that has since quit.
pub struct Pending {
    pub draft: Draft,
    pub unit: String,
    pub eml: PathBuf,
    /// When `y` was pressed: the outbox file's time.
    pub at: std::time::SystemTime,
    pub to: String,
    pub fate: Fate,
    /// The log the script writes a line to for each mail sent.
    log: PathBuf,
}

/// The user manager's part: the timer that sends, whether it or its script is at work, and
/// stopping it. Tests stand in for it.
trait Timers {
    /// Starts the timer that runs the send script with `args` in a minute.
    fn start(&self, unit: &str, description: &str, args: &[std::ffi::OsString]) -> bool;
    /// The timer waits, or its script runs.
    fn running(&self, unit: &str) -> bool;
    fn stop(&self, unit: &str);
}

struct Systemd;

impl Timers for Systemd {
    fn start(&self, unit: &str, description: &str, args: &[std::ffi::OsString]) -> bool {
        // The unit runs in the user manager, not here: it gets mmm's PATH, and its notmuch
        // database when that's not the default one.
        let mut env = vec!["--setenv=PATH".to_string()];
        if std::env::var_os("NOTMUCH_CONFIG").is_some() {
            env.push("--setenv=NOTMUCH_CONFIG".into());
        }
        Command::new("systemd-run")
            .args([
                "--user",
                "--quiet",
                "--collect",
                // The arguments go to the script as they are: a Message-ID or an address is
                // the sender's to choose, and systemd would put its environment in for `${…}`.
                "--expand-environment=no",
                &format!("--unit={unit}"),
                &format!("--on-active={UNDO_SECS}"),
                "--timer-property=AccuracySec=1s",
                &format!("--description={description}"),
            ])
            .args(&env)
            .args(["--", "/bin/sh", "-c", SEND, "mmm-send"])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn running(&self, unit: &str) -> bool {
        Command::new("systemctl")
            .args(["--user", "is-active", "--quiet"])
            .args([format!("{unit}.timer"), format!("{unit}.service")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn stop(&self, unit: &str) {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", &format!("{unit}.timer")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// The script the timer runs: takes the mail out of the outbox (so a take-back that got there
/// first wins), hands it to `deck-mail send`, tags the mail it answers, and clears the draft
/// away: its files (the editor's, its JSON, the forwarded mail's attachments) and its message
/// in notmuch, as `Draft::forget` does. The log gets its line before the mail leaves the
/// outbox: a mail gone from there is in the log, or it wasn't sent. A mail that didn't go
/// stays, as `.eml.sending`, for mmm to show.
const SEND: &str = r#"set -u
f="$1"; base="$2"; query="$3"; tag="$4"; send="$5"; log="$6"; draft="$7"; shift 7
mv -- "$f" "$f.sending" 2>/dev/null || exit 0
if $send "$@" < "$f.sending"; then
    printf '%s\tsent\t%s\n' "$(date -Is)" "$(basename "$f")" >> "$log"
    rm -rf -- "$f.sending" "$base.txt" "$base.json" "$base"
    notmuch tag +deleted -- "$draft"
    notmuch search --output=files --exclude=false "$draft" | while IFS= read -r m; do rm -f -- "$m"; done
    notmuch new --quiet --no-hooks || true
    if [ -n "$query" ]; then notmuch tag "+$tag" -- "$query"; fi
else
    printf '%s\tfailed\t%s\n' "$(date -Is)" "$(basename "$f")" >> "$log"
    notify-send -u critical "mmm: a mail didn't go" "It is in $f.sending; mmm shows it, and u there takes it back"
fi
"#;

fn unit_of(id: &str) -> String {
    format!("mmm-send-{id}")
}

fn sending(eml: &Path) -> PathBuf {
    eml.with_extension("eml.sending")
}

/// Who a mail goes to, for the footer: the first of To, and how many more.
fn to_label(d: &Draft) -> String {
    let to = addresses(&d.to)
        .ok()
        .and_then(|a| a.into_iter().next())
        .map(|(n, a)| n.unwrap_or(a))
        .unwrap_or_default();
    match d.recipients().map_or(0, |r| r.len().saturating_sub(1)) {
        0 => to,
        more => format!("{to} and {more} more"),
    }
}

#[derive(PartialEq, Debug)]
enum Withdrawn {
    /// It was in the outbox; it is out, and its timer stopped.
    Taken,
    /// Nothing of it was there.
    Nothing,
    /// The script is sending it right now.
    Busy,
}

/// Takes a mail out of the outbox, whatever state it is in there. The outbox file is the lock:
/// whoever moves it first, this or the timer's script, has it. A mail the script has moved is
/// the script's while it runs; left behind by a send that failed, it can be taken.
fn withdraw(eml: &Path, unit: &str, timers: &dyn Timers) -> Withdrawn {
    let back = eml.with_extension("eml.taken-back");
    if std::fs::rename(eml, &back).is_ok() {
        timers.stop(unit);
        let _ = std::fs::remove_file(back);
        return Withdrawn::Taken;
    }
    let left = sending(eml);
    if !left.exists() {
        return Withdrawn::Nothing;
    }
    if timers.running(unit) {
        return Withdrawn::Busy;
    }
    match std::fs::rename(&left, &back) {
        Ok(()) => {
            let _ = std::fs::remove_file(back);
            Withdrawn::Taken
        }
        Err(_) => Withdrawn::Nothing,
    }
}

/// `y`: the mail goes in a minute, from a systemd user timer. The timer lives in the user
/// manager: it outlives mmm, but not a logout or a reboot, after which the mail is still in the
/// outbox and `outbox` finds it.
pub fn queue(d: &Draft) -> Result<Pending> {
    let (errors, _) = d.check();
    if let Some(e) = errors.first() {
        bail!("{e}");
    }
    // The draft stays until it's sent, so a mail taken back or lost is still there.
    d.save()?;
    // MMM_SEND replaces `deck-mail send`, for tests.
    let send = std::env::var("MMM_SEND").unwrap_or_else(|_| "deck-mail send".into());
    let p = queue_in(&state_dir(), d, &send, &Systemd)?;
    if let Some(l) = d.lang {
        remember_lang(&d.recipients()?, l);
    }
    Ok(p)
}

fn queue_in(state: &Path, d: &Draft, send: &str, timers: &dyn Timers) -> Result<Pending> {
    let raw = d.build()?;
    let recipients = d.recipients()?;
    let outbox = state.join("outbox");
    std::fs::create_dir_all(&outbox)?;
    let eml = outbox.join(format!("{}.eml", d.id));
    let unit = unit_of(&d.id);
    // Sent once already and still in its minute (the draft resumed and sent again): that send
    // is called off first, its timer stopped, and this one takes its place. One the script is
    // sending right now is left alone.
    if withdraw(&eml, &unit, timers) == Withdrawn::Busy {
        bail!("it is being sent right now");
    }
    write_whole(&eml, &raw)?;
    let (query, tag) = match (&d.original, d.kind) {
        (Some(o), Kind::Forward) => (format!("id:{}", notmuch::quote(&o.id)), "passed"),
        (Some(o), _) => (format!("id:{}", notmuch::quote(&o.id)), "replied"),
        (None, _) => (String::new(), "replied"),
    };
    let log = state.join("sent.log");
    let mut args: Vec<std::ffi::OsString> = vec![eml.clone().into(), d.file.with_extension("").into()];
    args.extend([query.as_str(), tag, send].map(Into::into));
    args.push(log.clone().into());
    args.push(format!("id:{}", notmuch::quote(&draft_message_id(&d.id))).into());
    args.extend(recipients.iter().map(Into::into));
    let description = format!("mmm: a mail to {}", recipients.join(", "));
    if !timers.start(&unit, &description, &args) {
        // No timer has this file: an earlier one was stopped above, this one didn't start.
        let _ = std::fs::remove_file(&eml);
        bail!("systemd-run couldn't start the send timer");
    }
    Ok(Pending {
        to: to_label(d),
        draft: d.clone(),
        unit,
        eml,
        at: std::time::SystemTime::now(),
        fate: Fate::Waiting,
        log,
    })
}

/// What is in the outbox when mmm starts, oldest first: mail still in its minute (queued by an
/// mmm that quit), mail whose timer a logout or a reboot took, and mail that failed to send.
/// Each is a `Pending` again, to take back with `u`. Beside them, the outbox files whose draft
/// is no longer there to go back to.
pub fn outbox() -> (Vec<Pending>, Vec<PathBuf>) {
    scan(&state_dir(), &Systemd, Draft::load)
}

fn scan(
    state: &Path,
    timers: &dyn Timers,
    load: impl Fn(&str) -> Result<Draft>,
) -> (Vec<Pending>, Vec<PathBuf>) {
    let (mut found, mut alone) = (vec![], vec![]);
    for e in std::fs::read_dir(state.join("outbox"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = e.file_name().to_string_lossy().into_owned();
        let (id, taken) = match (name.strip_suffix(".eml"), name.strip_suffix(".eml.sending")) {
            (Some(id), _) => (id, false),
            (_, Some(id)) => (id, true),
            _ => continue,
        };
        if !is_draft_id(id) {
            continue;
        }
        let Ok(draft) = load(id) else {
            alone.push(e.path());
            continue;
        };
        let unit = unit_of(id);
        let fate = match (timers.running(&unit), taken) {
            (true, _) => Fate::Waiting,
            (false, false) => Fate::Lost,
            (false, true) => Fate::Failed,
        };
        found.push(Pending {
            to: to_label(&draft),
            draft,
            unit,
            eml: e.path().with_file_name(format!("{id}.eml")),
            at: e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            fate,
            log: state.join("sent.log"),
        });
    }
    found.sort_by_key(|p| p.at);
    alone.sort();
    (found, alone)
}

/// Calls off the queued send of a draft, if there is one: before the draft is thrown away, or
/// the mail would go all the same. False when the mail is being sent right now.
pub fn cancel(id: &str) -> bool {
    let eml = state_dir().join("outbox").join(format!("{id}.eml"));
    withdraw(&eml, &unit_of(id), &Systemd) != Withdrawn::Busy
}

impl Pending {
    /// Seconds of its minute left; none for a mail whose time is past (or not yet come).
    pub fn left(&self) -> u64 {
        UNDO_SECS.saturating_sub(self.at.elapsed().map_or(UNDO_SECS, |e| e.as_secs()))
    }

    /// Looks at the outbox again: `fate` is what the files say now.
    pub fn refresh(&mut self) {
        self.fate = self.fate_now(&Systemd);
    }

    fn fate_now(&self, timers: &dyn Timers) -> Fate {
        let files = || (self.eml.exists(), sending(&self.eml).exists());
        let gone = || {
            let line = format!(
                "\tsent\t{}",
                self.eml.file_name().unwrap_or_default().to_string_lossy()
            );
            let sent =
                std::fs::read_to_string(&self.log).is_ok_and(|l| l.lines().any(|l| l.ends_with(&line)));
            if sent { Fate::Sent } else { Fate::Gone }
        };
        // What the files alone tell; the user manager is only asked when they don't.
        match (files(), self.fate) {
            ((false, false), _) => return gone(),
            ((true, _), Fate::Lost) => return Fate::Lost,
            ((true, _), _) if self.left() > 0 => return Fate::Waiting,
            ((false, true), Fate::Failed) => return Fate::Failed,
            _ => {}
        }
        if timers.running(&self.unit) {
            return Fate::Waiting;
        }
        // Looked at again: the script may have finished in between.
        match files() {
            (true, _) => Fate::Lost,
            (false, true) => Fate::Failed,
            (false, false) => gone(),
        }
    }

    /// `u`: takes the mail back, if it hasn't gone: one in its minute, one whose timer is lost,
    /// or one that failed to send. From the outbox's files alone, so it works on a mail found
    /// there at the start as on one just queued.
    pub fn take_back(&self) -> Result<bool> {
        Ok(self.take_back_from(&Systemd))
    }

    fn take_back_from(&self, timers: &dyn Timers) -> bool {
        withdraw(&self.eml, &self.unit, timers) == Withdrawn::Taken
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(kind: Kind) -> Draft {
        let mut d = Draft::empty(kind);
        d.from = "Marko <m@example.org>".into();
        d
    }

    #[test]
    fn reads_back_what_the_editor_saved() {
        let mut d = draft(Kind::Reply);
        d.lead = Some("On 16 Oct 2026 at 10:00, Ana <ana@example.org> wrote:".into());
        d.quote = "> Bok".into();
        d.to = "Ana <ana@example.org>".into();
        let (text, line) = d.text();
        assert_eq!(text.lines().nth(line - 1), Some(""), "{text}");
        let edited = text
            .replacen("\n\n\n", "\n\nVidimo se sutra.\n\n", 1)
            .replace("Attach: \n", "Attach: ~/a.pdf\nAttach: /tmp/b.png\n");
        d.parse(&edited);
        assert_eq!(d.body, "Vidimo se sutra.");
        assert_eq!(d.quote, "> Bok");
        assert_eq!(d.attach, [home().join("a.pdf"), PathBuf::from("/tmp/b.png")]);
        assert_eq!(d.to, "Ana <ana@example.org>");
    }

    #[test]
    fn a_deleted_quote_leaves_only_the_text() {
        let mut d = draft(Kind::Reply);
        d.lead = Some("On …, Ana wrote:".into());
        d.parse("To: a@b\nSubject: x\n\nJust this.\n");
        assert_eq!(d.body, "Just this.");
        assert_eq!(d.lead, None);
        assert!(d.final_text().ends_with("Just this.\n"));
    }

    #[test]
    fn recipients_and_bcc() {
        let mut d = draft(Kind::New);
        d.to = "\"Horvat, Ana\" <ana@example.org>, ivo@example.org".into();
        d.cc = "Šime Šimić <sime@example.hr>".into();
        d.bcc = "me@example.org".into();
        d.subject = "Ručak u petak?".into();
        d.body = "Može.".into();
        assert_eq!(
            d.recipients().unwrap(),
            [
                "ana@example.org",
                "ivo@example.org",
                "sime@example.hr",
                "me@example.org"
            ]
        );
        let raw = String::from_utf8(d.build().unwrap()).unwrap();
        assert!(!raw.contains("\r\n"));
        assert!(!raw.to_ascii_lowercase().contains("bcc"), "{raw}");
        assert!(raw.contains("Subject: =?utf-8?"), "{raw}");
        assert!(raw.contains("\"Horvat, Ana\" <ana@example.org>"), "{raw}");
        assert!(
            raw.contains("=?utf-8?") && raw.contains("<sime@example.hr>"),
            "{raw}"
        );
    }

    #[test]
    fn reply_headers_carry_the_thread() {
        let mut d = draft(Kind::Reply);
        d.to = "a@example.org".into();
        d.subject = "Re: x".into();
        d.in_reply_to = Some("<abc@x>".into());
        d.references = Some("<one@x> <abc@x>".into());
        let raw = String::from_utf8(d.build().unwrap()).unwrap();
        assert!(raw.contains("In-Reply-To: <abc@x>"), "{raw}");
        assert!(raw.contains("References: <one@x> <abc@x>"), "{raw}");
    }

    #[test]
    fn checks() {
        let mut d = draft(Kind::New);
        d.body = "U prilogu je ugovor.".into();
        let (errors, warnings) = d.check();
        assert!(errors.iter().any(|e| e.contains("To is empty")));
        assert!(warnings.iter().any(|w| w.contains("nothing is attached")));
        assert!(warnings.iter().any(|w| w.contains("no subject")));
        d.to = "-oProxyCommand@x".into();
        assert!(d.check().0.iter().any(|e| e.contains("not an address")));
    }

    #[test]
    fn a_draft_is_its_own_message() {
        let mut d = draft(Kind::Reply);
        d.to = "Ana Horvat <ana@exam".into(); // half typed
        d.subject = "Re: Ručak".into();
        d.body = "Može.".into();
        d.lead = Some("On …, Ana wrote:".into());
        d.quote = "> Ručak?".into();
        d.in_reply_to = Some("<abc@x>".into());
        d.lang = Some("hr");
        let raw = String::from_utf8(d.message(true).unwrap()).unwrap();
        assert!(
            raw.contains(&format!("Message-ID: <{}>", draft_message_id(&d.id))),
            "{raw}"
        );
        assert!(raw.contains("In-Reply-To: <abc@x>"), "{raw}");
        assert!(raw.contains("Ana Horvat <ana@exam"), "{raw}");
        assert_eq!(draft_of(&draft_message_id(&d.id)), Some(d.id.as_str()));
        assert_eq!(draft_of("abc@x"), None);
        // Kept as JSON and back, the language with it.
        let mut kept = d.clone();
        kept.lang_code = d.lang.map(str::to_string);
        let mut back: Draft = serde_json::from_slice(&serde_json::to_vec(&kept).unwrap()).unwrap();
        assert_eq!(back.lang, None);
        back.lang = back
            .lang_code
            .as_deref()
            .and_then(|c| LANGS.iter().find(|(x, _)| *x == c).map(|(x, _)| *x));
        assert_eq!(back.lang, Some("hr"));
        assert_eq!((back.body, back.quote, back.kind), (d.body, d.quote, Kind::Reply));
    }

    #[test]
    fn languages() {
        assert_eq!(
            detect("Bok Ana, vidimo se sutra u deset ispred škole, ponesi kišobran jer će padati kiša."),
            Some(("hr", true))
        );
        assert_eq!(
            detect("Hallo Ana, wir sehen uns morgen um zehn vor der Schule, bring einen Regenschirm mit."),
            Some(("de", true))
        );
        assert_eq!(
            detect(
                "Hi Ana, see you tomorrow at ten in front of the school, bring an umbrella because it will rain."
            ),
            Some(("en", true))
        );
        assert_eq!(detect("Ok"), None);
        let de = attribution(Some("de"), "Ana", 0);
        assert!(
            de.starts_with("Am 1. Jan. 1970 um ") && de.ends_with(" schrieb Ana:"),
            "{de}"
        );
        assert!(attribution(Some("hr"), "Ana", 0).starts_with("1. 1. 1970. u "));
    }

    #[test]
    fn only_its_own_draft_ids() {
        assert_eq!(
            draft_of("mmm-draft-20261018-113015-4242-0@mmm"),
            Some("20261018-113015-4242-0")
        );
        for bad in [
            "mmm-draft-../../../run/user/1000/mmm/evil@mmm",
            "mmm-draft-20261018-113015-4242-0/../x@mmm",
            "mmm-draft-20261018-113015-4242@mmm",
            "mmm-draft-@mmm",
        ] {
            assert_eq!(draft_of(bad), None, "{bad}");
        }
        assert!(Draft::load("../x").is_err());
    }

    #[test]
    fn a_line_break_from_a_mail_adds_no_header() {
        let mut d = draft(Kind::Reply);
        d.to = "Ana <ana@example.org>".into();
        d.subject = "Hi\nBcc: evil@example.org\nAttach: ~/.ssh/id_ed25519".into();
        d.cc = "x@example.org\r\nBcc: evil@example.org".into();
        let (text, _) = d.text();
        let mut back = draft(Kind::Reply);
        back.parse(&text);
        assert_eq!(back.bcc, "", "{text}");
        assert!(back.attach.is_empty(), "{text}");
        assert!(back.subject.starts_with("Hi "), "{text}");
    }

    #[test]
    fn an_address_with_a_space_cant_go() {
        let mut d = draft(Kind::New);
        d.to = "\"a --host=evil.example\"@example.org".into();
        d.subject = "x".into();
        d.body = "x".into();
        let (errors, _) = d.check();
        assert!(!errors.is_empty());
    }

    /// A directory of its own for a test, gone when the test is.
    struct Tmp(PathBuf);

    impl Tmp {
        fn new(name: &str) -> Tmp {
            let dir = std::env::temp_dir().join(format!("mmm-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("bin")).unwrap();
            std::fs::create_dir_all(dir.join("drafts")).unwrap();
            let tmp = Tmp(dir);
            // What the send script runs, standing in: notmuch and notify-send write down what
            // they were asked; `send` is what MMM_SEND names, and keeps the mail; `fail` fails.
            tmp.script("notmuch", r#"echo "$*" >> "$(dirname "$0")/../notmuch.log""#);
            tmp.script("notify-send", r#"echo "$*" >> "$(dirname "$0")/../notify.log""#);
            tmp.script(
                "send",
                r#"d="$(dirname "$0")/.."; cat > "$d/went.eml"; printf '%s\n' "$@" > "$d/went.to""#,
            );
            tmp.script("fail", "exit 1");
            tmp
        }

        fn script(&self, name: &str, body: &str) {
            use std::os::unix::fs::PermissionsExt;
            let path = self.0.join("bin").join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        fn hook(&self, name: &str) -> String {
            self.0.join("bin").join(name).display().to_string()
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.0.join(name)).unwrap_or_default()
        }

        /// A draft to send, its files in this directory.
        fn draft(&self) -> Draft {
            let mut d = draft(Kind::Reply);
            d.file = self.0.join("drafts").join(format!("{}.txt", d.id));
            d.to = "Ana <ana@example.org>, ivo@example.org".into();
            d.subject = "Re: Ručak".into();
            d.body = "Može.".into();
            d.original = Some(Original {
                id: "abc${HOME}%n@example.org".into(),
                from: "Ana <ana@example.org>".into(),
                timestamp: 0,
                lang: None,
            });
            std::fs::write(&d.file, "the editor's file").unwrap();
            std::fs::write(d.file.with_extension("json"), "{}").unwrap();
            d
        }

        /// What the timer does when it fires: the send script, with the arguments it was
        /// started with, and only this directory's stand-ins and the system's tools to run.
        fn fire(&self, timers: &Fake) {
            let (_, args) = timers
                .started
                .borrow()
                .last()
                .cloned()
                .expect("a timer was started");
            let st = Command::new("/bin/sh")
                .args(["-c", SEND, "mmm-send"])
                .args(args)
                .env("PATH", format!("{}:/usr/bin:/bin", self.0.join("bin").display()))
                .status()
                .unwrap();
            assert!(st.success());
            timers.running.set(false);
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The user manager, standing in: it notes the timers started and stopped, and fires none.
    #[derive(Default)]
    struct Fake {
        running: std::cell::Cell<bool>,
        refuses: std::cell::Cell<bool>,
        started: std::cell::RefCell<Vec<(String, Vec<std::ffi::OsString>)>>,
        stopped: std::cell::RefCell<Vec<String>>,
    }

    impl Timers for Fake {
        fn start(&self, unit: &str, _: &str, args: &[std::ffi::OsString]) -> bool {
            if self.refuses.get() {
                return false;
            }
            self.started.borrow_mut().push((unit.to_string(), args.to_vec()));
            self.running.set(true);
            true
        }

        fn running(&self, _: &str) -> bool {
            self.running.get()
        }

        fn stop(&self, unit: &str) {
            self.stopped.borrow_mut().push(unit.to_string());
            self.running.set(false);
        }
    }

    #[test]
    fn a_queued_mail_goes_when_its_timer_fires() {
        let (tmp, timers) = (Tmp::new("goes"), Fake::default());
        let d = tmp.draft();
        let p = queue_in(&tmp.0, &d, &tmp.hook("send"), &timers).unwrap();
        assert_eq!(p.eml, tmp.0.join("outbox").join(format!("{}.eml", d.id)));
        assert!(p.eml.exists());
        assert_eq!(p.to, "Ana and 1 more");
        assert_eq!((p.fate, p.fate_now(&timers)), (Fate::Waiting, Fate::Waiting));
        assert_eq!(timers.started.borrow()[0].0, format!("mmm-send-{}", d.id));

        tmp.fire(&timers);
        assert!(
            tmp.read("went.eml").contains("<ana@example.org>"),
            "{}",
            tmp.read("went.eml")
        );
        assert_eq!(tmp.read("went.to"), "ana@example.org\nivo@example.org\n");
        // Out of the outbox, the draft's files with it, and in the log.
        assert!(!p.eml.exists() && !sending(&p.eml).exists());
        assert!(!d.file.exists() && !d.file.with_extension("json").exists());
        assert!(
            tmp.read("sent.log")
                .trim_end()
                .ends_with(&format!("\tsent\t{}.eml", d.id))
        );
        assert_eq!(p.fate_now(&timers), Fate::Sent);
        // The mail answered is tagged, its id as it is, whatever it holds.
        let asked = tmp.read("notmuch.log");
        assert!(
            asked.contains("tag +replied -- id:abc${HOME}%n@example.org"),
            "{asked}"
        );
        assert!(
            asked.contains(&format!("tag +deleted -- id:{}", draft_message_id(&d.id))),
            "{asked}"
        );
        assert_eq!(tmp.read("notify.log"), "");
    }

    #[test]
    fn a_mail_that_didnt_go_stays_to_take_back() {
        let (tmp, timers) = (Tmp::new("fails"), Fake::default());
        let d = tmp.draft();
        let mut p = queue_in(&tmp.0, &d, &tmp.hook("fail"), &timers).unwrap();
        tmp.fire(&timers);
        // The mail and the draft are still there, and it isn't said to be sent.
        assert!(!p.eml.exists() && sending(&p.eml).exists());
        assert!(d.file.exists() && d.file.with_extension("json").exists());
        assert!(tmp.read("sent.log").contains("\tfailed\t") && !tmp.read("sent.log").contains("\tsent\t"));
        assert!(tmp.read("notify.log").contains("didn't go"));
        p.fate = p.fate_now(&timers);
        assert_eq!(p.fate, Fate::Failed);
        // Found again by an mmm started later, and taken back from there.
        let (found, alone) = scan(&tmp.0, &timers, |_| Ok(d.clone()));
        assert!(alone.is_empty());
        assert_eq!(
            (found.len(), found[0].fate, &found[0].eml),
            (1, Fate::Failed, &p.eml)
        );
        assert!(found[0].take_back_from(&timers));
        assert!(!sending(&p.eml).exists());
        assert_eq!(p.fate_now(&timers), Fate::Gone);
    }

    #[test]
    fn a_mail_being_sent_is_left_alone() {
        let (tmp, timers) = (Tmp::new("busy"), Fake::default());
        let d = tmp.draft();
        let p = queue_in(&tmp.0, &d, &tmp.hook("send"), &timers).unwrap();
        // The script has taken it and runs.
        std::fs::rename(&p.eml, sending(&p.eml)).unwrap();
        assert_eq!(p.fate_now(&timers), Fate::Waiting);
        assert!(!p.take_back_from(&timers));
        assert!(queue_in(&tmp.0, &d, &tmp.hook("send"), &timers).is_err());
        assert!(sending(&p.eml).exists() && !p.eml.exists());
        assert!(timers.stopped.borrow().is_empty());
    }

    #[test]
    fn taken_back_it_doesnt_go() {
        let (tmp, timers) = (Tmp::new("back"), Fake::default());
        let d = tmp.draft();
        let p = queue_in(&tmp.0, &d, &tmp.hook("send"), &timers).unwrap();
        assert!(p.take_back_from(&timers));
        assert_eq!(timers.stopped.borrow().as_slice(), std::slice::from_ref(&p.unit));
        assert_eq!(std::fs::read_dir(tmp.0.join("outbox")).unwrap().count(), 0);
        // A timer that fires all the same finds nothing to send, and the draft is kept.
        tmp.fire(&timers);
        assert_eq!(tmp.read("went.eml"), "");
        assert!(d.file.exists());
        assert_eq!(p.fate_now(&timers), Fate::Gone);
        assert!(!p.take_back_from(&timers));
    }

    #[test]
    fn sent_again_in_its_minute_it_goes_once() {
        let (tmp, timers) = (Tmp::new("twice"), Fake::default());
        let mut d = tmp.draft();
        let first = queue_in(&tmp.0, &d, &tmp.hook("send"), &timers).unwrap();
        d.body = "Ipak ne mogu.".into();
        let second = queue_in(&tmp.0, &d, &tmp.hook("send"), &timers).unwrap();
        // The first timer was stopped before the second was started, on the same file.
        assert_eq!(first.eml, second.eml);
        assert_eq!(
            timers.stopped.borrow().as_slice(),
            std::slice::from_ref(&first.unit)
        );
        assert_eq!(timers.started.borrow().len(), 2);
        assert!(
            std::fs::read_to_string(&second.eml)
                .unwrap()
                .contains("Ipak ne mogu.")
        );
        // A timer that can't be started leaves nothing in the outbox, and nothing goes.
        timers.refuses.set(true);
        assert!(queue_in(&tmp.0, &d, &tmp.hook("send"), &timers).is_err());
        assert!(!second.eml.exists());
        assert_eq!(timers.stopped.borrow().len(), 2);
        tmp.fire(&timers);
        assert_eq!(tmp.read("went.eml"), "");
    }

    #[test]
    fn the_outbox_is_read_again_at_the_start() {
        let (tmp, timers) = (Tmp::new("start"), Fake::default());
        let d = tmp.draft();
        let mut p = queue_in(&tmp.0, &d, &tmp.hook("send"), &timers).unwrap();
        let outbox = tmp.0.join("outbox");
        std::fs::write(outbox.join("notes.txt"), "").unwrap();
        std::fs::write(outbox.join("x.eml"), "").unwrap();
        std::fs::write(outbox.join("20261018-113015-4242-0.eml"), "").unwrap();
        let load = |id: &str| {
            if id == d.id {
                Ok(d.clone())
            } else {
                bail!("no such draft")
            }
        };
        // Its timer still waits: in its minute, as when it was queued.
        let (found, alone) = scan(&tmp.0, &timers, load);
        assert_eq!(alone, [outbox.join("20261018-113015-4242-0.eml")]);
        assert_eq!(
            (found.len(), found[0].fate, &found[0].to),
            (1, Fate::Waiting, &p.to)
        );
        assert!(found[0].left() > UNDO_SECS - 10);
        // The timer gone (a logout, a reboot), the mail still there: not sent, and it says so,
        // also once its minute is over.
        timers.running.set(false);
        let (mut found, _) = scan(&tmp.0, &timers, load);
        assert_eq!(found[0].fate, Fate::Lost);
        assert_eq!(found[0].fate_now(&timers), Fate::Lost);
        p.at -= std::time::Duration::from_secs(2 * UNDO_SECS);
        assert_eq!((p.left(), p.fate_now(&timers)), (0, Fate::Lost));
        // Taken back, it is out of the outbox, to send again from the review screen.
        assert!(found.remove(0).take_back_from(&timers));
        assert!(!p.eml.exists());
    }

    #[test]
    fn a_file_is_written_whole() {
        let tmp = Tmp::new("whole");
        let file = tmp.0.join("drafts/a.json");
        write_whole(&file, b"one").unwrap();
        write_whole(&file, b"two").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(tmp.0.join("drafts")).unwrap().count(), 1);
        assert!(write_whole(&tmp.0.join("none/a.json"), b"x").is_err());
    }
}
