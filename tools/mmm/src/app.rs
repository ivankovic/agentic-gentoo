//! What mmm holds while it runs, and what each key does to it.

use crate::archive::{self, Choice, State};
use crate::compose::{self, Draft, Fate, Pending};
use crate::config::{self, Search};
use crate::notmuch::{self, Message, Thread};
use crate::render::{self, Doc, Pdfs, Target};
use crate::screen;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const PAGE: usize = 200;

pub struct ThreadView {
    pub thread: Thread,
    pub msgs: Vec<Message>,
    pub doc: Doc,
    pub width: usize,
    pub scroll: usize,
    pub focus: Option<usize>,
    pub html: bool,
    /// Not drawn yet: the first draw fills the screen up to where reading starts.
    pub fresh: bool,
    /// What became of the thread's PDFs, by (message, part), and the archive's categories.
    pub pdfs: Pdfs,
    pub categories: Vec<String>,
    /// Each PDF's sha256, by (message, part): what deck-mail knows it by.
    shas: HashMap<(String, u64), String>,
}

/// `t` on a PDF: where in Nextcloud it goes, before it's chosen.
pub struct FileForm {
    pub sha: String,
    pub file: String,
    /// The choice it had, for `u`.
    before: Choice,
    pub categories: Vec<String>,
    pub category: Option<usize>,
    pub name: String,
    /// In `name`, in characters.
    pub cursor: usize,
    /// The row being edited: 0 the category, 1 the name.
    pub row: usize,
}

pub enum Mode {
    Normal,
    /// Typing a notmuch query after `/`.
    Search(String),
    /// `?`: what the selected thing is, and the keys that work on it.
    Explain {
        title: String,
        body: Vec<Line<'static>>,
        keys: Vec<(&'static str, &'static str)>,
    },
    /// `?` twice: every key.
    Help,
    /// `t`: filing a PDF in Nextcloud.
    File(FileForm),
    /// A draft back from the editor: a look before it goes. `discard` asks y/n to throw it away.
    Review {
        draft: Box<Draft>,
        scroll: usize,
        discard: bool,
    },
}

/// What `u` puts back: a tag change, on exactly the messages it changed, or a PDF's choice.
enum Undo {
    Tags {
        ids: Vec<String>,
        change: String,
    },
    File {
        sha: String,
        file: String,
        before: Choice,
    },
    /// A sender let in or blocked: their line in deck-mail's senders, and what it tagged.
    Sender {
        address: String,
        changes: Vec<(String, Vec<String>)>,
    },
}

pub struct App {
    pub searches: Vec<Search>,
    /// Which search the list shows; `None` for a query typed after `/`.
    pub current: Option<usize>,
    pub query: String,
    /// Each search's mail that needs you: unread, or with a PDF waiting for a choice.
    pub unread: Vec<Option<u64>>,
    pub threads: Vec<Thread>,
    pub more: bool,
    pub sel: usize,
    pub view: Option<ThreadView>,
    pub mode: Mode,
    pub status: String,
    /// Lines the thread view showed last time it was drawn.
    pub page: usize,
    pub html: bool,
    pub quit: bool,
    undo: Vec<Undo>,
    lastmod: String,
    /// The database's version file when lastmod was last asked: unchanged, nothing to ask.
    stamp: Option<(u64, std::time::SystemTime)>,
    /// A draft for the editor: main suspends the screen and runs it.
    pub edit: Option<Draft>,
    /// Mail sent with `y` that is still in the outbox: in its minute before it goes, or left
    /// there because its timer was lost or the send failed.
    pub pending: Vec<Pending>,
    /// After a first `U`: the message and how it unsubscribes, until the second `U` does it.
    unsub: Option<(String, screen::Unsubscribe)>,
}

fn invert(change: &str) -> String {
    match change.split_at(1) {
        ("+", t) => format!("-{t}"),
        (_, t) => format!("+{t}"),
    }
}

const NOT_OPENED: &str = "not opened: only http, https and mailto links open from mail";

/// Whether a link from a mail is one for the browser or the mail program. `xdg-open` takes
/// anything, a file's path or another program's scheme too, and those a mail doesn't get to open.
fn opens(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, rest)| {
        !rest.is_empty()
            && ["http", "https", "mailto"]
                .iter()
                .any(|s| scheme.eq_ignore_ascii_case(s))
    })
}

/// The category after this one in a list of `n`, or before it; none when there are none.
fn step_category(now: Option<usize>, n: usize, back: bool) -> Option<usize> {
    if n == 0 {
        return None;
    }
    Some(match (now, back) {
        (None, true) => n - 1,
        (None, false) => 0,
        (Some(c), true) => (c.min(n - 1) + n - 1) % n,
        (Some(c), false) => (c + 1) % n,
    })
}

/// Starts a program apart from mmm, so it outlives it and owns no part of the terminal.
fn spawn(cmd: &str, args: &[&str]) -> Result<()> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

fn copy(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .map(|mut s| s.write_all(text.as_bytes()))
        .transpose()?;
    child.wait()?;
    Ok(())
}

/// The host of a URL, lower-cased, without `www.`.
pub fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let h = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let h = h.rsplit('@').next().unwrap_or(h).split(':').next().unwrap_or(h);
    h.trim_start_matches("www.").to_ascii_lowercase()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (
            b[i],
            b.get(i + 1).copied().and_then(hex),
            b.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(h), Some(l)) => {
                out.push(h * 16 + l);
                i += 3;
            }
            (b'+', ..) => {
                out.push(b' ');
                i += 1;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where a tracking or safe-links redirect leads: a URL carried in its query string
/// (`?destination=https%3A…`, `?url=…`, `?u=…`), when there is one.
pub fn destination(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next()?;
    query.split('&').find_map(|kv| {
        let (_, v) = kv.split_once('=')?;
        let v = percent_decode(v);
        (v.starts_with("https://") || v.starts_with("http://")).then_some(v)
    })
}

/// A link whose text names one site and whose target is another: worth a second look.
pub fn mismatch(text: &str, url: &str) -> Option<String> {
    let t = text.trim();
    if t.contains(' ') || !t.contains('.') || url.starts_with("mailto:") {
        return None;
    }
    let shown = host(t);
    let real = host(&destination(url).unwrap_or_else(|| url.to_string()));
    let looks_like_host =
        shown.split('.').count() >= 2 && shown.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-');
    (looks_like_host && !real.is_empty() && real != shown && !real.ends_with(&format!(".{shown}")))
        .then(|| format!("the link says {shown} but goes to {real}"))
}

impl App {
    pub fn new(query: Option<String>) -> Result<App> {
        let searches = config::searches();
        let (current, query) = match query {
            Some(q) => (None, q),
            None => (Some(0), searches[0].query.clone()),
        };
        let mut app = App {
            unread: vec![None; searches.len()],
            searches,
            current,
            query,
            threads: vec![],
            more: false,
            sel: 0,
            view: None,
            mode: Mode::Normal,
            status: String::new(),
            page: 20,
            html: true,
            quit: false,
            undo: vec![],
            lastmod: String::new(),
            stamp: None,
            edit: None,
            pending: vec![],
            unsub: None,
        };
        app.lastmod = notmuch::lastmod().unwrap_or_default();
        // What an earlier mmm left in the outbox: still to go, or not gone.
        let (pending, alone) = compose::outbox();
        let stuck = pending.iter().filter(|p| p.fate != Fate::Waiting).count();
        if stuck > 0 {
            app.status = format!(
                "{stuck} mail in the outbox didn't go: u takes one back to the review screen, y there sends it"
            );
        } else if let Some(f) = alone.first() {
            app.status = format!("a mail in the outbox has no draft: {}", compose::tilde(f));
        }
        app.pending = pending;
        app.load()?;
        app.count_unread();
        Ok(app)
    }

    /// Reads the list again, keeping the selected thread selected if it is still there.
    pub fn load(&mut self) -> Result<()> {
        let keep = self.threads.get(self.sel).map(|t| t.thread.clone());
        let want = (self.threads.len().max(PAGE)).div_ceil(PAGE) * PAGE;
        self.threads = notmuch::search(&self.query, 0, want)?;
        self.more = self.threads.len() == want;
        if let Some(k) = keep
            && let Some(i) = self.threads.iter().position(|t| t.thread == k)
        {
            self.sel = i;
        }
        self.sel = self.sel.min(self.threads.len().saturating_sub(1));
        Ok(())
    }

    fn load_more(&mut self) {
        if !self.more || self.sel + 20 < self.threads.len() {
            return;
        }
        match notmuch::search(&self.query, self.threads.len(), PAGE) {
            Ok(more) => {
                self.more = more.len() == PAGE;
                self.threads.extend(more);
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn count_unread(&mut self) {
        for (i, s) in self.searches.iter().enumerate() {
            self.unread[i] =
                notmuch::count(&format!("({}) and (tag:unread or tag:pdf-waiting)", s.query)).ok();
        }
    }

    /// Called between keys: reloads when mail or tags changed, as the sync timer does.
    pub fn tick(&mut self) {
        // What became of each mail is read from the outbox, not from the clock: sent when the
        // script says so, and still here, to take back, when it didn't go.
        let mut said = None;
        self.pending.retain_mut(|p| {
            let before = p.fate;
            p.refresh();
            match p.fate {
                Fate::Sent => said = Some(format!("✉ sent to {}", p.to)),
                Fate::Gone => said = Some(format!("the mail to {} was taken back elsewhere", p.to)),
                Fate::Failed if before != Fate::Failed => {
                    said = Some(format!("✉ the mail to {} didn't go: u takes it back", p.to));
                }
                Fate::Lost if before != Fate::Lost => {
                    said = Some(format!(
                        "✉ the mail to {} wasn't sent, its timer is gone: u takes it back",
                        p.to
                    ));
                }
                _ => {}
            }
            !matches!(p.fate, Fate::Sent | Fate::Gone)
        });
        if let Some(s) = said {
            self.status = s;
        }
        // Ticks come every two seconds; most find the database as it was, without running notmuch.
        let stamp = notmuch::db_stamp();
        if stamp.is_some() && stamp == self.stamp && !self.lastmod.is_empty() {
            return;
        }
        self.stamp = stamp;
        let Ok(m) = notmuch::lastmod() else { return };
        if m != self.lastmod {
            self.lastmod = m;
            if let Err(e) = self.load() {
                self.status = format!("{e:#}");
            }
            self.count_unread();
            // A sync may have sent a PDF, or review chosen one.
            if let Some(v) = &mut self.view
                && !v.shas.is_empty()
                && let Err(e) = v.load_pdfs()
            {
                self.status = format!("{e:#}");
            }
        }
    }

    fn switch(&mut self, i: usize) {
        if i >= self.searches.len() {
            return;
        }
        self.current = Some(i);
        self.query = self.searches[i].query.clone();
        self.threads.clear();
        self.sel = 0;
        if let Err(e) = self.load() {
            self.status = format!("{e:#}");
        }
    }

    fn selected(&self) -> Option<&Thread> {
        self.view
            .as_ref()
            .map(|v| &v.thread)
            .or_else(|| self.threads.get(self.sel))
    }

    /// Adds or removes `tag` on the thread's messages that don't have it so already.
    fn change(&mut self, thread: &str, change: &str, done: &str) {
        let (sign, tag) = change.split_at(1);
        let q = if sign == "+" {
            format!("thread:{thread} and not tag:{tag}")
        } else {
            format!("thread:{thread} and tag:{tag}")
        };
        self.change_ids(notmuch::message_ids(&q), change, done);
    }

    fn change_ids(&mut self, ids: Result<Vec<String>>, change: &str, done: &str) {
        let res = ids.and_then(|ids| notmuch::tag(&ids, change).map(|()| ids));
        match res {
            Ok(ids) if ids.is_empty() => self.status = format!("{done}: nothing to change"),
            Ok(ids) => {
                self.undo.push(Undo::Tags {
                    ids,
                    change: change.to_string(),
                });
                self.status = format!("{done} · u undoes");
                self.lastmod.clear();
                self.tick();
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn undo(&mut self) {
        // A mail in the outbox comes first: that's what u is for, right after y.
        if let Some(mut p) = self.pending.pop() {
            match p.take_back() {
                Ok(true) => {
                    self.status = format!("taken back: the mail to {} didn't go", p.to);
                    self.mode = Mode::Review {
                        draft: Box::new(p.draft),
                        scroll: 0,
                        discard: false,
                    };
                }
                Ok(false) => {
                    p.refresh();
                    if matches!(p.fate, Fate::Sent | Fate::Gone) {
                        self.status = format!("too late: the mail to {} has gone", p.to);
                    } else {
                        // The script has it; the next tick says how that went.
                        self.status = format!("too late: the mail to {} is being sent", p.to);
                        self.pending.push(p);
                    }
                }
                Err(e) => self.status = format!("{e:#}"),
            }
            return;
        }
        let Some(u) = self.undo.pop() else {
            self.status = "nothing to undo".into();
            return;
        };
        let (ids, change) = match u {
            Undo::Tags { ids, change } => (ids, change),
            Undo::File { sha, file, before } => {
                return match archive::choose(&sha, &before) {
                    Ok(()) => self.chosen(format!("undone: {file} is as it was")),
                    Err(e) => self.status = format!("{e:#}"),
                };
            }
            Undo::Sender { address, changes } => {
                let res = changes
                    .iter()
                    .try_for_each(|(change, ids)| notmuch::tag(ids, &invert(change)))
                    .and_then(|()| screen::sender("forget", &address).map(|_| ()));
                self.status = match res {
                    Ok(()) => format!("undone: {address} is a sender like any other again"),
                    Err(e) => format!("{e:#}"),
                };
                self.lastmod.clear();
                return self.tick();
            }
        };
        match notmuch::tag(&ids, &invert(&change)) {
            Ok(()) => {
                self.status = format!(
                    "undone: {} on {} message{}",
                    change,
                    ids.len(),
                    if ids.len() == 1 { "" } else { "s" }
                );
                self.lastmod.clear();
                self.tick();
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn open(&mut self, i: usize) {
        let Some(t) = self.threads.get(i).cloned() else {
            return;
        };
        self.sel = i;
        match notmuch::show_thread(&t.thread) {
            Ok(msgs) => {
                // Read once opened: the unread ones it shows. m marks it unread again.
                let unread: Vec<String> = msgs
                    .iter()
                    .filter(|m| m.has("unread"))
                    .map(|m| m.id.clone())
                    .collect();
                let first = msgs.iter().position(|m| m.has("unread"));
                if !unread.is_empty() {
                    let _ = notmuch::tag(&unread, "-unread");
                }
                let mut v = ThreadView {
                    thread: t,
                    msgs,
                    doc: Doc::default(),
                    width: 0,
                    scroll: 0,
                    focus: None,
                    html: self.html,
                    fresh: true,
                    pdfs: Pdfs::new(),
                    categories: vec![],
                    shas: HashMap::new(),
                };
                v.layout(80);
                if let Err(e) = v.load_pdfs() {
                    self.status = format!("PDFs: {e:#}");
                }
                // Start at the first unread message, or the last one.
                let at = first.unwrap_or(v.msgs.len().saturating_sub(1));
                v.scroll = v.doc.starts.get(at).copied().unwrap_or(0);
                if v.msgs.len() == 1 {
                    v.scroll = 0;
                }
                if v.thread.has("screener") || v.thread.has("suspect") {
                    self.status = why_line(&v.msgs);
                }
                self.view = Some(v);
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    pub fn key(&mut self, k: KeyEvent) {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => {}
            Mode::Search(mut q) => {
                match k.code {
                    KeyCode::Enter if !q.trim().is_empty() => {
                        self.current = None;
                        self.query = q.trim().to_string();
                        self.threads.clear();
                        self.sel = 0;
                        self.view = None;
                        if let Err(e) = self.load() {
                            self.status = format!("{e:#}");
                        }
                    }
                    KeyCode::Enter | KeyCode::Esc => {}
                    KeyCode::Backspace => {
                        q.pop();
                        self.mode = Mode::Search(q);
                    }
                    KeyCode::Char(c) => {
                        q.push(c);
                        self.mode = Mode::Search(q);
                    }
                    _ => self.mode = Mode::Search(q),
                }
                return;
            }
            Mode::Explain { title, body, keys } => {
                match k.code {
                    KeyCode::Char('?') => self.mode = Mode::Help,
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {}
                    _ => self.mode = Mode::Explain { title, body, keys },
                }
                return;
            }
            Mode::Help => {
                if !matches!(k.code, KeyCode::Esc | KeyCode::Char('q' | '?') | KeyCode::Enter) {
                    self.mode = Mode::Help;
                }
                return;
            }
            Mode::File(form) => return self.key_file(form, k),
            Mode::Review {
                draft,
                scroll,
                discard,
            } => return self.key_review(*draft, scroll, discard, k),
        }
        self.status.clear();
        if k.code != KeyCode::Char('U') {
            self.unsub = None;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // Keys that mean the same in the list and in a thread.
        match k.code {
            KeyCode::Char('?') => {
                self.mode = self.explain();
                return;
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Search(if self.current.is_none() {
                    self.query.clone()
                } else {
                    String::new()
                });
                return;
            }
            KeyCode::Char('u') if !ctrl => return self.undo(),
            KeyCode::Char('c') if !ctrl => return self.write(Draft::new_mail()),
            KeyCode::Char('e') if !ctrl => return self.resume(),
            KeyCode::Char(c @ ('r' | 'R' | 'f')) if !ctrl => return self.answer(c),
            KeyCode::Char(c @ '0'..='9') => {
                self.view = None;
                // 0 is the tenth, as on the keyboard.
                return self.switch((c as usize + 9 - '0' as usize) % 10);
            }
            KeyCode::Char('a' | 'd' | 's' | 'm' | '!') if !ctrl => return self.act(k.code),
            KeyCode::Char(c @ ('A' | 'B')) => return self.sender_rule(c),
            KeyCode::Char('U') => return self.unsubscribe(),
            _ => {}
        }
        if self.view.is_some() {
            self.key_thread(k, ctrl);
        } else {
            self.key_list(k, ctrl);
        }
    }

    fn act(&mut self, code: KeyCode) {
        let Some(t) = self.selected().cloned() else { return };
        let in_view = self.view.is_some();
        match code {
            KeyCode::Char('a') => {
                if !t.has("inbox") {
                    self.status = "not in the Inbox".into();
                    return;
                }
                self.change(&t.thread, "-inbox", "archived");
                self.view = None;
            }
            KeyCode::Char('d') => {
                if t.has("deleted") {
                    self.change(&t.thread, "-deleted", "back from the Bin");
                } else {
                    self.change(&t.thread, "+deleted", "to the Bin at the next sync");
                }
                self.view = None;
            }
            KeyCode::Char('s') => {
                if t.has("flagged")
                    || self
                        .view
                        .as_ref()
                        .is_some_and(|v| v.msgs.iter().any(|m| m.has("flagged")))
                {
                    self.change(&t.thread, "-flagged", "unstarred");
                } else {
                    // The message being read, in a thread; in the list, the newest.
                    let id = match &self.view {
                        Some(v) => v.msgs[v.current()].id.clone(),
                        None => notmuch::message_ids(&format!("thread:{}", t.thread))
                            .ok()
                            .and_then(|ids| ids.into_iter().next())
                            .unwrap_or_default(),
                    };
                    self.change_ids(Ok(vec![id]), "+flagged", "starred");
                }
                if let Some(v) = &mut self.view
                    && let Ok(msgs) = notmuch::show_thread(&v.thread.thread)
                {
                    v.msgs = msgs;
                }
            }
            KeyCode::Char('!') => {
                if t.has("spam") {
                    self.change(&t.thread, "-spam", "not spam: back to the Inbox at the next sync");
                } else {
                    self.change(
                        &t.thread,
                        "+spam",
                        "reported as spam: Gmail's Spam at the next sync",
                    );
                }
                self.view = None;
            }
            KeyCode::Char('m') => {
                if t.has("unread") && !in_view {
                    self.change(&t.thread, "-unread", "marked read");
                } else {
                    let id = match &self.view {
                        Some(v) => v.msgs[v.current()].id.clone(),
                        None => notmuch::message_ids(&format!("thread:{}", t.thread))
                            .ok()
                            .and_then(|ids| ids.into_iter().next())
                            .unwrap_or_default(),
                    };
                    self.change_ids(Ok(vec![id]), "+unread", "marked unread");
                    self.view = None;
                }
            }
            _ => {}
        }
        if let Some(i) = self.threads.iter().position(|x| x.thread == t.thread) {
            self.sel = i;
        }
        self.sel = self.sel.min(self.threads.len().saturating_sub(1));
    }

    fn key_list(&mut self, k: KeyEvent, ctrl: bool) {
        let len = self.threads.len();
        let step = |s: usize, d: isize| (s as isize + d).clamp(0, len.saturating_sub(1) as isize) as usize;
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                if self.current.is_none() {
                    // Back from a typed search to the searches.
                    self.switch(0);
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => self.sel = step(self.sel, 1),
            KeyCode::Char('k') | KeyCode::Up => self.sel = step(self.sel, -1),
            KeyCode::Char('d') if ctrl => self.sel = step(self.sel, 15),
            KeyCode::Char('u') if ctrl => self.sel = step(self.sel, -15),
            KeyCode::PageDown => self.sel = step(self.sel, 15),
            KeyCode::PageUp => self.sel = step(self.sel, -15),
            KeyCode::Char('g') | KeyCode::Home => self.sel = 0,
            KeyCode::Char('G') | KeyCode::End => self.sel = len.saturating_sub(1),
            KeyCode::Tab => self.switch((self.current.map_or(0, |c| c + 1)) % self.searches.len()),
            KeyCode::BackTab => {
                let n = self.searches.len();
                self.switch((self.current.unwrap_or(0) + n - 1) % n)
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => self.open(self.sel),
            _ => {}
        }
        self.load_more();
    }

    fn key_thread(&mut self, k: KeyEvent, ctrl: bool) {
        let page = self.page.max(1);
        let Some(v) = &mut self.view else { return };
        let max = v.doc.lines.len().saturating_sub(page / 2);
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
                self.view = None;
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => v.scroll = (v.scroll + 1).min(max),
            KeyCode::Char('k') | KeyCode::Up => v.scroll = v.scroll.saturating_sub(1),
            KeyCode::Char('d') if ctrl => v.scroll = (v.scroll + page / 2).min(max),
            KeyCode::Char('u') if ctrl => v.scroll = v.scroll.saturating_sub(page / 2),
            KeyCode::Char(' ') | KeyCode::PageDown => v.scroll = (v.scroll + page.saturating_sub(2)).min(max),
            KeyCode::PageUp => v.scroll = v.scroll.saturating_sub(page.saturating_sub(2)),
            KeyCode::Char('g') | KeyCode::Home => v.scroll = 0,
            KeyCode::Char('G') | KeyCode::End => v.scroll = max,
            KeyCode::Char('n') => {
                if let Some(&s) = v.doc.starts.iter().find(|&&s| s > v.scroll) {
                    v.scroll = s;
                }
            }
            KeyCode::Char('p') => {
                if let Some(&s) = v.doc.starts.iter().rev().find(|&&s| s < v.scroll) {
                    v.scroll = s;
                }
            }
            KeyCode::Tab => v.step_focus(1, page),
            KeyCode::BackTab => v.step_focus(-1, page),
            KeyCode::Char('H') => {
                v.html = !v.html;
                self.html = v.html;
                let w = v.width;
                v.width = 0;
                v.layout(w);
                self.status = if v.html {
                    "HTML where there is one".into()
                } else {
                    "plain text where there is one".into()
                };
            }
            KeyCode::Char('J') => {
                let next = self.sel + 1;
                if next < self.threads.len() {
                    self.open(next);
                }
                return;
            }
            KeyCode::Char('K') => {
                if self.sel > 0 {
                    self.open(self.sel - 1);
                }
                return;
            }
            KeyCode::Enter | KeyCode::Char('o') => return self.open_focus(false),
            KeyCode::Char('t') => return self.start_file(),
            KeyCode::Char('x') => return self.leave_out(),
            KeyCode::Char('w') => return self.open_focus(true),
            KeyCode::Char('y') => {
                if let Some(t) = v.focused() {
                    let text = match t {
                        Target::Link(u) => u.clone(),
                        Target::Attachment { name, .. } => name.clone(),
                    };
                    self.status = match copy(&text) {
                        Ok(()) => format!("copied {text}"),
                        Err(e) => format!("not copied: {e:#}"),
                    };
                }
                return;
            }
            _ => {}
        }
        // A focus scrolled out of sight lets go, so Enter never opens what isn't on screen.
        if let Some(f) = v.focus
            && let Some(fp) = v.doc.focus.get(f)
            && (fp.line < v.scroll || fp.line >= v.scroll + page)
        {
            v.focus = None;
        }
    }

    /// Enter on a link opens it in the browser; on an attachment, opens it with its program.
    /// `save` writes an attachment to ~/Downloads instead.
    fn open_focus(&mut self, save: bool) {
        let Some(v) = &self.view else { return };
        let Some(t) = v.focused().cloned() else {
            self.status = "Tab moves to the next link or attachment".into();
            return;
        };
        self.status = match t {
            Target::Link(url) if !save && !opens(&url) => NOT_OPENED.into(),
            Target::Link(url) if !save => match spawn("xdg-open", &[&url]) {
                Ok(()) => format!("opened {url}"),
                Err(e) => format!("not opened: {e:#}"),
            },
            Target::Link(_) => "w saves attachments; this is a link".into(),
            Target::Attachment { msg, part, name, .. } => match save_part(&msg, part, &name, save) {
                Ok(path) if save => format!("saved {}", path.display()),
                Ok(path) => match spawn("xdg-open", &[&path.to_string_lossy()]) {
                    Ok(()) => format!("opened {name}"),
                    Err(e) => format!("not opened: {e:#}"),
                },
                Err(e) => format!("{e:#}"),
            },
        };
    }

    /// Starts the editor on a draft (main runs it, the screen suspended).
    fn write(&mut self, d: Draft) {
        self.edit = Some(d);
    }

    /// `r` `R` `f`: a reply, a reply to all, or a forward of the message being read; in the list,
    /// of the thread's newest.
    fn answer(&mut self, c: char) {
        let msg = match &self.view {
            Some(v) => v.msgs.get(v.current()).cloned(),
            None => self
                .threads
                .get(self.sel)
                .and_then(|t| notmuch::show_thread(&t.thread).ok())
                .and_then(|ms| ms.into_iter().max_by_key(|m| m.timestamp)),
        };
        let Some(m) = msg else { return };
        let d = match c {
            'r' => Draft::reply(&m, false),
            'R' => Draft::reply(&m, true),
            _ => Draft::forward(&m),
        };
        match d {
            Ok(d) => self.write(d),
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    /// `e`: back to one of mmm's drafts: the message being read, in a thread; in the list, the
    /// thread's newest draft.
    fn resume(&mut self) {
        let id = match &self.view {
            Some(v) => v.msgs.get(v.current()).map(|m| m.id.clone()),
            None => self.threads.get(self.sel).and_then(|t| {
                notmuch::message_ids(&format!("thread:{} and tag:draft", t.thread))
                    .ok()?
                    .into_iter()
                    .find(|m| compose::draft_of(m).is_some())
            }),
        };
        let Some(id) = id else { return };
        let Some(draft) = compose::draft_of(&id) else {
            self.status = if self
                .view
                .as_ref()
                .is_some_and(|v| v.msgs[v.current()].has("draft"))
            {
                "a draft from Gmail: mmm resumes only its own; open it on the web or phone".into()
            } else {
                "e resumes a draft: there's none here (the Drafts tab has them)".into()
            };
            return;
        };
        match Draft::load(draft) {
            Ok(d) => {
                // Sent and still in its minute: writing on takes it back, or the mail would go
                // as it was when sent, and take the draft being written with it.
                if !compose::cancel(&d.id) {
                    self.status = "not resumed: this mail is being sent right now".into();
                    return;
                }
                self.pending.retain(|p| p.draft.id != d.id);
                self.write(d)
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    /// Back from the editor: an empty new mail is dropped; anything else is reviewed.
    pub fn edited(&mut self, mut d: Draft, ok: bool) {
        if !ok {
            self.status = format!("the editor failed; the draft is in {}", compose::tilde(&d.file));
            return;
        }
        if let Err(e) = d.read() {
            self.status = format!("{e:#}");
            return;
        }
        if d.kind == compose::Kind::New && d.body.trim().is_empty() && d.to.trim().is_empty() {
            let _ = d.forget();
            self.status = "nothing written: dropped".into();
            return;
        }
        if d.lang_why != "chosen with l" {
            d.choose_lang();
        }
        // Kept as it is now, so it's in Drafts whatever happens next.
        if let Err(e) = d.save() {
            self.status = format!("not kept as a draft: {e:#}");
        }
        self.mode = Mode::Review {
            draft: Box::new(d),
            scroll: 0,
            discard: false,
        };
    }

    fn key_review(&mut self, mut d: Draft, mut scroll: usize, discard: bool, k: KeyEvent) {
        if discard {
            match k.code {
                KeyCode::Char('y') => {
                    // A draft sent and still in its minute: the send is called off with it.
                    if !compose::cancel(&d.id) {
                        self.status = "not discarded: this mail is being sent right now".into();
                        return;
                    }
                    self.pending.retain(|p| p.draft.id != d.id);
                    self.status = match d.forget() {
                        Ok(()) => "discarded".into(),
                        Err(e) => format!("{e:#}"),
                    };
                }
                _ => {
                    self.mode = Mode::Review {
                        draft: Box::new(d),
                        scroll,
                        discard: false,
                    }
                }
            }
            return;
        }
        match k.code {
            KeyCode::Char('y') => match compose::queue(&d) {
                Ok(p) => {
                    // The footer counts down to it. An earlier send of this draft was called
                    // off by this one.
                    self.pending.retain(|q| q.draft.id != p.draft.id);
                    self.pending.push(p);
                    return;
                }
                Err(e) => self.status = format!("not sent: {e:#}"),
            },
            KeyCode::Char('e') | KeyCode::Char('n') | KeyCode::Enter => {
                if let Err(e) = d.write() {
                    self.status = format!("{e:#}");
                } else {
                    self.edit = Some(d);
                    return;
                }
            }
            KeyCode::Char('l') => {
                let i = d
                    .lang
                    .and_then(|l| compose::LANGS.iter().position(|(c, _)| *c == l));
                let next = match i {
                    Some(i) if i + 1 < compose::LANGS.len() => Some(compose::LANGS[i + 1].0),
                    Some(_) => None,
                    None => Some(compose::LANGS[0].0),
                };
                d.set_lang(next);
            }
            KeyCode::Char('f') => {
                let ids = compose::identities();
                if !ids.is_empty() {
                    let i = ids
                        .iter()
                        .position(|x| *x == d.from)
                        .map_or(0, |i| (i + 1) % ids.len());
                    d.from = ids[i].clone();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => scroll += 1,
            KeyCode::Char('k') | KeyCode::Up => scroll = scroll.saturating_sub(1),
            KeyCode::Char(' ') | KeyCode::PageDown => scroll += 10,
            KeyCode::PageUp => scroll = scroll.saturating_sub(10),
            KeyCode::Char('q') | KeyCode::Esc => {
                self.status = match d.save() {
                    Ok(()) => "kept in Drafts: e there resumes it".into(),
                    Err(e) => format!("not kept: {e:#}"),
                };
                self.lastmod.clear();
                self.tick();
                return;
            }
            KeyCode::Char('D') => {
                self.mode = Mode::Review {
                    draft: Box::new(d),
                    scroll,
                    discard: true,
                };
                return;
            }
            _ => {}
        }
        self.mode = Mode::Review {
            draft: Box::new(d),
            scroll,
            discard: false,
        };
    }

    /// The PDF `t` and `x` act on: the one focused, or else the thread's only one waiting.
    pub fn pdf_target(&self) -> Option<(&Target, &archive::Pdf)> {
        let v = self.view.as_ref()?;
        let key = |t: &Target| match t {
            Target::Attachment { msg, part, .. } => Some((msg.clone(), *part)),
            Target::Link(_) => None,
        };
        if let Some(t) = v.focused() {
            return key(t).and_then(|k| v.pdfs.get(&k)).map(|p| (t, p));
        }
        let mut waiting = v.doc.focus.iter().filter_map(|f| {
            let p = key(&f.target).and_then(|k| v.pdfs.get(&k))?;
            (p.state == State::Waiting).then_some((&f.target, p))
        });
        let one = waiting.next();
        if waiting.next().is_some() { None } else { one }
    }

    fn start_file(&mut self) {
        let Some((Target::Attachment { name: file, msg, .. }, p)) = self.pdf_target() else {
            self.status = "t files a PDF in Nextcloud: Tab to one first".into();
            return;
        };
        if p.state == State::Sent {
            self.status = format!("already in Nextcloud: {}", p.r#where.as_deref().unwrap_or(""));
            return;
        }
        let v = self.view.as_ref().expect("a PDF is in a thread");
        // What it was chosen as, else what the rules suggest, else the sender and file's name.
        let (cat, name) = match (&p.category, &p.name, &p.suggested) {
            (Some(c), Some(n), _) => (Some(c.clone()), n.clone()),
            (_, _, Some((c, n))) if c != "-" => (Some(c.clone()), n.clone()),
            _ => {
                let from = v
                    .msgs
                    .iter()
                    .find(|m| &m.id == msg)
                    .map(|m| m.from.as_str())
                    .unwrap_or("");
                let stem = file
                    .strip_suffix(".pdf")
                    .or(file.strip_suffix(".PDF"))
                    .unwrap_or(file);
                (None, format!("{} - {stem}", render::name(from)))
            }
        };
        let mut categories = v.categories.clone();
        let category = cat.map(|c| {
            categories.iter().position(|x| *x == c).unwrap_or_else(|| {
                categories.push(c);
                categories.len() - 1
            })
        });
        if categories.is_empty() {
            self.status = "deck-mail lists no categories to file it under".into();
            return;
        }
        self.mode = Mode::File(FileForm {
            sha: p.sha256.clone(),
            file: file.clone(),
            before: p.choice(),
            categories,
            row: if category.is_some() { 1 } else { 0 },
            category,
            cursor: name.chars().count(),
            name,
        });
    }

    fn key_file(&mut self, mut f: FileForm, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let n = f.categories.len();
        match k.code {
            KeyCode::Esc => return,
            KeyCode::Enter => {
                let (Some(c), name) = (f.category, f.name.trim()) else {
                    f.row = 0;
                    self.status = "choose a category: ← → or its number".into();
                    self.mode = Mode::File(f);
                    return;
                };
                if name.is_empty() {
                    self.status = "it needs a name".into();
                    self.mode = Mode::File(f);
                    return;
                }
                let Some(category) = f.categories.get(c) else {
                    f.category = None;
                    self.mode = Mode::File(f);
                    return;
                };
                let choice = Choice::To(category.clone(), name.to_string());
                return self.choose(f.sha, f.file, f.before, choice);
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => f.row = 1 - f.row,
            _ if f.row == 0 => match k.code {
                KeyCode::Left | KeyCode::Char('h') => f.category = step_category(f.category, n, true),
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => {
                    f.category = step_category(f.category, n, false);
                }
                KeyCode::Char(d @ '0'..='9') => {
                    let i = if d == '0' { 9 } else { d as usize - '1' as usize };
                    if i < n {
                        f.category = Some(i);
                        f.row = 1;
                    }
                }
                _ => {}
            },
            KeyCode::Char('u') if ctrl => {
                f.name.clear();
                f.cursor = 0;
            }
            KeyCode::Char('a') if ctrl => f.cursor = 0,
            KeyCode::Char('e') if ctrl => f.cursor = f.name.chars().count(),
            KeyCode::Home => f.cursor = 0,
            KeyCode::End => f.cursor = f.name.chars().count(),
            KeyCode::Left => f.cursor = f.cursor.saturating_sub(1),
            KeyCode::Right => f.cursor = (f.cursor + 1).min(f.name.chars().count()),
            KeyCode::Backspace if f.cursor > 0 => {
                let at = f.name.char_indices().nth(f.cursor - 1).map_or(0, |(i, _)| i);
                f.name.remove(at);
                f.cursor -= 1;
            }
            KeyCode::Delete => {
                if let Some((at, _)) = f.name.char_indices().nth(f.cursor) {
                    f.name.remove(at);
                }
            }
            KeyCode::Char(c) if !ctrl => {
                let at = f
                    .name
                    .char_indices()
                    .nth(f.cursor)
                    .map_or(f.name.len(), |(i, _)| i);
                f.name.insert(at, c);
                f.cursor += 1;
            }
            _ => {}
        }
        self.mode = Mode::File(f);
    }

    fn leave_out(&mut self) {
        let Some((Target::Attachment { name: file, .. }, p)) = self.pdf_target() else {
            self.status = "x leaves a PDF out of Nextcloud: Tab to one first".into();
            return;
        };
        match p.state {
            State::Sent => {
                self.status = format!("already in Nextcloud: {}", p.r#where.as_deref().unwrap_or(""));
            }
            State::LeftOut => self.status = format!("{file} is left out already"),
            _ => {
                let (sha, file, before) = (p.sha256.clone(), file.clone(), p.choice());
                self.choose(sha, file, before, Choice::Out);
            }
        }
    }

    fn choose(&mut self, sha: String, file: String, before: Choice, choice: Choice) {
        if let Err(e) = archive::choose(&sha, &choice) {
            self.status = format!("{e:#}");
            return;
        }
        let done = match &choice {
            Choice::To(c, n) => format!("{file}: to Nextcloud at the next sync, as {c} / {n} · u undoes"),
            Choice::Out => format!("{file}: left out of Nextcloud · u undoes"),
            Choice::Forget => format!("{file}: waits again"),
        };
        self.undo.push(Undo::File { sha, file, before });
        self.chosen(done);
    }

    /// After a PDF's choice: its line, the Waiting tab and the counts as they are now.
    fn chosen(&mut self, status: String) {
        if let Some(v) = &mut self.view
            && let Err(e) = v.load_pdfs()
        {
            self.status = format!("{e:#}");
            return;
        }
        self.lastmod.clear();
        self.tick();
        self.status = status;
    }

    /// The message `A`, `B` and `U` act on: in a thread the one read, in the list the newest.
    fn target_message(&self) -> Option<Message> {
        if let Some(v) = &self.view {
            return v.msgs.get(v.current()).cloned();
        }
        // Not your own reply: the newest someone else sent, if there is one.
        let t = self.threads.get(self.sel)?;
        notmuch::show_thread(&t.thread)
            .ok()?
            .into_iter()
            .max_by_key(|m| (!m.has("sent"), m.timestamp))
    }

    /// `A` lets the sender in (their mail leaves the Screener), `B` blocks them (to the Bin, and
    /// so will their next mail). deck-mail keeps the rule; u takes it back.
    fn sender_rule(&mut self, c: char) {
        let Some(m) = self.target_message() else { return };
        let address = screen::address(&m.from);
        let (kind, done) = if c == 'A' {
            ("allow", format!("{address} let in: their mail goes to the Inbox"))
        } else {
            ("block", format!("{address} blocked: their mail goes to the Bin"))
        };
        match screen::sender(kind, &address) {
            Ok(changes) => {
                self.undo.push(Undo::Sender { address, changes });
                self.status = format!("{done} · u undoes");
                self.view = None;
                self.lastmod.clear();
                self.tick();
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    /// `U` says how the mail unsubscribes; `U` again does it: the one-click request, a mail to
    /// write, or the list's page in the browser.
    fn unsubscribe(&mut self) {
        let Some(m) = self.target_message() else { return };
        if let Some((id, u)) = self.unsub.take()
            && id == m.id
        {
            self.status = match u.method.as_str() {
                "one-click" => match screen::unsubscribe(&id) {
                    Ok(()) => format!("unsubscribed at {}: a archives this one", u.host),
                    Err(e) => format!("{e:#}"),
                },
                "link" if !opens(&u.target) => NOT_OPENED.into(),
                "link" => match spawn("xdg-open", &[&u.target]) {
                    Ok(()) => format!("{}'s unsubscribe page is open in the browser", u.host),
                    Err(e) => format!("{e:#}"),
                },
                _ => {
                    let mut d = Draft::new_mail();
                    d.to = u.target.clone();
                    d.subject = u.subject.clone();
                    d.body = u.body.clone();
                    return self.write(d);
                }
            };
            return;
        }
        match screen::unsubscribe_check(&m.id) {
            Ok(u) => {
                self.status = match u.method.as_str() {
                    "one-click" => format!("U again: unsubscribe at {} (one click, nothing to open)", u.host),
                    "mailto" => format!("U again: write the unsubscribe mail to {}", u.target),
                    "link" => format!("U again: open {}'s unsubscribe page in the browser", u.host),
                    _ => "this mail names no way to unsubscribe (no List-Unsubscribe)".into(),
                };
                if u.method != "none" {
                    self.unsub = Some((m.id, u));
                }
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn explain(&self) -> Mode {
        if let Some(v) = &self.view {
            return crate::ui::explain_thread(self, v);
        }
        crate::ui::explain_list(self)
    }
}

/// The footer's line for a screened or suspect thread: who the sender is to you and what looked
/// wrong, from deck-mail's filter.
fn why_line(msgs: &[Message]) -> String {
    let ids: Vec<String> = msgs.iter().map(|m| m.id.clone()).collect();
    let Ok(whys) = screen::why(&ids) else {
        return String::new();
    };
    let mut parts = vec![];
    if let Some(w) = whys.iter().find(|w| w.new_sender) {
        parts.push(format!("? {} is new to you: A lets them in, B blocks", w.sender));
    }
    let reasons: Vec<&str> = whys
        .iter()
        .flat_map(|w| w.reasons.iter().map(String::as_str))
        .collect();
    if !reasons.is_empty() {
        parts.push(format!("⚠ {} · ! reports spam", reasons.join("; ")));
    }
    parts.join(" · ")
}

/// Writes an attachment out: to ~/Downloads with `save`, else to a private folder to open it.
fn save_part(msg: &str, part: u64, name: &str, save: bool) -> Result<PathBuf> {
    let bytes = notmuch::part(msg, part)?;
    let safe: String = name
        .chars()
        .map(|c| if c == '/' || c == '\0' { '_' } else { c })
        .collect::<String>()
        .trim_start_matches('.')
        .to_string();
    let safe = if safe.is_empty() || safe.starts_with('(') {
        "attachment".to_string()
    } else {
        safe
    };
    let dir = if save {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Downloads")
    } else {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("mmm")
    };
    std::fs::create_dir_all(&dir)?;
    let mut path = dir.join(&safe);
    let mut n = 2;
    while save && path.exists() {
        let (stem, ext) = match safe.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (safe.clone(), String::new()),
        };
        path = dir.join(format!("{stem}-{n}{ext}"));
        n += 1;
    }
    std::fs::write(&path, bytes)?;
    Ok(path)
}

impl ThreadView {
    /// Lays the thread out for `width` columns, when that changed.
    pub fn layout(&mut self, width: usize) {
        if width == self.width {
            return;
        }
        // Keep the reader at the same message across a re-wrap.
        let at = self.current();
        let into = self
            .scroll
            .saturating_sub(self.doc.starts.get(at).copied().unwrap_or(0));
        self.width = width;
        self.doc = render::thread(&self.msgs, width, self.html, &self.pdfs);
        self.scroll = self.doc.starts.get(at).copied().unwrap_or(0) + into;
        self.focus = None;
    }

    /// Asks deck-mail what became of the thread's PDFs, and lays it out again to show it.
    pub fn load_pdfs(&mut self) -> Result<()> {
        for f in &self.doc.focus {
            if let Target::Attachment {
                msg,
                part,
                name,
                ctype,
            } = &f.target
                && archive::is_pdf(name, ctype)
                && !self.shas.contains_key(&(msg.clone(), *part))
            {
                let bytes = notmuch::part(msg, *part)?;
                self.shas.insert((msg.clone(), *part), archive::sha256(&bytes));
            }
        }
        if self.shas.is_empty() {
            return Ok(());
        }
        let mut msgs: Vec<String> = self.shas.keys().map(|(m, _)| m.clone()).collect();
        msgs.sort();
        msgs.dedup();
        let (categories, found) = archive::pdfs(&msgs)?;
        self.categories = categories;
        self.pdfs = self
            .shas
            .iter()
            .filter_map(|(k, sha)| {
                let p = found.iter().find(|p| p.message == k.0 && &p.sha256 == sha)?;
                Some((k.clone(), p.clone()))
            })
            .collect();
        // Lay out again at the same width, keeping the place and the focus.
        let (w, focus) = (self.width, self.focus);
        self.width = 0;
        self.layout(w);
        self.focus = focus.filter(|&f| f < self.doc.focus.len());
        Ok(())
    }

    /// The message at the top of the screen.
    pub fn current(&self) -> usize {
        self.doc
            .starts
            .iter()
            .rposition(|&s| s <= self.scroll)
            .unwrap_or(0)
    }

    pub fn focused(&self) -> Option<&Target> {
        self.focus.and_then(|f| self.doc.focus.get(f)).map(|f| &f.target)
    }

    /// Tab: the next link or attachment, from the one focused or from the top of the screen.
    fn step_focus(&mut self, dir: isize, page: usize) {
        let n = self.doc.focus.len();
        if n == 0 {
            return;
        }
        let next = match self.focus {
            Some(f) if dir > 0 => (f + 1 < n).then_some(f + 1),
            Some(f) => f.checked_sub(1),
            None if dir > 0 => self.doc.focus.iter().position(|f| f.line >= self.scroll),
            None => self.doc.focus.iter().rposition(|f| f.line < self.scroll + page),
        };
        let Some(next) = next else { return };
        self.focus = Some(next);
        let line = self.doc.focus[next].line;
        if line < self.scroll || line >= self.scroll + page {
            self.scroll = line.saturating_sub(page / 3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_and_mail_links_open() {
        for ok in [
            "https://example.org/a",
            "HTTP://example.org",
            "mailto:ana@example.org",
        ] {
            assert!(opens(ok), "{ok}");
        }
        for no in [
            "file:///home/ana/x.desktop",
            "/home/ana/x.desktop",
            "~/x",
            "example.org/a",
            "//example.org/a",
            "javascript:x",
            "smb://example.org/share",
            " https://example.org",
            "-x",
            "https:",
            "",
        ] {
            assert!(!opens(no), "{no}");
        }
    }

    #[test]
    fn stepping_through_categories() {
        assert_eq!(step_category(None, 0, false), None);
        assert_eq!(step_category(None, 0, true), None);
        assert_eq!(step_category(Some(3), 0, true), None);
        assert_eq!(step_category(None, 3, true), Some(2));
        assert_eq!(step_category(None, 3, false), Some(0));
        assert_eq!(step_category(Some(2), 3, false), Some(0));
        assert_eq!(step_category(Some(0), 3, true), Some(2));
    }

    #[test]
    fn hosts_and_mismatches() {
        assert_eq!(host("https://www.Example.org:8080/a?b"), "example.org");
        assert_eq!(mismatch("www.hep.hr", "https://hep.hr/racun"), None);
        assert_eq!(mismatch("hep.hr", "https://moj.hep.hr/x"), None);
        assert_eq!(mismatch("Click here", "https://evil.example"), None);
        assert_eq!(
            destination("https://nam.safelink.example/redirect/?destination=https%3A%2F%2Fazure.microsoft.com%2Ffree%2F%23x&p=bT1").as_deref(),
            Some("https://azure.microsoft.com/free/#x")
        );
        assert_eq!(destination("https://example.org/a?b=c"), None);
        assert_eq!(
            mismatch("www.hep.hr", "https://hep-hr.evil.example/login").as_deref(),
            Some("the link says hep.hr but goes to hep-hr.evil.example")
        );
    }
}
