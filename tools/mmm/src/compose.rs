//! Writing mail: a draft in the editor, a look at it before it goes, and a minute to take it back.
//!
//! The draft is a text file: headers (From, To, Cc, Bcc, Subject, Attach), a blank line, what you
//! write, and below it the mail you reply to or forward. The signature isn't in it: it's chosen
//! by the language of what you wrote, shown on the review screen, and goes between your text and
//! the quote. Sending is `deck-mail send` (msmtpq), a minute later, from a systemd user timer:
//! until then the mail can be taken back, and quitting mmm doesn't stop it.

use crate::notmuch::{self, Message};
use crate::render;
use anyhow::{Context, Result, bail};
use chrono::{Local, TimeZone};
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use whatlang::{Detector, Lang};

/// The languages there are signatures for, in the order `l` steps through them.
pub const LANGS: &[(&str, &str)] = &[("hr", "Croatian"), ("en", "English"), ("de", "German")];

/// How long a mail waits after `y`, for `u` to take it back.
pub const UNDO_SECS: u64 = 60;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    New,
    Reply,
    Forward,
}

#[derive(Clone, Debug)]
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
    pub lang: Option<&'static str>,
    /// Why that language: shown on the review screen.
    pub lang_why: String,
}

#[derive(Clone, Debug)]
pub struct Original {
    pub id: String,
    pub from: String,
    pub timestamp: i64,
    /// The language its text is in, as a guess for this one's.
    pub lang: Option<&'static str>,
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
        let mut t = format!(
            "From: {}\nTo: {}\nCc: {}\nBcc: {}\nSubject: {}\n",
            self.from, self.to, self.cc, self.bcc, self.subject
        );
        for a in &self.attach {
            t += &format!("Attach: {}\n", tilde(a));
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
                if let Some(bad) = r.iter().find(|a| !a.contains('@') || a.starts_with('-')) {
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
            .header("User-Agent", mail_builder::headers::text::Text::new("mmm"))
            .text_body(self.final_text());
        if !self.to.trim().is_empty() {
            b = b.to(list(&self.to)?);
        }
        if !self.cc.trim().is_empty() {
            b = b.cc(list(&self.cc)?);
        }
        // Bcc goes as a recipient only, never as a header.
        let ids = |v: &str| -> Vec<String> {
            v.split_whitespace()
                .map(|i| i.trim_matches(['<', '>']).to_string())
                .filter(|i| !i.is_empty())
                .collect()
        };
        if let Some(r) = &self.in_reply_to {
            b = b.in_reply_to(ids(r));
        }
        if let Some(r) = &self.references {
            b = b.references(ids(r));
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

/// A mail on its way: it goes when `unit`'s timer fires, unless taken back before.
pub struct Pending {
    pub draft: Draft,
    pub unit: String,
    pub eml: PathBuf,
    pub at: std::time::Instant,
    pub to: String,
}

/// The script the timer runs: takes the mail out of the outbox (so a take-back that got there
/// first wins), hands it to `deck-mail send`, tags the mail it answers, and clears away the
/// draft and the forwarded mail's attachments.
/// (No `${…}` in it: systemd expands those in a unit's command line.)
const SEND: &str = r#"set -u
f="$1"; draft="$2"; query="$3"; tag="$4"; send="$5"; log="$6"; files="$7"; shift 7
mv -- "$f" "$f.sending" 2>/dev/null || exit 0
if $send "$@" < "$f.sending"; then
    rm -rf -- "$f.sending" "$draft" "$files"
    if [ -n "$query" ]; then notmuch tag "+$tag" -- "$query"; fi
    printf '%s\tsent\t%s\n' "$(date -Is)" "$(basename "$f")" >> "$log"
else
    notify-send -u critical "mmm: a mail didn't go" "It is in $f.sending"
fi
"#;

/// `y`: the mail goes in a minute, from a systemd user timer.
pub fn queue(d: &Draft) -> Result<Pending> {
    let (errors, _) = d.check();
    if let Some(e) = errors.first() {
        bail!("{e}");
    }
    let raw = d.build()?;
    let recipients = d.recipients()?;
    let outbox = state_dir().join("outbox");
    std::fs::create_dir_all(&outbox)?;
    let eml = outbox.join(format!("{}.eml", d.id));
    std::fs::write(&eml, raw)?;
    // The draft file stays until it's sent, so a mail taken back or lost is still there.
    d.write()?;
    let unit = format!("mmm-send-{}", d.id);
    let (query, tag) = match (&d.original, d.kind) {
        (Some(o), Kind::Forward) => (format!("id:{}", notmuch::quote(&o.id)), "passed"),
        (Some(o), _) => (format!("id:{}", notmuch::quote(&o.id)), "replied"),
        (None, _) => (String::new(), "replied"),
    };
    // The unit runs in the user manager, not here: it gets mmm's PATH, and its notmuch database
    // when that's not the default one. MMM_SEND replaces `deck-mail send`, for tests.
    let send = std::env::var("MMM_SEND").unwrap_or_else(|_| "deck-mail send".into());
    let mut env = vec!["--setenv=PATH".to_string()];
    if std::env::var_os("NOTMUCH_CONFIG").is_some() {
        env.push("--setenv=NOTMUCH_CONFIG".into());
    }
    let st = Command::new("systemd-run")
        .args([
            "--user",
            "--quiet",
            "--collect",
            &format!("--unit={unit}"),
            &format!("--on-active={UNDO_SECS}"),
            "--timer-property=AccuracySec=1s",
            &format!("--description=mmm: a mail to {}", recipients.join(", ")),
        ])
        .args(&env)
        .args(["--", "/bin/sh", "-c", SEND, "mmm-send"])
        .arg(&eml)
        .arg(&d.file)
        .args([&query, tag, &send])
        .arg(state_dir().join("sent.log"))
        .arg(d.file.with_extension(""))
        .args(&recipients)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .context("running systemd-run")?;
    if !st.success() {
        let _ = std::fs::remove_file(&eml);
        bail!("systemd-run couldn't start the send timer");
    }
    if let Some(l) = d.lang {
        remember_lang(&recipients, l);
    }
    let to = addresses(&d.to)
        .ok()
        .and_then(|a| a.into_iter().next())
        .map(|(n, a)| n.unwrap_or(a))
        .unwrap_or_default();
    let more = recipients.len().saturating_sub(1);
    Ok(Pending {
        draft: d.clone(),
        unit,
        eml,
        at: std::time::Instant::now(),
        to: if more > 0 {
            format!("{to} and {more} more")
        } else {
            to
        },
    })
}

impl Pending {
    pub fn left(&self) -> u64 {
        UNDO_SECS.saturating_sub(self.at.elapsed().as_secs())
    }

    /// `u`: takes the mail back, if it hasn't gone. The outbox file is the lock: whoever moves it
    /// first, this or the timer's script, has it.
    pub fn take_back(&self) -> Result<bool> {
        let back = self.eml.with_extension("eml.taken-back");
        if std::fs::rename(&self.eml, &back).is_err() {
            return Ok(false);
        }
        let _ = Command::new("systemctl")
            .args(["--user", "stop", &format!("{}.timer", self.unit)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_file(back);
        Ok(true)
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
}
