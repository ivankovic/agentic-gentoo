//! deck-usaged: CPU and memory use per package, hour by hour, and the spikes in between.
//!
//! Every second it reads each process's CPU counter from /proc and credits the increase to the
//! package that owns the program (or the script, for interpreters). The kernel's process events
//! name programs as they start, and its exit reports give the CPU time of processes too short
//! for a reading. Every 10 s it reads and resets each process's peak memory, every 5 min its PSS.
//! Totals go to SQLite each minute; `deck usage` reads them.

mod netlink;
mod pkgmap;
mod procfs;
mod store;

use anyhow::Result;
use netlink::{ProcConnector, ProcEvent, Taskstats, ThreadExit};
use pkgmap::PkgMap;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use store::{Agg, Spike, Store};

const DEFAULT_DB: &str = "/var/lib/deck/usage.db";
const TICK: Duration = Duration::from_secs(1);
const PEAK_EVERY: u64 = 10;
const PSS_EVERY: u64 = 300;
const FLUSH_EVERY: u64 = 60;
/// A burst starts at half a core, lasts while above a tenth of one, ends after 3 quiet seconds,
/// and is kept as a spike if it used at least one CPU-second.
const BURST_START: f64 = 0.5;
const BURST_KEEP: f64 = 0.1;
const BURST_QUIET: u32 = 3;
const SPIKE_MIN_US: u64 = 1_000_000;
/// Every this many seconds all processes are read, kernel threads included, and their cgroups
/// re-read; in between only those in cgroups whose CPU total moved.
const FULL_EVERY: u64 = 10;
const NO_CG: u32 = u32::MAX;

/// Who a process runs for. Build and jail pass from parent to child, and build.slice is a build;
/// the rest come from the process's cgroup.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Ctx {
    User,
    System,
    Build,
    Jail,
    Kernel,
    Other,
}

impl Ctx {
    fn label(self) -> &'static str {
        match self {
            Ctx::User => "user",
            Ctx::System => "system",
            Ctx::Build => "build",
            Ctx::Jail => "jail",
            Ctx::Kernel => "kernel",
            Ctx::Other => "other",
        }
    }
}

/// Package label (an index into `labels`) and context.
type Key = (u32, Ctx);

struct Proc {
    /// Start time in clock ticks after boot; 0 until read.
    start: u64,
    key: Key,
    /// Index into the daemon's cgroups, or NO_CG for kernel threads.
    cg: u32,
    last_ticks: u64,
    credited_us: u64,
    /// CPU of this process's exited threads, from the exit reports.
    exited_us: u64,
    hiwater_kb: u64,
    /// Started after the daemon: all of its CPU time is counted, not just what follows a baseline.
    born_here: bool,
    /// Readings in a row that did not find it; finalized at 2, so late exit reports still land.
    missing: u8,
}

struct Burst {
    start: i64,
    secs: i64,
    cpu_us: u64,
    peak_cores: f64,
    rss_peak_kb: u64,
    quiet: u32,
}

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The family of a kernel thread: `kworker/u40:3-events` → `kworker`, `irq/145-iwlwifi` → `irq-iwlwifi`.
fn family(comm: &str) -> String {
    if let Some(rest) = comm.strip_prefix("irq/") {
        return format!("irq-{}", rest.split_once('-').map_or(rest, |(_, n)| n));
    }
    let head = comm.split(['/', ':']).next().unwrap_or(comm);
    head.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-')
        .to_string()
}

fn is_interpreter(exe: &str) -> bool {
    let base = exe.rsplit('/').next().unwrap_or(exe);
    let base = base.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    matches!(
        base,
        "python" | "perl" | "ruby" | "bash" | "sh" | "dash" | "zsh" | "lua" | "luajit" | "node" | "tclsh"
    )
}

struct Daemon {
    map: PkgMap,
    labels: Vec<String>,
    label_ix: HashMap<String, u32>,
    procs: HashMap<i32, Proc>,
    /// Recently finalized processes, so a late exit report doesn't bring them back.
    gone: HashMap<i32, Instant>,
    my_mnt: Option<u64>,
    cgroups: Vec<String>,
    cg_ix: HashMap<String, u32>,
    /// Per cgroup: its CPU total at the last reading, and whether it moved since the one before.
    cg_usage: Vec<u64>,
    cg_moved: Vec<bool>,
    start_ticks: u64,
    us_per_tick: u64,
    /// CPU credited in the current second.
    now_us: HashMap<Key, u64>,
    hourly: HashMap<(i64, Key), Agg>,
    bursts: HashMap<Key, Burst>,
    spikes: Vec<Spike>,
    lost: u64,
}

impl Daemon {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.label_ix.get(s) {
            return i;
        }
        self.labels.push(s.to_string());
        let i = (self.labels.len() - 1) as u32;
        self.label_ix.insert(s.to_string(), i);
        i
    }

    fn cg(&mut self, path: String) -> u32 {
        if let Some(&i) = self.cg_ix.get(&path) {
            return i;
        }
        self.cgroups.push(path.clone());
        self.cg_usage.push(0);
        self.cg_moved.push(true);
        let i = (self.cgroups.len() - 1) as u32;
        self.cg_ix.insert(path, i);
        i
    }

    /// Reads the CPU total of every cgroup a process is in and marks those that moved.
    fn read_cgroups(&mut self) {
        let used: HashSet<u32> = self
            .procs
            .values()
            .map(|p| p.cg)
            .filter(|&c| c != NO_CG)
            .collect();
        for c in used {
            let i = c as usize;
            let usage = procfs::cgroup_usage_us(&self.cgroups[i]);
            self.cg_moved[i] = usage.is_none_or(|u| u != self.cg_usage[i]);
            self.cg_usage[i] = usage.unwrap_or(0);
        }
    }

    fn agg(&mut self, key: Key) -> &mut Agg {
        let hour = unix_now() / 3600 * 3600;
        self.hourly.entry((hour, key)).or_default()
    }

    fn credit(&mut self, key: Key, us: u64) {
        if us > 0 {
            *self.now_us.entry(key).or_default() += us;
        }
    }

    /// The package a process belongs to: its script's for an interpreter running one, else its
    /// program's. Also returns the program path.
    fn package(&self, pid: i32, comm: &str) -> (String, Option<String>) {
        let Some(exe) = procfs::exe(pid) else {
            return (format!("unknown:{comm}"), None);
        };
        // In another mount namespace the path may name a different file (a container's).
        let foreign = self.my_mnt.is_some() && procfs::mnt_ns(pid) != self.my_mnt;
        let same_file = !foreign || procfs::exe_is(pid, &exe);
        let mut label = match (self.map.lookup(&exe), same_file) {
            (Some(cp), true) => cp.to_string(),
            (_, false) => {
                let cg = procfs::cgroup(pid);
                format!("container:{}", cg.rsplit('/').next().unwrap_or(""))
            }
            (None, true) => pkgmap::unowned(&exe),
        };
        if same_file
            && is_interpreter(&exe)
            && let Some(script) = self.script(pid)
        {
            label = script;
        }
        (label, Some(exe))
    }

    /// The first of the first few arguments that is a file: what an interpreter is running.
    fn script(&self, pid: i32) -> Option<String> {
        for a in procfs::cmdline(pid).iter().skip(1).take(4) {
            if a.starts_with('-') {
                continue;
            }
            let path = if a.starts_with('/') {
                format!("/proc/{pid}/root{a}")
            } else {
                format!("/proc/{pid}/cwd/{a}")
            };
            let Ok(real) = std::fs::canonicalize(&path) else {
                continue;
            };
            if !real.is_file() {
                continue;
            }
            let real = real.to_string_lossy();
            return Some(
                self.map
                    .lookup(&real)
                    .map_or_else(|| pkgmap::unowned(&real), str::to_string),
            );
        }
        None
    }

    fn context(&self, pid: i32, label: &str, exe: Option<&str>, cg: &str) -> Ctx {
        if label == "sys-apps/portage" {
            return Ctx::Build;
        }
        if exe.is_some_and(|e| e.ends_with("/bwrap"))
            && procfs::cmdline(pid)
                .windows(2)
                .any(|w| w[0] == "--hostname" && w[1] == "jail")
        {
            return Ctx::Jail;
        }
        // Portage builds run in build.slice (the emerge wrapper, /usr/local/sbin/emerge).
        if cg.starts_with("/build.slice") {
            Ctx::Build
        } else if cg.starts_with("/user.slice") {
            Ctx::User
        } else if cg.starts_with("/system.slice") || cg.starts_with("/init.scope") {
            Ctx::System
        } else {
            Ctx::Other
        }
    }

    /// The process's package and context, and its cgroup.
    fn identify(&mut self, pid: i32, st: &procfs::Stat, inherited: Option<Ctx>) -> (Key, u32) {
        if st.kernel_thread {
            let label = format!("kernel:{}", family(&st.comm));
            return ((self.intern(&label), Ctx::Kernel), NO_CG);
        }
        let (label, exe) = self.package(pid, &st.comm);
        let cg_path = procfs::cgroup(pid);
        let ctx = match inherited {
            Some(c @ (Ctx::Build | Ctx::Jail)) => c,
            _ => self.context(pid, &label, exe.as_deref(), &cg_path),
        };
        ((self.intern(&label), ctx), self.cg(cg_path))
    }

    fn on_fork(&mut self, parent: i32, child: i32) {
        let Some((key, cg)) = self.procs.get(&parent).map(|p| (p.key, p.cg)) else {
            return;
        };
        if let Some(old) = self.procs.remove(&child) {
            self.finalize(child, old);
        }
        self.gone.remove(&child);
        self.procs.insert(
            child,
            Proc {
                start: 0,
                key,
                cg,
                last_ticks: 0,
                credited_us: 0,
                exited_us: 0,
                hiwater_kb: 0,
                born_here: true,
                missing: 0,
            },
        );
    }

    fn on_exec(&mut self, tgid: i32) {
        let Some(st) = procfs::stat(tgid) else { return };
        let inherited = match self.procs.get_mut(&tgid) {
            Some(p) => {
                // What it used before the exec belongs to the program it was.
                let d = st.ticks.saturating_sub(p.last_ticks) * self.us_per_tick;
                p.last_ticks = p.last_ticks.max(st.ticks);
                p.credited_us += d;
                if p.start == 0 {
                    p.start = st.start;
                }
                let key = p.key;
                self.credit(key, d);
                Some(key.1)
            }
            None => {
                let parent = self.procs.get(&st.ppid).map(|p| p.key.1);
                let born_here = st.start >= self.start_ticks;
                let (key, cg) = self.identify(tgid, &st, parent);
                self.procs.insert(
                    tgid,
                    Proc {
                        start: st.start,
                        key,
                        cg,
                        last_ticks: if born_here { 0 } else { st.ticks },
                        credited_us: 0,
                        exited_us: 0,
                        hiwater_kb: 0,
                        born_here,
                        missing: 0,
                    },
                );
                parent
            }
        };
        let (key, cg) = self.identify(tgid, &st, inherited);
        if let Some(p) = self.procs.get_mut(&tgid) {
            p.key = key;
            p.cg = cg;
        }
        self.agg(key).launches += 1;
    }

    fn on_thread_exit(&mut self, r: ThreadExit) {
        if self.gone.contains_key(&r.tgid) {
            return;
        }
        if !self.procs.contains_key(&r.tgid) {
            // Never seen: both its fork and exec events were lost.
            let key = (self.intern(&format!("unknown:{}", r.comm)), Ctx::Other);
            self.procs.insert(
                r.tgid,
                Proc {
                    start: 0,
                    key,
                    cg: NO_CG,
                    last_ticks: 0,
                    credited_us: 0,
                    exited_us: 0,
                    hiwater_kb: 0,
                    born_here: true,
                    missing: 0,
                },
            );
        }
        let p = self.procs.get_mut(&r.tgid).unwrap();
        p.exited_us += r.cpu_us;
        p.hiwater_kb = p.hiwater_kb.max(r.hiwater_kb);
    }

    fn finalize(&mut self, pid: i32, p: Proc) {
        // The exit reports add up its whole life; credit what the readings missed. For a process
        // older than the daemon they include time before the baseline, so they are not used.
        if p.born_here {
            self.credit(p.key, p.exited_us.saturating_sub(p.credited_us));
        }
        if p.hiwater_kb > 0 {
            let a = self.agg(p.key);
            a.rss_peak_kb = a.rss_peak_kb.max(p.hiwater_kb);
        }
        self.gone.insert(pid, Instant::now());
    }

    /// Credits each process's CPU since the last reading. A full sweep reads every process; the
    /// others only those in cgroups whose total moved, and otherwise just check they still exist.
    fn sweep(&mut self, full: bool, first: bool) {
        if !full {
            self.read_cgroups();
        }
        let mut alive = HashSet::new();
        let mut fresh: Vec<(i32, procfs::Stat)> = Vec::new();
        for pid in procfs::pids() {
            let read = match self.procs.get(&pid) {
                None => true,
                Some(p) => full || p.start == 0 || (p.cg != NO_CG && self.cg_moved[p.cg as usize]),
            };
            if !read {
                alive.insert(pid);
                continue;
            }
            let Some(st) = procfs::stat(pid) else { continue };
            alive.insert(pid);
            if self
                .procs
                .get(&pid)
                .is_some_and(|p| p.start != 0 && p.start != st.start)
            {
                let old = self.procs.remove(&pid).unwrap();
                self.finalize(pid, old);
            }
            match self.procs.get_mut(&pid) {
                Some(p) => {
                    if p.start == 0 {
                        p.start = st.start;
                    }
                    if full && p.cg != NO_CG {
                        // systemd moves a launched app into its own scope after the fork.
                        let path = procfs::cgroup(pid);
                        let cg = self.cg(path);
                        self.procs.get_mut(&pid).unwrap().cg = cg;
                    }
                    self.read_cpu(pid, &st);
                }
                None => fresh.push((pid, st)),
            }
        }
        if first {
            // Parents first, so children inherit build and jail.
            fresh.sort_by_key(|(_, s)| s.start);
        }
        for (pid, st) in fresh {
            let born_here = st.start >= self.start_ticks;
            let parent = self.procs.get(&st.ppid).map(|p| p.key.1);
            let (key, cg) = self.identify(pid, &st, parent);
            self.procs.insert(
                pid,
                Proc {
                    start: st.start,
                    key,
                    cg,
                    last_ticks: if born_here { 0 } else { st.ticks },
                    credited_us: 0,
                    exited_us: 0,
                    hiwater_kb: 0,
                    born_here,
                    missing: 0,
                },
            );
            self.read_cpu(pid, &st);
        }
        let mut dead = Vec::new();
        for (pid, p) in self.procs.iter_mut() {
            if alive.contains(pid) {
                p.missing = 0;
            } else {
                p.missing += 1;
                if p.missing >= 2 {
                    dead.push(*pid);
                }
            }
        }
        for pid in dead {
            let p = self.procs.remove(&pid).unwrap();
            self.finalize(pid, p);
        }
        self.gone.retain(|_, t| t.elapsed() < Duration::from_secs(10));
    }

    fn read_cpu(&mut self, pid: i32, st: &procfs::Stat) {
        let p = self.procs.get_mut(&pid).unwrap();
        let d = st.ticks.saturating_sub(p.last_ticks) * self.us_per_tick;
        p.last_ticks = p.last_ticks.max(st.ticks);
        p.credited_us += d;
        let key = p.key;
        self.credit(key, d);
    }

    fn end_tick(&mut self, elapsed_us: u64) {
        let alive: HashSet<Key> = self
            .procs
            .values()
            .filter(|p| p.missing == 0)
            .map(|p| p.key)
            .collect();
        for k in alive {
            self.agg(k).alive_s += 1;
        }
        let used: Vec<(Key, u64)> = self.now_us.drain().collect();
        let mut busy = HashSet::new();
        for (key, us) in used {
            let cores = us as f64 / elapsed_us.max(1) as f64;
            let bucket = match cores {
                c if c <= 0.05 => 0,
                c if c <= 0.25 => 1,
                c if c <= 1.0 => 2,
                c if c <= 2.0 => 3,
                _ => 4,
            };
            let a = self.agg(key);
            a.cpu_us += us;
            a.secs[bucket] += 1;
            self.burst(key, cores, us);
            busy.insert(key);
        }
        let quiet: Vec<Key> = self
            .bursts
            .keys()
            .filter(|k| !busy.contains(k))
            .copied()
            .collect();
        for key in quiet {
            self.burst(key, 0.0, 0);
        }
    }

    fn burst(&mut self, key: Key, cores: f64, us: u64) {
        match self.bursts.get_mut(&key) {
            Some(b) => {
                b.secs += 1;
                b.cpu_us += us;
                b.peak_cores = b.peak_cores.max(cores);
                b.quiet = if cores < BURST_KEEP { b.quiet + 1 } else { 0 };
                if b.quiet >= BURST_QUIET {
                    let b = self.bursts.remove(&key).unwrap();
                    if b.cpu_us >= SPIKE_MIN_US {
                        self.spikes.push(Spike {
                            start: b.start,
                            secs: b.secs - b.quiet as i64,
                            pkg: self.labels[key.0 as usize].clone(),
                            ctx: key.1.label(),
                            cpu_us: b.cpu_us,
                            peak_cores: b.peak_cores,
                            rss_peak_kb: b.rss_peak_kb,
                        });
                    }
                }
            }
            None if cores >= BURST_START => {
                self.bursts.insert(
                    key,
                    Burst {
                        start: unix_now(),
                        secs: 1,
                        cpu_us: us,
                        peak_cores: cores,
                        rss_peak_kb: 0,
                        quiet: 0,
                    },
                );
            }
            None => {}
        }
    }

    /// Peak memory of each process over the last interval, summed per package.
    fn peaks(&mut self) {
        let mut sums: HashMap<Key, u64> = HashMap::new();
        for (&pid, p) in &self.procs {
            if p.key.1 == Ctx::Kernel || p.missing > 0 {
                continue;
            }
            if let Some(kb) = procfs::peak_rss_kb(pid) {
                *sums.entry(p.key).or_default() += kb;
                procfs::reset_peak(pid);
            }
        }
        for (key, kb) in sums {
            let a = self.agg(key);
            a.rss_peak_kb = a.rss_peak_kb.max(kb);
            if let Some(b) = self.bursts.get_mut(&key) {
                b.rss_peak_kb = b.rss_peak_kb.max(kb);
            }
        }
    }

    fn pss(&mut self) {
        let mut sums: HashMap<Key, u64> = HashMap::new();
        for (&pid, p) in &self.procs {
            if p.key.1 == Ctx::Kernel || p.missing > 0 {
                continue;
            }
            if let Some(kb) = procfs::pss_kb(pid) {
                *sums.entry(p.key).or_default() += kb;
            }
        }
        for (key, kb) in sums {
            let a = self.agg(key);
            a.pss_kb_sum += kb;
            a.pss_n += 1;
        }
    }

    fn flush(&mut self, store: &mut Store) {
        let rows = self
            .hourly
            .iter()
            .map(|((h, (pkg, ctx)), a)| (*h, self.labels[*pkg as usize].as_str(), ctx.label(), a));
        match store.flush(rows, &self.spikes) {
            Ok(()) => {
                self.hourly.clear();
                self.spikes.clear();
            }
            Err(e) => eprintln!("deck-usaged: writing the database failed, keeping the totals: {e:#}"),
        }
        let _ = store.meta("lost_event_batches", &self.lost.to_string());
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut db = PathBuf::from(DEFAULT_DB);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--db" => db = args.next().map(PathBuf::from).unwrap_or(db),
            "-h" | "--help" => {
                println!("usage: deck-usaged [--db PATH]   (default {DEFAULT_DB}; run as root)");
                return Ok(());
            }
            _ => anyhow::bail!("unknown argument {a}"),
        }
    }
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }

    let me = std::process::id() as i32;
    let start_ticks = procfs::stat(me).map_or(0, |s| s.start);
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
    let cpus = std::fs::read_to_string("/sys/devices/system/cpu/possible").unwrap_or_else(|_| "0".into());

    // Without CAP_NET_ADMIN both fail: processes shorter than a second then go uncounted.
    let mut pc = ProcConnector::open()
        .map_err(|e| eprintln!("deck-usaged: no process events: {e}"))
        .ok();
    let mut ts = Taskstats::open(cpus.trim())
        .map_err(|e| eprintln!("deck-usaged: no exit reports: {e}"))
        .ok();

    let t = Instant::now();
    let map = PkgMap::build();
    eprintln!(
        "deck-usaged: {} installed files mapped in {:.1?}",
        map.len(),
        t.elapsed()
    );

    let mut store = Store::open(&db)?;
    store.meta("started", &unix_now().to_string())?;

    let mut d = Daemon {
        map,
        labels: Vec::new(),
        label_ix: HashMap::new(),
        procs: HashMap::new(),
        gone: HashMap::new(),
        my_mnt: procfs::mnt_ns(me),
        cgroups: Vec::new(),
        cg_ix: HashMap::new(),
        cg_usage: Vec::new(),
        cg_moved: Vec::new(),
        start_ticks,
        us_per_tick: 1_000_000 / hz,
        now_us: HashMap::new(),
        hourly: HashMap::new(),
        bursts: HashMap::new(),
        spikes: Vec::new(),
        lost: 0,
    };
    d.sweep(true, true);
    d.read_cgroups();
    d.now_us.clear();
    eprintln!(
        "deck-usaged: tracking {} processes, writing {}",
        d.procs.len(),
        db.display()
    );

    let mut events = Vec::new();
    let mut exits = Vec::new();
    let mut last_tick = Instant::now();
    let mut next_tick = last_tick + TICK;
    let mut n: u64 = 0;
    while !STOP.load(Ordering::Relaxed) {
        let mut fds: Vec<libc::pollfd> = [pc.as_ref().map(|p| p.fd()), ts.as_ref().map(|t| t.fd())]
            .into_iter()
            .flatten()
            .map(|fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let wait = next_tick.saturating_duration_since(Instant::now()).as_millis() as i32;
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, wait) };

        if let Some(p) = pc.as_mut() {
            match p.drain(&mut events) {
                Ok(lost) => d.lost += lost,
                Err(e) => eprintln!("deck-usaged: process events: {e}"),
            }
        }
        if let Some(t) = ts.as_mut() {
            match t.drain(&mut exits) {
                Ok(lost) => d.lost += lost,
                Err(e) => eprintln!("deck-usaged: exit reports: {e}"),
            }
        }
        for e in events.drain(..) {
            match e {
                ProcEvent::Fork { parent, child } => d.on_fork(parent, child),
                ProcEvent::Exec { tgid } => d.on_exec(tgid),
            }
        }
        for r in exits.drain(..) {
            d.on_thread_exit(r);
        }

        let now = Instant::now();
        if now >= next_tick {
            d.sweep(n % FULL_EVERY == FULL_EVERY - 1, false);
            d.end_tick((now - last_tick).as_micros() as u64);
            last_tick = now;
            n += 1;
            if n.is_multiple_of(PEAK_EVERY) {
                d.peaks();
            }
            if n.is_multiple_of(PSS_EVERY) {
                d.pss();
            }
            if n.is_multiple_of(FLUSH_EVERY) {
                d.flush(&mut store);
                if d.map.stale() {
                    d.map = PkgMap::build();
                    eprintln!(
                        "deck-usaged: package database changed, {} files mapped",
                        d.map.len()
                    );
                }
            }
            next_tick += TICK;
            if next_tick < now {
                next_tick = now + TICK;
            }
        }
    }
    d.flush(&mut store);
    eprintln!("deck-usaged: stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_kernel_thread_families() {
        assert_eq!(family("kworker/u40:3-events_unbound"), "kworker");
        assert_eq!(family("ksoftirqd/3"), "ksoftirqd");
        assert_eq!(family("irq/145-iwlwifi"), "irq-iwlwifi");
        assert_eq!(family("btrfs-endio-write"), "btrfs-endio-write");
    }

    #[test]
    fn knows_interpreters() {
        assert!(is_interpreter("/usr/bin/python3.14"));
        assert!(is_interpreter("/usr/bin/bash"));
        assert!(is_interpreter("/usr/bin/perl5.42.0"));
        assert!(!is_interpreter("/usr/bin/nvim"));
    }
}
