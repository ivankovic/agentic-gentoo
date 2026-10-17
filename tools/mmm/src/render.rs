//! A thread as lines on screen. Links (in HTML and bare URLs in plain text) and attachments are
//! focus points the cursor jumps between; everything else is text with a kind for its style.

use crate::archive::{Pdf, State};
use crate::notmuch::Message;
use chrono::{Datelike, Local, TimeZone};
use html2text::render::RichAnnotation;
use serde_json::Value;
use std::collections::HashMap;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Quote,
    /// A signature, after a `-- ` line.
    Sig,
    From,
    Meta,
    Strong,
    Emph,
    Code,
    Link,
    Attachment,
    Rule,
    /// A PDF that waits for a choice: to Nextcloud, or left out.
    Waiting,
    /// A PDF chosen for, or already in, Nextcloud.
    Filed,
}

/// What became of each PDF, by (message id, part).
pub type Pdfs = HashMap<(String, u64), Pdf>;

#[derive(Clone, Debug)]
pub struct Seg {
    pub text: String,
    pub kind: Kind,
    /// The focus point this text belongs to, an index into `Doc::focus`.
    pub focus: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Link(String),
    Attachment {
        msg: String,
        part: u64,
        name: String,
        ctype: String,
    },
}

#[derive(Clone, Debug)]
pub struct Focus {
    pub target: Target,
    /// The first line it is on.
    pub line: usize,
    /// What the reader sees of it: a link's text, an attachment's name.
    pub text: String,
}

#[derive(Default)]
pub struct Doc {
    pub lines: Vec<Vec<Seg>>,
    pub focus: Vec<Focus>,
    /// The line each message starts on, and its index in the thread.
    pub starts: Vec<usize>,
}

/// `10:42` today, `Mon 10:42` this week, `12 Oct` this year, `2024-03-11` before.
pub fn when(ts: i64) -> String {
    let Some(t) = Local.timestamp_opt(ts, 0).single() else {
        return String::new();
    };
    let now = Local::now();
    let days = (now.date_naive() - t.date_naive()).num_days();
    if days == 0 {
        t.format("%H:%M").to_string()
    } else if (0..7).contains(&days) {
        t.format("%a %H:%M").to_string()
    } else if t.year() == now.year() {
        t.format("%-d %b").to_string()
    } else {
        t.format("%Y-%m-%d").to_string()
    }
}

/// `Ana Horvat` from `Ana Horvat <ana@example.org>`, the address when there is no name.
pub fn name(addr: &str) -> String {
    let addr = addr.trim();
    match addr.split_once('<') {
        Some((n, _)) if !n.trim().is_empty() => n.trim().trim_matches('"').to_string(),
        Some((_, a)) => a.trim_end_matches('>').to_string(),
        None => addr.to_string(),
    }
}

/// Where the URLs are in a line of plain text, as byte ranges.
pub fn find_urls(s: &str) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let lower = s.to_ascii_lowercase();
    let mut i = 0;
    while i < s.len() {
        let rest = &lower[i..];
        let starts = ["https://", "http://", "mailto:", "www."]
            .iter()
            .any(|p| rest.starts_with(p));
        let boundary = i == 0
            || !s[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '/');
        if starts && boundary {
            let end = s[i..]
                .char_indices()
                .find(|&(_, c)| c.is_whitespace() || "<>\"'`|^{}[]".contains(c))
                .map_or(s.len(), |(j, _)| i + j);
            let mut url = &s[i..end];
            // Sentence punctuation after a URL isn't part of it; a ")" is when it closes a "(".
            loop {
                let trimmed = url.trim_end_matches(['.', ',', ';', ':', '!', '?', '*']);
                let trimmed = if trimmed.ends_with(')')
                    && trimmed.matches('(').count() < trimmed.matches(')').count()
                {
                    &trimmed[..trimmed.len() - 1]
                } else {
                    trimmed
                };
                if trimmed.len() == url.len() {
                    break;
                }
                url = trimmed;
            }
            let min = if lower[i..].starts_with("www.") { 5 } else { 9 };
            if url.len() >= min {
                out.push((i, i + url.len()));
            }
            i = end.max(i + 1);
        } else {
            i += s[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

/// What a message's body is made of, in order.
enum Piece {
    Plain(String),
    Html(String),
    Attachment {
        part: u64,
        name: String,
        ctype: String,
        size: u64,
    },
    Forwarded {
        headers: Value,
        body: Vec<Value>,
    },
}

fn pieces(p: &Value, html: bool, out: &mut Vec<Piece>) {
    let ctype = p["content-type"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let part = p["id"].as_u64().unwrap_or_default();
    let filename = p["filename"].as_str().filter(|f| !f.is_empty());
    let attachment = |name: String, out: &mut Vec<Piece>| {
        out.push(Piece::Attachment {
            part,
            name,
            ctype: ctype.clone(),
            size: p["content-length"].as_u64().unwrap_or_default(),
        })
    };
    if let Some(f) = filename {
        return attachment(f.to_string(), out);
    }
    let children = || p["content"].as_array().into_iter().flatten();
    match ctype.as_str() {
        "multipart/alternative" => {
            let kids: Vec<&Value> = children().collect();
            let has = |want: &str, v: &Value| {
                let t = v["content-type"].as_str().unwrap_or_default();
                t == want || (t.starts_with("multipart/") && v.to_string().contains(&format!("\"{want}\"")))
            };
            let pick = if html { "text/html" } else { "text/plain" };
            let other = if html { "text/plain" } else { "text/html" };
            let chosen = kids
                .iter()
                .rev()
                .find(|k| has(pick, k))
                .or_else(|| kids.iter().rev().find(|k| has(other, k)))
                .or(kids.first());
            if let Some(k) = chosen {
                pieces(k, html, out);
            }
        }
        t if t.starts_with("multipart/") => {
            for k in children() {
                pieces(k, html, out);
            }
        }
        "text/plain" => out.push(Piece::Plain(
            p["content"].as_str().unwrap_or_default().to_string(),
        )),
        "text/html" => out.push(Piece::Html(p["content"].as_str().unwrap_or_default().to_string())),
        "message/rfc822" => {
            for m in children() {
                out.push(Piece::Forwarded {
                    headers: m["headers"].clone(),
                    body: m["body"].as_array().cloned().unwrap_or_default(),
                });
            }
        }
        _ => attachment(format!("({ctype})"), out),
    }
}

/// A body as plain text, to quote or forward: text parts as they are, HTML as text.
pub fn plain_text(body: &[Value]) -> String {
    let mut ps = vec![];
    for p in body {
        pieces(p, false, &mut ps);
    }
    let mut out = String::new();
    for p in ps {
        let text = match p {
            Piece::Plain(t) => t,
            Piece::Html(h) => html2text::from_read(h.as_bytes(), 76).unwrap_or(h),
            Piece::Attachment { .. } => continue,
            Piece::Forwarded { headers, body } => {
                let h = |k: &str| headers[k].as_str().unwrap_or_default().to_string();
                format!(
                    "---------- Forwarded message ----------\nFrom: {}\nDate: {}\nSubject: {}\n\n{}",
                    h("From"),
                    h("Date"),
                    h("Subject"),
                    plain_text(&body)
                )
            }
        };
        if !out.is_empty() {
            out += "\n";
        }
        out += text.trim_end();
        out += "\n";
    }
    out
}

/// A body's attachments, as (part, name).
pub fn attachments(body: &[Value]) -> Vec<(u64, String)> {
    let mut ps = vec![];
    for p in body {
        pieces(p, true, &mut ps);
    }
    ps.into_iter()
        .filter_map(|p| match p {
            Piece::Attachment { part, name, .. } => Some((part, name)),
            _ => None,
        })
        .collect()
}

fn size(bytes: u64) -> String {
    match bytes {
        0 => String::new(),
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{} KB", b / 1024),
        b => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
    }
}

struct Builder<'a> {
    doc: Doc,
    width: usize,
    pdfs: &'a Pdfs,
}

/// The line under a PDF: what became of it, or that it waits and what `t` would make of it.
fn filing(p: &Pdf) -> (String, Kind) {
    let as_ = |c: &str, n: &str| format!("{c} / {n}");
    match (&p.state, &p.category, &p.name) {
        (State::Sent, ..) => (
            format!("   ☁ in Nextcloud: {}", p.r#where.as_deref().unwrap_or("")),
            Kind::Meta,
        ),
        (State::LeftOut, ..) => ("   ☁ left out of Nextcloud".into(), Kind::Meta),
        (State::Chosen, Some(c), Some(n)) => (
            format!(
                "   ☁ to Nextcloud at the next sync, as {} · t changes it",
                as_(c, n)
            ),
            Kind::Filed,
        ),
        _ => match &p.suggested {
            Some((c, _)) if c == "-" => (
                "   ☁ waiting · the rules say leave it out: x does · t files it in Nextcloud".into(),
                Kind::Waiting,
            ),
            Some((c, n)) => (
                format!("   ☁ waiting · t files it as {} · x leaves it out", as_(c, n)),
                Kind::Waiting,
            ),
            None => (
                "   ☁ waiting · t files it in Nextcloud · x leaves it out".into(),
                Kind::Waiting,
            ),
        },
    }
}

impl Builder<'_> {
    fn focus(&mut self, target: Target, text: String) -> usize {
        self.doc.focus.push(Focus {
            target,
            line: self.doc.lines.len(),
            text,
        });
        self.doc.focus.len() - 1
    }

    fn blank(&mut self) {
        self.doc.lines.push(vec![]);
    }

    /// Lays out one logical line, wrapping at spaces where it can.
    fn line(&mut self, segs: Vec<Seg>) {
        // Words, with the spaces after them, each carrying its segment's style.
        let mut words: Vec<Seg> = vec![];
        for s in segs {
            let mut cur = String::new();
            for c in s.text.chars() {
                cur.push(c);
                if c == ' ' {
                    words.push(Seg {
                        text: std::mem::take(&mut cur),
                        ..s.clone()
                    });
                }
            }
            if !cur.is_empty() {
                words.push(Seg { text: cur, ..s });
            }
        }
        let mut line: Vec<Seg> = vec![];
        let mut used = 0;
        for mut w in words {
            loop {
                let ww = w.text.trim_end().width();
                if used + ww <= self.width || (used == 0 && ww <= self.width) {
                    used += w.text.width();
                    push(&mut line, w);
                    break;
                }
                if used > 0 {
                    self.flush(&mut line);
                    used = 0;
                    if w.text.trim().is_empty() {
                        break;
                    }
                    continue;
                }
                // A word wider than the screen: cut it.
                let mut head = String::new();
                let mut hw = 0;
                let mut rest = String::new();
                for c in w.text.chars() {
                    let cw = c.width().unwrap_or(0);
                    if rest.is_empty() && hw + cw <= self.width {
                        head.push(c);
                        hw += cw;
                    } else {
                        rest.push(c);
                    }
                }
                push(
                    &mut line,
                    Seg {
                        text: head,
                        ..w.clone()
                    },
                );
                self.flush(&mut line);
                w.text = rest;
                if w.text.is_empty() {
                    break;
                }
            }
        }
        self.flush(&mut line);
    }

    fn flush(&mut self, line: &mut Vec<Seg>) {
        let l = std::mem::take(line);
        // A focus point's line is where its first piece lands.
        for s in &l {
            if let Some(f) = s.focus
                && self.doc.focus[f].line > self.doc.lines.len()
            {
                self.doc.focus[f].line = self.doc.lines.len();
            }
        }
        self.doc.lines.push(l);
    }

    fn plain(&mut self, text: &str) {
        let mut sig = false;
        for raw in text.replace("\r\n", "\n").lines() {
            let raw = raw.replace('\t', "    ");
            if raw == "-- " || raw == "--" {
                sig = true;
            }
            let kind = if sig {
                Kind::Sig
            } else if raw.trim_start().starts_with('>') {
                Kind::Quote
            } else {
                Kind::Text
            };
            let mut segs = vec![];
            let mut at = 0;
            for (s, e) in find_urls(&raw) {
                if s > at {
                    segs.push(Seg {
                        text: raw[at..s].to_string(),
                        kind,
                        focus: None,
                    });
                }
                let url = &raw[s..e];
                let target = if url.to_ascii_lowercase().starts_with("www.") {
                    format!("https://{url}")
                } else {
                    url.to_string()
                };
                let f = self.focus(Target::Link(target), url.to_string());
                self.doc.focus[f].line = usize::MAX;
                segs.push(Seg {
                    text: url.to_string(),
                    kind: Kind::Link,
                    focus: Some(f),
                });
                at = e;
            }
            if at < raw.len() || segs.is_empty() {
                segs.push(Seg {
                    text: raw[at..].to_string(),
                    kind,
                    focus: None,
                });
            }
            self.line(segs);
        }
    }

    fn html(&mut self, html: &str) {
        let lines = match html2text::config::rich()
            .no_table_borders()
            .lines_from_read(html.as_bytes(), self.width.max(20))
        {
            Ok(l) => l,
            Err(_) => {
                return self.plain("(this HTML could not be read; h shows the plain text, if there is one)");
            }
        };
        // A link split over several strings or lines stays one focus point while its URL runs on.
        let mut open: Option<(String, usize)> = None;
        let mut blank_run = 0;
        for l in lines {
            let mut segs = vec![];
            for ts in l.tagged_strings() {
                let mut kind = Kind::Text;
                let mut link = None;
                for a in &ts.tag {
                    match a {
                        RichAnnotation::Link(url) => link = Some(url.clone()),
                        RichAnnotation::Strong => kind = Kind::Strong,
                        RichAnnotation::Emphasis => kind = Kind::Emph,
                        RichAnnotation::Code | RichAnnotation::Preformat(_) => kind = Kind::Code,
                        RichAnnotation::Image(_) => kind = Kind::Meta,
                        _ => {}
                    }
                }
                let focus = match link {
                    Some(url) if !url.is_empty() && !url.starts_with('#') => {
                        let f = match &open {
                            Some((u, f)) if *u == url => *f,
                            _ => {
                                let f = self.focus(Target::Link(url.clone()), String::new());
                                open = Some((url, f));
                                f
                            }
                        };
                        self.doc.focus[f].text.push_str(&ts.s);
                        kind = Kind::Link;
                        Some(f)
                    }
                    _ => {
                        if !ts.s.trim().is_empty() {
                            open = None;
                        }
                        None
                    }
                };
                segs.push(Seg {
                    text: ts.s.clone(),
                    kind,
                    focus,
                });
            }
            // Newsletters stack empty lines; two in a row are enough.
            let empty = segs.iter().all(|s| s.text.trim().is_empty());
            blank_run = if empty { blank_run + 1 } else { 0 };
            if blank_run <= 1 {
                self.flush(&mut segs);
            }
        }
    }

    fn body(&mut self, msg: &str, body: &[Value], html: bool) {
        let mut ps = vec![];
        for p in body {
            pieces(p, html, &mut ps);
        }
        for p in ps {
            match p {
                Piece::Plain(t) => self.plain(&t),
                Piece::Html(t) => self.html(&t),
                Piece::Attachment {
                    part,
                    name,
                    ctype,
                    size: bytes,
                } => {
                    let f = self.focus(
                        Target::Attachment {
                            msg: msg.to_string(),
                            part,
                            name: name.clone(),
                            ctype: ctype.clone(),
                        },
                        name.clone(),
                    );
                    self.doc.lines.push(vec![
                        Seg {
                            text: "📎 ".into(),
                            kind: Kind::Meta,
                            focus: None,
                        },
                        Seg {
                            text: name,
                            kind: Kind::Attachment,
                            focus: Some(f),
                        },
                        Seg {
                            text: format!("  {ctype}  {}", size(bytes)),
                            kind: Kind::Meta,
                            focus: None,
                        },
                    ]);
                    if let Some(p) = self.pdfs.get(&(msg.to_string(), part)) {
                        let (text, kind) = filing(p);
                        self.line(vec![Seg {
                            text,
                            kind,
                            focus: None,
                        }]);
                    }
                }
                Piece::Forwarded { headers, body } => {
                    self.blank();
                    let h = |k: &str| headers[k].as_str().unwrap_or_default().to_string();
                    self.line(vec![Seg {
                        text: format!("── Forwarded: {} · {} · {}", h("From"), h("Subject"), h("Date")),
                        kind: Kind::Meta,
                        focus: None,
                    }]);
                    self.body(msg, &body, html);
                }
            }
        }
    }
}

fn push(line: &mut Vec<Seg>, s: Seg) {
    if let Some(last) = line.last_mut()
        && last.kind == s.kind
        && last.focus == s.focus
    {
        last.text.push_str(&s.text);
        return;
    }
    line.push(s);
}

/// The thread at `width` columns. `html` prefers the HTML of a message that has both; `pdfs`
/// says what became of its PDFs.
pub fn thread(msgs: &[Message], width: usize, html: bool, pdfs: &Pdfs) -> Doc {
    let mut b = Builder {
        doc: Doc::default(),
        width: width.max(20),
        pdfs,
    };
    for (i, m) in msgs.iter().enumerate() {
        if i > 0 {
            b.doc.lines.push(vec![Seg {
                text: "─".repeat(b.width),
                kind: Kind::Rule,
                focus: None,
            }]);
        }
        b.doc.starts.push(b.doc.lines.len());
        b.doc.lines.push(vec![
            Seg {
                text: name(&m.from),
                kind: Kind::From,
                focus: None,
            },
            Seg {
                text: format!("  {}", when(m.timestamp)),
                kind: Kind::Meta,
                focus: None,
            },
        ]);
        let mut meta = format!("to {}", m.to);
        if !m.cc.is_empty() {
            meta += &format!("  cc {}", m.cc);
        }
        b.line(vec![Seg {
            text: meta,
            kind: Kind::Meta,
            focus: None,
        }]);
        b.blank();
        b.body(&m.id, &m.body, html);
    }
    b.doc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(s: &str) -> Vec<&str> {
        find_urls(s).into_iter().map(|(a, b)| &s[a..b]).collect()
    }

    #[test]
    fn finds_urls_without_their_punctuation() {
        assert_eq!(urls("see https://example.org/a."), ["https://example.org/a"]);
        assert_eq!(
            urls("(https://en.wikipedia.org/wiki/Rust_(language))"),
            ["https://en.wikipedia.org/wiki/Rust_(language)"]
        );
        assert_eq!(
            urls("at www.hep.hr, or <mailto:a@b.hr>"),
            ["www.hep.hr", "mailto:a@b.hr"]
        );
        assert!(urls("a xhttps://x and http:// alone").is_empty());
    }

    #[test]
    fn a_wrapped_link_is_one_focus() {
        let mut b = Builder {
            doc: Doc::default(),
            width: 30,
            pdfs: &Pdfs::new(),
        };
        b.plain("Read it at https://example.org/a/very/long/path/that/wraps/around and reply");
        assert_eq!(b.doc.focus.len(), 1);
        let on: usize = b
            .doc
            .lines
            .iter()
            .filter(|l| l.iter().any(|s| s.focus == Some(0)))
            .count();
        assert!(on >= 2, "{:?}", b.doc.lines);
        assert_eq!(
            b.doc.focus[0].line, 1,
            "the URL does not fit after \"Read it at \""
        );
        assert!(
            b.doc
                .lines
                .iter()
                .all(|l| l.iter().map(|s| s.text.trim_end().width()).sum::<usize>() <= 30)
        );
    }

    #[test]
    fn html_links_keep_their_target() {
        let mut b = Builder {
            doc: Doc::default(),
            width: 60,
            pdfs: &Pdfs::new(),
        };
        b.html("<p>Bok, mjesto je <a href=\"https://maps.example/x\">ovdje na karti</a>.</p>");
        assert_eq!(b.doc.focus.len(), 1);
        assert_eq!(
            b.doc.focus[0].target,
            Target::Link("https://maps.example/x".into())
        );
        assert_eq!(b.doc.focus[0].text, "ovdje na karti");
    }

    #[test]
    fn a_pdf_says_what_became_of_it() {
        let msg = Message {
            id: "m@x".into(),
            timestamp: 0,
            tags: vec![],
            from: "HEP <racuni@hep.hr>".into(),
            to: "me".into(),
            cc: String::new(),
            subject: "Račun".into(),
            body: serde_json::from_str(
                r#"[{"id": 1, "content-type": "multipart/mixed", "content": [
                    {"id": 2, "content-type": "text/plain", "content": "U privitku."},
                    {"id": 3, "content-type": "application/pdf", "filename": "racun.pdf"}]}]"#,
            )
            .unwrap(),
        };
        let mut pdfs = Pdfs::new();
        pdfs.insert(
            ("m@x".into(), 3),
            Pdf {
                message: "m@x".into(),
                sha256: "ab".into(),
                state: State::Waiting,
                category: None,
                name: None,
                r#where: None,
                suggested: Some(("Računi".into(), "HEP - račun".into())),
            },
        );
        let doc = thread(&[msg], 80, true, &pdfs);
        let text: Vec<String> = doc
            .lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect())
            .collect();
        let at = text.iter().position(|l| l.contains("racun.pdf")).unwrap();
        assert!(
            text[at + 1].contains("waiting · t files it as Računi / HEP - račun"),
            "{text:?}"
        );
        assert_eq!(doc.lines[at + 1][0].kind, Kind::Waiting);
    }

    #[test]
    fn names() {
        assert_eq!(name("\"Ana Horvat\" <ana@example.org>"), "Ana Horvat");
        assert_eq!(name("<ana@example.org>"), "ana@example.org");
        assert_eq!(name("ana@example.org"), "ana@example.org");
    }
}
