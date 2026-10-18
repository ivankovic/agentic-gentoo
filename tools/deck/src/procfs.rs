//! What the daemons read from /proc.

use std::fs::{self, File};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{FileExt, MetadataExt};

pub struct Stat {
    pub comm: String,
    pub ppid: i32,
    pub pgrp: i32,
    /// The controlling terminal, 0 for none.
    pub tty: i32,
    /// The terminal's foreground process group.
    pub tpgid: i32,
    pub kernel_thread: bool,
    /// User plus system time of all threads, live and exited, in clock ticks.
    pub ticks: u64,
    /// Start time after boot, in clock ticks: with the PID, it names a process uniquely.
    pub start: u64,
}

const PF_KTHREAD: u64 = 0x0020_0000;

pub fn stat(pid: i32) -> Option<Stat> {
    parse_stat(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

fn parse_stat(s: &str) -> Option<Stat> {
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    let comm = s[open + 1..close].to_string();
    // Fields from the third (state) on.
    let f: Vec<&str> = s[close + 2..].split(' ').collect();
    let num = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok());
    Some(Stat {
        comm,
        ppid: f.get(1)?.parse().ok()?,
        pgrp: f.get(2)?.parse().ok()?,
        tty: f.get(4)?.parse().ok()?,
        tpgid: f.get(5)?.parse().ok()?,
        kernel_thread: num(6)? & PF_KTHREAD != 0,
        ticks: num(11)? + num(12)?,
        start: num(19)?,
    })
}

/// A process's stat file, held open: reading it again is one pread, with no path to look up, and
/// the descriptor stays with the process it was opened for (a PID taken by a new process reads as
/// gone).
pub struct StatFile(File);

impl StatFile {
    pub fn open(pid: i32) -> Option<StatFile> {
        File::open(format!("/proc/{pid}/stat")).ok().map(StatFile)
    }

    /// CPU ticks (`Stat::ticks`) and start time (`Stat::start`); None once the process is gone.
    pub fn times(&self, buf: &mut [u8]) -> Option<(u64, u64)> {
        let n = self.0.read_at(buf, 0).ok()?;
        times(&buf[..n])
    }
}

fn number(b: &[u8]) -> Option<u64> {
    std::str::from_utf8(b).ok()?.parse().ok()
}

/// What `parse_stat` gives as `ticks` and `start`, without allocating.
fn times(b: &[u8]) -> Option<(u64, u64)> {
    let close = b.iter().rposition(|&c| c == b')')?;
    // Fields from the third (state) on: utime is the 12th of them, stime the 13th, start the 20th.
    let mut f = b.get(close + 2..)?.split(|&c| c == b' ');
    let utime = number(f.nth(11)?)?;
    let stime = number(f.next()?)?;
    let start = number(f.nth(6)?)?;
    Some((utime + stime, start))
}

/// The PIDs in /proc, into `out`.
pub fn pids(out: &mut Vec<i32>) {
    out.clear();
    out.extend(
        fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok()),
    );
}

/// The program's path, as the process's own mount namespace sees it.
pub fn exe(pid: i32) -> Option<String> {
    exe_deleted(pid).map(|(p, _)| p)
}

/// The program's path, and whether the file was deleted or replaced since it started (an update).
pub fn exe_deleted(pid: i32) -> Option<(String, bool)> {
    let p = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let p = p.to_string_lossy();
    Some(match p.strip_suffix(" (deleted)") {
        Some(p) => (p.to_string(), true),
        None => (p.to_string(), false),
    })
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

/// A cgroup's cpu.stat, held open like `StatFile`.
pub struct CpuStat(File);

impl CpuStat {
    pub fn open(path: &str) -> Option<CpuStat> {
        File::open(format!("/sys/fs/cgroup{path}/cpu.stat")).ok().map(CpuStat)
    }

    /// The CPU total, from the first line; None once the cgroup is gone.
    pub fn usage_us(&self, buf: &mut [u8]) -> Option<u64> {
        let n = self.0.read_at(buf, 0).ok()?;
        let line = buf[..n].split(|&c| c == b'\n').next()?;
        number(line.strip_prefix(b"usage_usec ")?)
    }
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

/// Peak resident memory since the last `reset_peak`, and resident memory now, in KiB.
pub fn peak_rss_kb(pid: i32) -> Option<(u64, u64)> {
    let s = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    Some((status_kb(&s, "VmHWM:")?, status_kb(&s, "VmRSS:")?))
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

/// The process's children (CONFIG_PROC_CHILDREN), through its main thread.
pub fn children(pid: i32) -> Vec<i32> {
    fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|c| c.parse().ok())
        .collect()
}

/// What an interpreter runs: the first of its first few arguments that is a file. Resolved as the
/// process itself would: in its own root and working directory, and with its own user's rights,
/// not root's. Arguments are the process's to choose: resolved as root, in the daemon's view, any
/// process could learn which files exist, and where links lead, where it can't look itself.
pub fn script_path(pid: i32) -> Option<String> {
    let args = cmdline(pid);
    let root = File::open(format!("/proc/{pid}/root")).ok()?;
    let cwd = fs::read_link(format!("/proc/{pid}/cwd")).ok();
    // Its file system ids, from status: /proc/<pid> itself shows root as the owner of a process
    // that isn't dumpable, and any user can make one.
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let id = |key: &str| -> Option<u32> {
        let l = status.lines().find(|l| l.starts_with(key))?;
        l[key.len()..].split_whitespace().nth(3)?.parse().ok()
    };
    let _as = FsCreds::of(id("Uid:")?, id("Gid:")?)?;
    for a in args.iter().skip(1).take(4) {
        if a.starts_with('-') {
            continue;
        }
        let path = if a.starts_with('/') {
            a.clone()
        } else {
            format!("{}/{a}", cwd.as_ref()?.display())
        };
        let Some(f) = open_in_root(&root, &path) else {
            continue;
        };
        if f.metadata().is_ok_and(|m| m.is_file())
            && let Ok(real) = fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd()))
        {
            return Some(real.to_string_lossy().into_owned());
        }
    }
    None
}

/// `path` looked up as if `root` were /: `..` and absolute links stay inside it.
fn open_in_root(root: &File, path: &str) -> Option<File> {
    let c = std::ffi::CString::new(path).ok()?;
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (libc::O_PATH | libc::O_CLOEXEC) as u64;
    how.resolve = libc::RESOLVE_IN_ROOT | libc::RESOLVE_NO_MAGICLINKS;
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            root.as_raw_fd(),
            c.as_ptr(),
            &how as *const libc::open_how,
            size_of::<libc::open_how>(),
        )
    };
    (fd >= 0).then(|| unsafe { File::from_raw_fd(fd as i32) })
}

/// For root: file access as another user until dropped (setfsuid, for this thread). A file system
/// uid other than 0 also takes away root's overrides of file permissions until it is 0 again.
/// None when the switch didn't take: then nothing is looked up.
struct FsCreds(bool);

impl FsCreds {
    fn of(uid: u32, gid: u32) -> Option<FsCreds> {
        if unsafe { libc::geteuid() } != 0 || uid == 0 {
            return Some(FsCreds(false));
        }
        let creds = FsCreds(true);
        unsafe {
            libc::setfsgid(gid);
            libc::setfsuid(uid);
        }
        // setfsuid reports the previous value, and fails silently: -1 only asks.
        let now = unsafe { (libc::setfsuid(u32::MAX), libc::setfsgid(u32::MAX)) };
        (now == (uid as i32, gid as i32)).then_some(creds)
    }
}

impl Drop for FsCreds {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                libc::setfsuid(0);
                libc::setfsgid(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_times_as_parse_stat_does() {
        let line = "4242 (a (b) c) S 1 4242 4242 0 -1 4194560 5000 0 0 0 1234 567 0 0 20 0 3 0 98765 \
                    1000000 2000 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0\n";
        let st = parse_stat(line).unwrap();
        assert_eq!(st.comm, "a (b) c");
        assert_eq!((st.ticks, st.start), (1234 + 567, 98765));
        assert_eq!(times(line.as_bytes()), Some((st.ticks, st.start)));
        assert_eq!(times(b"4242 (x) S 1 2"), None);
    }

    #[test]
    fn reads_this_process_through_its_descriptors() {
        let me = std::process::id() as i32;
        let f = StatFile::open(me).unwrap();
        let mut buf = [0u8; 1024];
        let (_, start) = f.times(&mut buf).unwrap();
        assert_eq!(start, stat(me).unwrap().start);
        let cg = cgroup(me);
        if let Some(c) = CpuStat::open(&cg) {
            assert!(c.usage_us(&mut buf).is_some());
        }
    }

    #[test]
    fn finds_a_script_in_the_process_view() {
        // This process runs as itself: the path comes back as it is.
        let me = std::process::id() as i32;
        let exe = std::env::current_exe().unwrap();
        let root = File::open(format!("/proc/{me}/root")).unwrap();
        let f = open_in_root(&root, exe.to_str().unwrap()).unwrap();
        assert!(f.metadata().unwrap().is_file());
        assert!(open_in_root(&root, "/no/such/file").is_none());
    }

    #[test]
    fn names_an_interpreters_script_through_a_link() {
        let dir = std::env::temp_dir().join(format!("deck-procfs-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("x.sh"), "sleep 5\n").unwrap();
        let _ = fs::remove_file(dir.join("link.sh"));
        std::os::unix::fs::symlink("x.sh", dir.join("link.sh")).unwrap();
        let mut child = std::process::Command::new("sh")
            .arg("link.sh")
            .current_dir(&dir)
            .spawn()
            .unwrap();
        let mut found = None;
        for _ in 0..50 {
            found = script_path(child.id() as i32);
            if found.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
        let real = fs::canonicalize(dir.join("x.sh")).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.as_deref(), real.to_str());
    }
}
