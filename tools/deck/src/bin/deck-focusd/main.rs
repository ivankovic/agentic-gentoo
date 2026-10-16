//! deck-focusd: how long each program is actively used: its window has the focus and the user is
//! not idle. A user service in the graphical session.
//!
//! niri's event stream says which window has the focus and its process. In a terminal the program
//! is the terminal's foreground process (aerc, nvim, chess-tui), not the terminal; wrappers that
//! start one program and wait for it (a script like `jail`, bwrap) are followed down to that
//! program. Wayland's ext-idle-notify says when the user went idle: the seconds before it fired
//! were idle too, so they are dropped. An idle inhibitor (a playing video) keeps the session
//! active, which counts watching as use.
//!
//! It records program paths, not packages: `deck usage` maps them, as deck-usaged does.

mod idle;

use anyhow::{Context, Result};
use deck::pkgmap::is_interpreter;
use deck::procfs;
use rusqlite::{Connection, params};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Seconds without input before the user counts as idle.
pub const IDLE_SECS: u32 = 60;
const FLUSH_EVERY: Duration = Duration::from_secs(60);

pub enum Msg {
    Niri(Value),
    NiriGone,
    Idle(bool),
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS active (
    hour    INTEGER NOT NULL,  -- unix time of the hour's start
    program TEXT    NOT NULL,  -- the program's path, or the script's for an interpreter
    app     TEXT    NOT NULL,  -- the window's app id (foot for anything in a terminal)
    secs    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (hour, program, app)
);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Follows niri's event stream, reconnecting when niri restarts.
fn niri(tx: Sender<Msg>) {
    loop {
        let res = (|| -> Result<()> {
            let path = std::env::var("NIRI_SOCKET").context("NIRI_SOCKET is not set")?;
            let mut sock = UnixStream::connect(&path).with_context(|| format!("connecting to {path}"))?;
            sock.write_all(b"\"EventStream\"\n")?;
            for line in BufReader::new(sock).lines() {
                let v: Value = serde_json::from_str(&line?)?;
                if tx.send(Msg::Niri(v)).is_err() {
                    return Ok(());
                }
            }
            Ok(())
        })();
        if let Err(e) = res {
            eprintln!("deck-focusd: niri: {e:#}");
        }
        if tx.send(Msg::NiriGone).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// The process the user works with in a window: for a terminal, its foreground program.
fn foreground(pid: i32) -> i32 {
    let fg = procfs::children(pid)
        .into_iter()
        .filter_map(procfs::stat)
        .find(|s| s.tty != 0 && s.tpgid > 0)
        .map(|s| s.tpgid);
    let Some(mut p) = fg else { return pid };
    for _ in 0..8 {
        let Some(exe) = procfs::exe(p) else { break };
        let wrapper = exe.ends_with("/bwrap") || (is_interpreter(&exe) && procfs::script_path(p).is_some());
        let kids = procfs::children(p);
        if !wrapper || kids.len() != 1 {
            break;
        }
        p = kids[0];
    }
    p
}

/// The path that names the program: its script's for an interpreter running one.
fn program(pid: i32) -> Option<String> {
    let p = foreground(pid);
    let exe = procfs::exe(p)?;
    if is_interpreter(&exe)
        && let Some(script) = procfs::script_path(p)
    {
        return Some(script);
    }
    Some(exe)
}

#[derive(Default)]
struct Focus {
    /// Window id → (pid, app id).
    windows: HashMap<u64, (i32, String)>,
    focused: Option<u64>,
}

impl Focus {
    fn window(&mut self, w: &Value) {
        let Some(id) = w["id"].as_u64() else { return };
        let pid = w["pid"].as_i64().unwrap_or(0) as i32;
        let app = w["app_id"].as_str().unwrap_or("").to_string();
        self.windows.insert(id, (pid, app));
        if w["is_focused"].as_bool() == Some(true) {
            self.focused = Some(id);
        }
    }

    fn event(&mut self, v: &Value) {
        if let Some(e) = v.get("WindowsChanged") {
            self.windows.clear();
            self.focused = None;
            for w in e["windows"].as_array().into_iter().flatten() {
                self.window(w);
            }
        } else if let Some(e) = v.get("WindowOpenedOrChanged") {
            self.window(&e["window"]);
        } else if let Some(e) = v.get("WindowClosed") {
            if let Some(id) = e["id"].as_u64() {
                self.windows.remove(&id);
                if self.focused == Some(id) {
                    self.focused = None;
                }
            }
        } else if let Some(e) = v.get("WindowFocusChanged") {
            self.focused = e["id"].as_u64();
        }
    }

    /// The focused window's program and app id.
    fn target(&self) -> Option<(String, String)> {
        let (pid, app) = self.windows.get(&self.focused?)?;
        if *pid <= 0 {
            return None;
        }
        Some((program(*pid)?, app.clone()))
    }
}

fn flush(db: &mut Connection, done: &mut HashMap<(i64, String, String), u64>) -> Result<()> {
    let tx = db.transaction()?;
    {
        let mut up = tx.prepare_cached(
            "INSERT INTO active (hour, program, app, secs) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (hour, program, app) DO UPDATE SET secs = secs + excluded.secs",
        )?;
        for ((hour, program, app), secs) in done.iter() {
            up.execute(params![hour, program, app, *secs as i64])?;
        }
    }
    tx.commit()?;
    done.clear();
    Ok(())
}

fn main() -> Result<()> {
    // --print: also say each second what is counted, for checking it against the screen.
    let print = std::env::args().any(|a| a == "--print");
    // --windows: what each open window would count as, then exit.
    if std::env::args().any(|a| a == "--windows") {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || niri(tx));
        let mut focus = Focus::default();
        while let Ok(Msg::Niri(v)) = rx.recv_timeout(Duration::from_secs(2)) {
            focus.event(&v);
            if v.get("WindowsChanged").is_some() {
                break;
            }
        }
        let mut ids: Vec<_> = focus.windows.keys().copied().collect();
        ids.sort();
        for id in ids {
            let (pid, app) = &focus.windows[&id];
            println!("{app}\t{pid}\t{}", program(*pid).unwrap_or_default());
        }
        return Ok(());
    }
    let path = deck::focus_db();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut db = Connection::open(&path).with_context(|| format!("opening {}", path.display()))?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute_batch(SCHEMA)?;
    db.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('idle_secs', ?1)",
        [IDLE_SECS.to_string()],
    )?;

    let (tx, rx) = mpsc::channel();
    let t = tx.clone();
    std::thread::spawn(move || niri(t));
    let t = tx.clone();
    std::thread::spawn(move || {
        if let Err(e) = idle::watch(t, IDLE_SECS * 1000) {
            eprintln!("deck-focusd: no idle notifications, counting all focused time: {e:#}");
        }
    });
    drop(tx);
    eprintln!("deck-focusd: writing {}", path.display());

    let mut focus = Focus::default();
    let mut idle = false;
    // The last IDLE_SECS seconds, kept back until it is clear the user was not idle in them.
    let mut pending: VecDeque<(i64, String, String)> = VecDeque::new();
    let mut done: HashMap<(i64, String, String), u64> = HashMap::new();
    let mut next = Instant::now() + Duration::from_secs(1);
    let mut flushed = Instant::now();
    loop {
        match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(Msg::Niri(v)) => focus.event(&v),
            Ok(Msg::NiriGone) => focus = Focus::default(),
            Ok(Msg::Idle(true)) => {
                idle = true;
                pending.clear();
            }
            Ok(Msg::Idle(false)) => idle = false,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        if now < next {
            continue;
        }
        // After a suspend, one tick: no catching up.
        next = now + Duration::from_secs(1);
        let target = if idle { None } else { focus.target() };
        if print {
            eprintln!(
                "{}",
                target.as_ref().map_or("-".into(), |(p, a)| format!("{a}\t{p}"))
            );
        }
        if let Some((program, app)) = target {
            pending.push_back((unix_now() / 3600 * 3600, program, app));
        }
        while pending.len() > IDLE_SECS as usize {
            let k = pending.pop_front().unwrap();
            *done.entry(k).or_default() += 1;
        }
        if flushed.elapsed() >= FLUSH_EVERY {
            if let Err(e) = flush(&mut db, &mut done) {
                eprintln!("deck-focusd: writing {}: {e:#}", path.display());
            }
            flushed = Instant::now();
        }
    }
    flush(&mut db, &mut done)
}
