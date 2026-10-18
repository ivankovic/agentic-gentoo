//! deck-usaged: CPU and memory use per package, hour by hour, and the spikes in between.
//!
//! Every second it reads each process's CPU counter from /proc and credits the increase to the
//! package that owns the program (or the script, for interpreters). The kernel's process events
//! name programs as they start, and its exit reports give the CPU time of processes too short
//! for a reading. Every 10 s it reads and resets the peak memory of each process that ran, every
//! 5 min each one's PSS. Totals go to SQLite each minute; `deck usage` reads them.
//!
//! It keeps each process's stat file and each cgroup's cpu.stat open, and reads them again with
//! one pread: opening a path in /proc for every process every second was most of its own cost.

mod netlink;
mod profile;
mod store;

use anyhow::Result;
use deck::pkgmap::{self, PkgMap, is_interpreter};
use deck::procfs::{self, CpuStat, StatFile};
use netlink::{ProcConnector, ProcEvent, Taskstats, ThreadExit};
use profile::Profiler;
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
/// Every this many seconds all processes are read, kernel threads included, and /proc listed for
/// any the kernel's events missed; in between only those in cgroups whose CPU total moved.
const FULL_EVERY: u64 = 10;
/// For this many seconds after a process is first seen, full readings read its cgroup again:
/// systemd moves a launched app into a scope of its own just after it starts.
const YOUNG: u64 = 60;
/// Labels are names any process can choose (a program's file name, its comm): the daemon keeps at
/// most this many, each at most this long, so a loop of random names can't grow it without end.
const MAX_LABELS: usize = 20_000;
const MAX_LABEL: usize = 200;
/// Where labels past MAX_LABELS go; the first label, index 0.
const TOO_MANY: &str = "other:too-many-labels";
/// /proc/<pid>/stat is under 1 KiB; cpu.stat a few hundred bytes.
const READ_BUF: usize = 4096;
const NO_CG: u32 = u32::MAX;
/// Profiler samples per second per CPU: a prime, so it doesn't beat with periodic work.
const PROFILE_HZ: u64 = 49;
/// A mapping of anonymous executable memory (JIT): credited to the process's own package.
const SELF: u32 = u32::MAX;

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

/// A process's executable mappings: start, end, package label (or SELF).
type ExecMaps = Vec<(u64, u64, u32)>;

/// Package label (an index into `labels`) and context.
type Key = (u32, Ctx);

struct Proc {
    /// Its stat file, open since it was first read.
    stat: Option<StatFile>,
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
    /// The reading (`Daemon::reading`) that last found it.
    reading: u64,
    /// The tick it was first seen.
    seen_tick: u64,
    /// To read at the next reading whatever its cgroup did: an exit report says it may have ended,
    /// or the last reading didn't find it.
    check: bool,
    /// `credited_us` at the last peak memory reading (MAX before the first), and its resident
    /// memory then: a process that used no CPU since can't have grown, so it isn't read again.
    peak_credited: u64,
    idle_kb: u64,
}

impl Proc {
    fn new(start: u64, key: Key, cg: u32, last_ticks: u64, born_here: bool, tick: u64) -> Proc {
        Proc {
            stat: None,
            start,
            key,
            cg,
            last_ticks,
            credited_us: 0,
            exited_us: 0,
            hiwater_kb: 0,
            born_here,
            missing: 0,
            reading: 0,
            seen_tick: tick,
            check: false,
            peak_credited: u64::MAX,
            idle_kb: 0,
        }
    }
}

/// What a reading found of a process it knew.
enum Read {
    Alive,
    Gone,
    /// Its PID now names another process.
    Replaced,
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

struct Daemon {
    map: PkgMap,
    labels: Vec<String>,
    label_ix: HashMap<String, u32>,
    procs: HashMap<i32, Proc>,
    /// Recently finalized processes, so a late exit report doesn't bring them back.
    gone: HashMap<i32, Instant>,
    host_mnt: Option<u64>,
    cgroups: Vec<String>,
    cg_ix: HashMap<String, u32>,
    /// Per cgroup: its cpu.stat, its CPU total at the last reading, whether it moved since the one
    /// before, and whether it is gone (its processes moved on).
    cpustat: Vec<Option<CpuStat>>,
    cg_usage: Vec<u64>,
    cg_moved: Vec<bool>,
    cg_gone: Vec<bool>,
    start_ticks: u64,
    us_per_tick: u64,
    /// CPU credited in the current second.
    now_us: HashMap<Key, u64>,
    hourly: HashMap<(i64, Key), Agg>,
    bursts: HashMap<Key, Burst>,
    spikes: Vec<Spike>,
    lost: u64,
    /// Kernel modules' address ranges and labels (kmod:<name>), for samples in the kernel.
    modules: Vec<(u64, u64, u32)>,
    /// Per process: its executable mappings with their package labels, and when they were read.
    maps: HashMap<i32, (ExecMaps, Instant)>,
    /// Samples per hour, per package whose code ran (library or program), per process key.
    libs: HashMap<(i64, u32, Key), u64>,
    unresolved: u64,
    jit_guess: u64,
    /// Readings so far, and seconds so far.
    reading: u64,
    tick: u64,
    /// What the database's meta table last got, so unchanged values aren't written again.
    meta: HashMap<&'static str, String>,
    /// The package database's stamp when it was last seen changed, and minutes it stayed so:
    /// the map is rebuilt once a merge or an emerge is over, not after each package.
    map_pending: Option<Option<std::time::SystemTime>>,
    map_quiet: u32,
    // Kept between ticks so they aren't allocated each second.
    buf: Vec<u8>,
    pid_buf: Vec<i32>,
    fresh: Vec<(i32, procfs::Stat)>,
    cg_used: Vec<bool>,
    alive_keys: HashSet<Key>,
    busy: HashSet<Key>,
    used: Vec<(Key, u64)>,
}

/// A label as kept: control characters out (they would reach a terminal from `deck usage`), and
/// no longer than MAX_LABEL.
fn clean_label(s: &str) -> String {
    s.chars()
        .take(MAX_LABEL)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

impl Daemon {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.label_ix.get(s) {
            return i;
        }
        let clean = clean_label(s);
        if let Some(&i) = self.label_ix.get(&clean) {
            return i;
        }
        if self.labels.len() >= MAX_LABELS {
            return 0;
        }
        self.labels.push(clean.clone());
        let i = (self.labels.len() - 1) as u32;
        self.label_ix.insert(clean, i);
        i
    }

    fn cg(&mut self, path: String) -> u32 {
        if let Some(&i) = self.cg_ix.get(&path) {
            return i;
        }
        self.cgroups.push(path.clone());
        self.cpustat.push(None);
        self.cg_usage.push(0);
        self.cg_moved.push(true);
        self.cg_gone.push(false);
        let i = (self.cgroups.len() - 1) as u32;
        self.cg_ix.insert(path, i);
        i
    }

    /// Drops the cgroups no process is in any more: each app launched gets a scope of its own.
    fn compact_cgroups(&mut self) {
        let mut used = vec![false; self.cgroups.len()];
        for p in self.procs.values().filter(|p| p.cg != NO_CG) {
            used[p.cg as usize] = true;
        }
        if used.iter().all(|&u| u) {
            return;
        }
        let mut remap = vec![NO_CG; used.len()];
        let cgroups = std::mem::take(&mut self.cgroups);
        let cpustat = std::mem::take(&mut self.cpustat);
        let usage = std::mem::take(&mut self.cg_usage);
        let moved = std::mem::take(&mut self.cg_moved);
        let gone = std::mem::take(&mut self.cg_gone);
        self.cg_ix.clear();
        for (i, (((path, f), u), (m, g))) in cgroups
            .into_iter()
            .zip(cpustat)
            .zip(usage)
            .zip(moved.into_iter().zip(gone))
            .enumerate()
        {
            if used[i] {
                remap[i] = self.cgroups.len() as u32;
                self.cg_ix.insert(path.clone(), remap[i]);
                self.cgroups.push(path);
                self.cpustat.push(f);
                self.cg_usage.push(u);
                self.cg_moved.push(m);
                self.cg_gone.push(g);
            }
        }
        for p in self.procs.values_mut().filter(|p| p.cg != NO_CG) {
            p.cg = remap[p.cg as usize];
        }
    }

    /// Reads the CPU total of every cgroup a process is in and marks those that moved.
    fn read_cgroups(&mut self) {
        self.cg_used.clear();
        self.cg_used.resize(self.cgroups.len(), false);
        for p in self.procs.values().filter(|p| p.cg != NO_CG) {
            self.cg_used[p.cg as usize] = true;
        }
        for i in 0..self.cgroups.len() {
            if !self.cg_used[i] || self.cg_gone[i] {
                continue;
            }
            if self.cpustat[i].is_none() {
                self.cpustat[i] = CpuStat::open(&self.cgroups[i]);
            }
            let usage = self.cpustat[i].as_ref().and_then(|f| f.usage_us(&mut self.buf));
            if usage.is_none() {
                // Removed: its processes are elsewhere now, or gone. The next full reading
                // reads where they are.
                self.cpustat[i] = None;
                self.cg_gone[i] = true;
            }
            self.cg_moved[i] = usage.is_none_or(|u| u != self.cg_usage[i]);
            self.cg_usage[i] = usage.unwrap_or(0);
        }
    }

    /// Kernel modules again: one may have been loaded.
    fn load_modules(&mut self) {
        let mods = profile::modules();
        self.modules = mods
            .into_iter()
            .map(|(a, b, name)| (a, b, self.intern(&format!("kmod:{name}"))))
            .collect();
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
        let Some((exe, deleted)) = procfs::exe_deleted(pid) else {
            return (format!("unknown:{comm}"), None);
        };
        // In another mount namespace than the host's (PID 1's; the daemon has a private one) the
        // path may name a different file: a container's. A program updated since it started has
        // no file to compare and is taken by its path.
        let foreign = self.host_mnt.is_some() && procfs::mnt_ns(pid) != self.host_mnt;
        let same_file = !foreign || deleted || procfs::exe_is(pid, &exe);
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

    /// The package of the script an interpreter runs, if it runs one.
    fn script(&self, pid: i32) -> Option<String> {
        let real = procfs::script_path(pid)?;
        Some(
            self.map
                .lookup(&real)
                .map_or_else(|| pkgmap::unowned(&real), str::to_string),
        )
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
        // A kernel thread is named after it starts and never execs: the next reading names it.
        if key.1 == Ctx::Kernel {
            return;
        }
        if let Some(old) = self.procs.remove(&child) {
            self.finalize(child, old);
        }
        self.gone.remove(&child);
        self.procs
            .insert(child, Proc::new(0, key, cg, 0, true, self.tick));
    }

    fn on_exec(&mut self, tgid: i32) {
        let Some(st) = procfs::stat(tgid) else { return };
        // Another program: other mappings.
        self.maps.remove(&tgid);
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
                // Its fork was missed: named here, once.
                let parent = self.procs.get(&st.ppid).map(|p| p.key.1);
                let born_here = st.start >= self.start_ticks;
                let (key, cg) = self.identify(tgid, &st, parent);
                let last = if born_here { 0 } else { st.ticks };
                let mut p = Proc::new(st.start, key, cg, last, born_here, self.tick);
                p.stat = StatFile::open(tgid);
                self.procs.insert(tgid, p);
                self.agg(key).launches += 1;
                return;
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
            self.procs
                .insert(r.tgid, Proc::new(0, key, NO_CG, 0, true, self.tick));
        }
        let p = self.procs.get_mut(&r.tgid).unwrap();
        p.exited_us += r.cpu_us;
        p.hiwater_kb = p.hiwater_kb.max(r.hiwater_kb);
        p.check |= r.last;
    }

    /// The package whose code is at `ip` in process `pid`; SELF for JIT code.
    fn code_at(&mut self, pid: i32, ip: u64) -> Option<u32> {
        let find = |m: &ExecMaps| {
            let i = m.partition_point(|r| r.1 <= ip);
            m.get(i).filter(|r| r.0 <= ip).map(|r| r.2)
        };
        if let Some((m, _)) = self.maps.get(&pid)
            && let Some(l) = find(m)
        {
            return Some(l);
        }
        // Not mapped when last read: read again, at most every 5 min per process (and after an
        // exec). Misses are nearly all JIT code, counted as the process's own anyway, and
        // Firefox's maps are long: reading them every second for each of its processes cost more
        // than the rest of the daemon, and every 30 s still most of what the profile cost.
        if self
            .maps
            .get(&pid)
            .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(300))
        {
            return None;
        }
        let mut m = Vec::new();
        for (a, b, path) in profile::exec_maps(pid) {
            let label = if path.is_empty() || path.starts_with("[anon") || path.starts_with("/memfd:") {
                SELF
            } else if path.starts_with('[') {
                // [vdso], [vsyscall]: the kernel's code in user space.
                self.intern("kernel")
            } else {
                let l = self
                    .map
                    .lookup(&path)
                    .map_or_else(|| pkgmap::unowned(&path), str::to_string);
                self.intern(&l)
            };
            m.push((a, b, label));
        }
        let found = find(&m);
        self.maps.insert(pid, (m, Instant::now()));
        found
    }

    fn profile(&mut self, samples: &mut Vec<profile::Sample>) {
        let hour = unix_now() / 3600 * 3600;
        for s in samples.drain(..) {
            // PID 0 is the kernel's idle loop, before a CPU sleeps (or while it polls).
            let key = match self.procs.get(&s.pid) {
                Some(p) => p.key,
                None if s.pid == 0 => (self.intern("kernel:idle"), Ctx::Kernel),
                None => {
                    self.unresolved += 1;
                    continue;
                }
            };
            let lib = if s.pid == 0 {
                key.0
            } else if s.kernel {
                let i = self.modules.partition_point(|m| m.1 <= s.ip);
                match self.modules.get(i).filter(|m| m.0 <= s.ip) {
                    Some(m) => m.2,
                    None => self.intern("kernel"),
                }
            } else {
                match self.code_at(s.pid, s.ip) {
                    Some(SELF) => key.0,
                    Some(l) => l,
                    // Mapped after the last reading of the maps: nearly always fresh JIT code
                    // (Firefox, Bun), so the process's own; counted apart.
                    None => {
                        self.jit_guess += 1;
                        key.0
                    }
                }
            };
            *self.libs.entry((hour, lib, key)).or_default() += 1;
        }
    }

    fn finalize(&mut self, pid: i32, p: Proc) {
        self.maps.remove(&pid);
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

    /// Credits each process's CPU since the last reading. A full sweep lists /proc and reads
    /// every process; the others only those in cgroups whose total moved, those not read yet and
    /// those an exit report named, and take the rest to be still there.
    fn sweep(&mut self, full: bool, first: bool) {
        if !full {
            self.read_cgroups();
        }
        self.reading += 1;
        let mut pids = std::mem::take(&mut self.pid_buf);
        let mut fresh = std::mem::take(&mut self.fresh);
        fresh.clear();
        if full {
            procfs::pids(&mut pids);
        } else {
            pids.clear();
            for (&pid, p) in self.procs.iter_mut() {
                if p.start == 0 || p.check || (p.cg != NO_CG && self.cg_moved[p.cg as usize]) {
                    pids.push(pid);
                } else {
                    p.reading = self.reading;
                }
            }
        }
        for &pid in &pids {
            let read = if self.procs.contains_key(&pid) {
                self.read_proc(pid, full)
            } else if full {
                Read::Replaced
            } else {
                Read::Gone
            };
            // Only when a process is there to replace it: one that just ended keeps its grace.
            if let Read::Replaced = read
                && let Some(st) = procfs::stat(pid)
            {
                if let Some(old) = self.procs.remove(&pid) {
                    self.finalize(pid, old);
                }
                self.gone.remove(&pid);
                fresh.push((pid, st));
            }
        }
        self.pid_buf = pids;
        if first {
            // Parents first, so children inherit build and jail.
            fresh.sort_by_key(|(_, s)| s.start);
        }
        for (pid, st) in fresh.drain(..) {
            // Opened after the stat was read: the PID must still be that process.
            let Some(f) = StatFile::open(pid) else { continue };
            let Some((ticks, start)) = f.times(&mut self.buf) else { continue };
            if start != st.start {
                continue;
            }
            let born_here = st.start >= self.start_ticks;
            let parent = self.procs.get(&st.ppid).map(|p| p.key.1);
            let (key, cg) = self.identify(pid, &st, parent);
            let last = if born_here { 0 } else { st.ticks };
            let mut p = Proc::new(st.start, key, cg, last, born_here, self.tick);
            p.stat = Some(f);
            p.reading = self.reading;
            self.procs.insert(pid, p);
            self.read_cpu(pid, ticks);
        }
        self.fresh = fresh;
        let mut dead = std::mem::take(&mut self.pid_buf);
        dead.clear();
        for (pid, p) in self.procs.iter_mut() {
            if p.reading == self.reading {
                p.missing = 0;
            } else {
                p.missing += 1;
                p.check = true;
                if p.missing >= 2 {
                    dead.push(*pid);
                }
            }
        }
        for &pid in &dead {
            let p = self.procs.remove(&pid).unwrap();
            self.finalize(pid, p);
        }
        self.pid_buf = dead;
        self.gone.retain(|_, t| t.elapsed() < Duration::from_secs(10));
    }

    /// Reads a known process through its stat file, and credits its CPU.
    fn read_proc(&mut self, pid: i32, full: bool) -> Read {
        let p = self.procs.get_mut(&pid).unwrap();
        if p.stat.is_none() {
            // Known from its fork event, not read yet.
            p.stat = StatFile::open(pid);
        }
        let Some((ticks, start)) = p.stat.as_ref().and_then(|f| f.times(&mut self.buf)) else {
            // Gone; or, listed in /proc all the same, its PID taken by another.
            return if full && p.stat.is_some() {
                Read::Replaced
            } else {
                Read::Gone
            };
        };
        if p.start != 0 && p.start != start {
            return Read::Replaced;
        }
        p.start = start;
        p.reading = self.reading;
        p.check = false;
        let recheck = full
            && p.cg != NO_CG
            && (self.tick.saturating_sub(p.seen_tick) <= YOUNG || self.cg_gone[p.cg as usize]);
        if recheck {
            let path = procfs::cgroup(pid);
            let cg = self.cg(path);
            self.procs.get_mut(&pid).unwrap().cg = cg;
        }
        self.read_cpu(pid, ticks);
        Read::Alive
    }

    fn read_cpu(&mut self, pid: i32, ticks: u64) {
        let p = self.procs.get_mut(&pid).unwrap();
        let d = ticks.saturating_sub(p.last_ticks) * self.us_per_tick;
        p.last_ticks = p.last_ticks.max(ticks);
        p.credited_us += d;
        let key = p.key;
        self.credit(key, d);
    }

    fn end_tick(&mut self, elapsed_us: u64) {
        let mut alive = std::mem::take(&mut self.alive_keys);
        alive.clear();
        alive.extend(self.procs.values().filter(|p| p.missing == 0).map(|p| p.key));
        for &k in &alive {
            self.agg(k).alive_s += 1;
        }
        self.alive_keys = alive;
        let mut used = std::mem::take(&mut self.used);
        used.clear();
        used.extend(self.now_us.drain());
        let mut busy = std::mem::take(&mut self.busy);
        busy.clear();
        for &(key, us) in &used {
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
        self.used = used;
        self.busy = busy;
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

    /// Peak memory of each process over the last interval, summed per package. Only processes
    /// that ran since the last reading are read and reset: resetting takes the process's memory
    /// map lock. The others count what they held when they were last read.
    fn peaks(&mut self) {
        let mut sums: HashMap<Key, u64> = HashMap::new();
        for (&pid, p) in self.procs.iter_mut() {
            if p.key.1 == Ctx::Kernel || p.missing > 0 {
                continue;
            }
            if p.credited_us == p.peak_credited {
                *sums.entry(p.key).or_default() += p.idle_kb;
                continue;
            }
            if let Some((peak, now)) = procfs::peak_rss_kb(pid) {
                *sums.entry(p.key).or_default() += peak;
                procfs::reset_peak(pid);
                p.peak_credited = p.credited_us;
                p.idle_kb = now;
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

    /// The meta values that changed since they were last written.
    fn meta_changes(&mut self, prof_lost: Option<u64>) -> Vec<(&'static str, String)> {
        let mut now = vec![
            ("lost_event_batches", self.lost.to_string()),
            ("profile_unresolved_samples", self.unresolved.to_string()),
            ("profile_unmapped_as_own_samples", self.jit_guess.to_string()),
        ];
        if let Some(l) = prof_lost {
            now.push(("profile_lost_samples", l.to_string()));
        }
        now.retain(|(k, v)| self.meta.get(k) != Some(v));
        now
    }

    fn flush(&mut self, store: &mut Store, prof_lost: Option<u64>) {
        let meta = self.meta_changes(prof_lost);
        let rows = self
            .hourly
            .iter()
            .map(|((h, (pkg, ctx)), a)| (*h, self.labels[*pkg as usize].as_str(), ctx.label(), a));
        let libs = self.libs.iter().map(|((h, lib, (prog, ctx)), n)| {
            (
                *h,
                self.labels[*lib as usize].as_str(),
                self.labels[*prog as usize].as_str(),
                ctx.label(),
                *n,
            )
        });
        // One transaction a minute, meta values with the totals: each commit syncs the journal.
        match store.flush(rows, libs, &self.spikes, &meta) {
            Ok(()) => {
                self.hourly.clear();
                self.libs.clear();
                self.spikes.clear();
                self.meta.extend(meta);
            }
            Err(e) => eprintln!("deck-usaged: writing the database failed, keeping the totals: {e:#}"),
        }
        self.compact_cgroups();
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

    // A descriptor for each process's stat file, and each cgroup's: the default soft limit is 1024.
    unsafe {
        let mut l: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut l) == 0 {
            l.rlim_cur = l.rlim_max;
            libc::setrlimit(libc::RLIMIT_NOFILE, &l);
        }
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

    let mut prof = match Profiler::open(PROFILE_HZ) {
        Ok(p) => {
            eprintln!("deck-usaged: profiling {} CPUs at {PROFILE_HZ} Hz", p.cpus());
            Some(p)
        }
        Err(e) => {
            eprintln!("deck-usaged: no profiler: {e}");
            None
        }
    };
    let mut samples = Vec::new();

    let t = Instant::now();
    let map = PkgMap::build();
    eprintln!(
        "deck-usaged: {} installed files mapped in {:.1?}",
        map.len(),
        t.elapsed()
    );

    let mut store = Store::open(&db)?;
    store.meta("started", &unix_now().to_string())?;
    store.meta("profile_hz", &prof.as_ref().map_or(0, |p| p.freq).to_string())?;

    let mut d = Daemon {
        map,
        labels: vec![TOO_MANY.to_string()],
        label_ix: HashMap::from([(TOO_MANY.to_string(), 0)]),
        procs: HashMap::new(),
        gone: HashMap::new(),
        host_mnt: procfs::mnt_ns(1),
        cgroups: Vec::new(),
        cg_ix: HashMap::new(),
        cpustat: Vec::new(),
        cg_usage: Vec::new(),
        cg_moved: Vec::new(),
        cg_gone: Vec::new(),
        start_ticks,
        us_per_tick: 1_000_000 / hz,
        now_us: HashMap::new(),
        hourly: HashMap::new(),
        bursts: HashMap::new(),
        spikes: Vec::new(),
        lost: 0,
        modules: Vec::new(),
        maps: HashMap::new(),
        libs: HashMap::new(),
        unresolved: 0,
        jit_guess: 0,
        reading: 0,
        tick: 0,
        meta: HashMap::new(),
        map_pending: None,
        map_quiet: 0,
        buf: vec![0; READ_BUF],
        pid_buf: Vec::new(),
        fresh: Vec::new(),
        cg_used: Vec::new(),
        alive_keys: HashSet::new(),
        busy: HashSet::new(),
        used: Vec::new(),
    };
    d.load_modules();
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
    // Only the process events wake the loop: an exec is named while the process is still there.
    // Exit reports wait in their socket's buffer (16 MiB, far more than a second of them) for the
    // tick; the readings already allow for reports that come late.
    let mut fds: Vec<libc::pollfd> = pc
        .as_ref()
        .map(|p| libc::pollfd {
            fd: p.fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .into_iter()
        .collect();
    while !STOP.load(Ordering::Relaxed) {
        let wait = next_tick.saturating_duration_since(Instant::now()).as_millis() as i32;
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, wait) };

        if let Some(p) = pc.as_mut() {
            match p.drain(&mut events) {
                Ok(lost) => d.lost += lost,
                Err(e) => eprintln!("deck-usaged: process events: {e}"),
            }
        }
        for e in events.drain(..) {
            match e {
                ProcEvent::Fork { parent, child } => d.on_fork(parent, child),
                ProcEvent::Exec { tgid } => d.on_exec(tgid),
            }
        }

        let now = Instant::now();
        if now >= next_tick {
            if let Some(t) = ts.as_mut() {
                match t.drain(&mut exits) {
                    Ok(lost) => d.lost += lost,
                    Err(e) => eprintln!("deck-usaged: exit reports: {e}"),
                }
            }
            for r in exits.drain(..) {
                d.on_thread_exit(r);
            }
            d.tick = n;
            d.sweep(n % FULL_EVERY == FULL_EVERY - 1, false);
            d.end_tick((now - last_tick).as_micros() as u64);
            if let Some(p) = prof.as_mut() {
                p.drain(&mut samples);
                d.profile(&mut samples);
            }
            last_tick = now;
            n += 1;
            if n.is_multiple_of(PEAK_EVERY) {
                d.peaks();
            }
            if n.is_multiple_of(PSS_EVERY) {
                d.pss();
            }
            if n.is_multiple_of(FLUSH_EVERY) {
                d.flush(&mut store, prof.as_ref().map(|p| p.lost));
                d.load_modules();
                // Rebuilt once the package database stayed the same for two minutes: an emerge
                // changes it after each package it merges.
                let stamp = pkgmap::stamp();
                if stamp != d.map.stamp {
                    if d.map_pending == Some(stamp) {
                        d.map_quiet += 1;
                    } else {
                        d.map_pending = Some(stamp);
                        d.map_quiet = 0;
                    }
                    if d.map_quiet >= 2 {
                        let old = std::mem::take(&mut d.map);
                        d.map = old.rebuilt();
                        d.map_pending = None;
                        eprintln!(
                            "deck-usaged: package database changed, {} files mapped",
                            d.map.len()
                        );
                    }
                }
            }
            next_tick += TICK;
            if next_tick < now {
                next_tick = now + TICK;
            }
        }
    }
    d.flush(&mut store, prof.as_ref().map(|p| p.lost));
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
