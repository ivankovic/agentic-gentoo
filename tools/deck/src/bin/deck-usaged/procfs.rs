//! What the daemon reads from /proc.

use std::fs;
use std::os::unix::fs::MetadataExt;

pub struct Stat {
    pub comm: String,
    pub ppid: i32,
    pub kernel_thread: bool,
    /// User plus system time of all threads, live and exited, in clock ticks.
    pub ticks: u64,
    /// Start time after boot, in clock ticks: with the PID, it names a process uniquely.
    pub start: u64,
}

const PF_KTHREAD: u64 = 0x0020_0000;

pub fn stat(pid: i32) -> Option<Stat> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    let comm = s[open + 1..close].to_string();
    // Fields from the third (state) on.
    let f: Vec<&str> = s[close + 2..].split(' ').collect();
    let num = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok());
    Some(Stat {
        comm,
        ppid: f.get(1)?.parse().ok()?,
        kernel_thread: num(6)? & PF_KTHREAD != 0,
        ticks: num(11)? + num(12)?,
        start: num(19)?,
    })
}

pub fn pids() -> Vec<i32> {
    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect()
}

/// The program's path, as the process's own mount namespace sees it.
pub fn exe(pid: i32) -> Option<String> {
    let p = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let p = p.to_string_lossy();
    Some(p.strip_suffix(" (deleted)").unwrap_or(&p).to_string())
}

pub fn cmdline(pid: i32) -> Vec<String> {
    fs::read(format!("/proc/{pid}/cmdline"))
        .unwrap_or_default()
        .split(|&b| b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

/// The cgroup v2 path, e.g. `/user.slice/user-1000.slice/…`.
pub fn cgroup(pid: i32) -> String {
    let s = fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
    s.lines()
        .find_map(|l| l.strip_prefix("0::"))
        .unwrap_or("")
        .to_string()
}

/// A cgroup's CPU total, from the first line of its cpu.stat.
pub fn cgroup_usage_us(path: &str) -> Option<u64> {
    let s = fs::read_to_string(format!("/sys/fs/cgroup{path}/cpu.stat")).ok()?;
    s.lines().next()?.strip_prefix("usage_usec ")?.trim().parse().ok()
}

pub fn mnt_ns(pid: i32) -> Option<u64> {
    fs::metadata(format!("/proc/{pid}/ns/mnt")).ok().map(|m| m.ino())
}

/// Whether the process's program is the same file as `host_path` on this side.
pub fn exe_is(pid: i32, host_path: &str) -> bool {
    match (fs::metadata(format!("/proc/{pid}/exe")), fs::metadata(host_path)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

fn status_kb(text: &str, key: &str) -> Option<u64> {
    let l = text.lines().find(|l| l.starts_with(key))?;
    l[key.len()..].split_whitespace().next()?.parse().ok()
}

/// Peak resident memory since the last `reset_peak`, in KiB.
pub fn peak_rss_kb(pid: i32) -> Option<u64> {
    status_kb(&fs::read_to_string(format!("/proc/{pid}/status")).ok()?, "VmHWM:")
}

/// Sets the peak back to current use, so the next reading is the peak of the next interval.
pub fn reset_peak(pid: i32) {
    let _ = fs::write(format!("/proc/{pid}/clear_refs"), "5");
}

/// Proportional set size: shared pages split among the processes sharing them, in KiB.
pub fn pss_kb(pid: i32) -> Option<u64> {
    status_kb(
        &fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?,
        "Pss:",
    )
}
