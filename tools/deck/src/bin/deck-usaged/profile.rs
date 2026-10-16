//! Where the CPUs execute: a sample of the instruction pointer 49 times a second per CPU, counted
//! in CPU cycles, so an idle CPU takes no samples and is not woken. Each sample names a process and
//! an address; the daemon maps the address to the library or program file mapped there, so time
//! spent in mesa, glibc or the kernel is credited to them and not only to the program that called.
//!
//! Intel's hybrid CPUs have one counter unit for the P-cores and one for the E-cores
//! (cpu_core and cpu_atom in /sys/bus/event_source/devices); each CPU is opened on its own.
//! Offsets are those of linux/perf_event.h, checked with offsetof against the 6.18 headers.

use std::fs;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{Ordering, fence};

const PERF_TYPE_HARDWARE: u32 = 0;
const PERF_COUNT_HW_CPU_CYCLES: u64 = 0;
const PERF_SAMPLE_IP: u64 = 1;
const PERF_SAMPLE_TID: u64 = 2;
const PERF_RECORD_LOST: u32 = 2;
const PERF_RECORD_SAMPLE: u32 = 9;
const PERF_RECORD_MISC_CPUMODE_MASK: u16 = 7;
const PERF_RECORD_MISC_KERNEL: u16 = 1;
const PERF_FLAG_FD_CLOEXEC: libc::c_ulong = 8;
const ATTR_SIZE: usize = 136;
// Bits of the flags word at offset 40.
const FLAG_EXCLUDE_HV: u64 = 1 << 6;
const FLAG_FREQ: u64 = 1 << 10;
const FLAG_EXCLUDE_GUEST: u64 = 1 << 20;
// struct perf_event_mmap_page
const DATA_HEAD: usize = 1024;
const DATA_TAIL: usize = 1032;
const DATA_OFFSET: usize = 1040;
const DATA_SIZE: usize = 1048;
/// Data pages per CPU: 16 KiB, a few seconds of samples at the most.
const DATA_PAGES: usize = 4;

pub struct Sample {
    pub pid: i32,
    pub ip: u64,
    pub kernel: bool,
}

struct Ring {
    _fd: OwnedFd,
    base: *mut u8,
    len: usize,
    data: usize,
    size: usize,
}

impl Drop for Ring {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.base as *mut libc::c_void, self.len) };
    }
}

pub struct Profiler {
    rings: Vec<Ring>,
    pub lost: u64,
    pub freq: u64,
}

/// A CPU list such as `0-11,14`.
fn cpu_list(s: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in s.trim().split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                    out.extend(a..=b);
                }
            }
            None => out.extend(part.parse::<u32>()),
        }
    }
    out
}

/// The counter units and their CPUs: the two of a hybrid CPU, or the one `cpu` unit.
fn pmus() -> Vec<(Option<u32>, Vec<u32>)> {
    let dev = "/sys/bus/event_source/devices";
    let hybrid: Vec<_> = ["cpu_core", "cpu_atom"]
        .iter()
        .filter_map(|n| {
            let ty = fs::read_to_string(format!("{dev}/{n}/type"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            let cpus = cpu_list(&fs::read_to_string(format!("{dev}/{n}/cpus")).ok()?);
            Some((Some(ty), cpus))
        })
        .collect();
    if !hybrid.is_empty() {
        return hybrid;
    }
    vec![(
        None,
        cpu_list(&fs::read_to_string("/sys/devices/system/cpu/online").unwrap_or_default()),
    )]
}

fn open_cpu(pmu: Option<u32>, cpu: u32, freq: u64) -> io::Result<Ring> {
    let mut attr = [0u8; ATTR_SIZE];
    let mut put = |o: usize, v: &[u8]| attr[o..o + v.len()].copy_from_slice(v);
    put(0, &PERF_TYPE_HARDWARE.to_ne_bytes());
    put(4, &(ATTR_SIZE as u32).to_ne_bytes());
    // On a hybrid CPU the unit's type goes in the config's upper half.
    let config = PERF_COUNT_HW_CPU_CYCLES | pmu.map_or(0, |t| (t as u64) << 32);
    put(8, &config.to_ne_bytes());
    put(16, &freq.to_ne_bytes());
    put(24, &(PERF_SAMPLE_IP | PERF_SAMPLE_TID).to_ne_bytes());
    put(
        40,
        &(FLAG_FREQ | FLAG_EXCLUDE_HV | FLAG_EXCLUDE_GUEST).to_ne_bytes(),
    );
    let fd = unsafe {
        libc::syscall(
            libc::SYS_perf_event_open,
            attr.as_ptr(),
            -1 as libc::pid_t,
            cpu as i32,
            -1,
            PERF_FLAG_FD_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    let len = page * (1 + DATA_PAGES);
    let base = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        )
    };
    if base == libc::MAP_FAILED {
        return Err(io::Error::last_os_error());
    }
    let base = base as *mut u8;
    let read = |o: usize| unsafe { std::ptr::read_volatile(base.add(o) as *const u64) } as usize;
    let data = match read(DATA_OFFSET) {
        0 => page,
        d => d,
    };
    let size = match read(DATA_SIZE) {
        0 => page * DATA_PAGES,
        s => s,
    };
    Ok(Ring {
        _fd: fd,
        base,
        len,
        data,
        size,
    })
}

impl Profiler {
    pub fn open(freq: u64) -> io::Result<Profiler> {
        let mut rings = Vec::new();
        for (pmu, cpus) in pmus() {
            for cpu in cpus {
                rings.push(open_cpu(pmu, cpu, freq)?);
            }
        }
        Ok(Profiler { rings, lost: 0, freq })
    }

    pub fn cpus(&self) -> usize {
        self.rings.len()
    }

    /// Takes every sample the CPUs have written since the last call.
    pub fn drain(&mut self, out: &mut Vec<Sample>) {
        let mut rec = Vec::with_capacity(64);
        for r in &self.rings {
            let head = unsafe { std::ptr::read_volatile(r.base.add(DATA_HEAD) as *const u64) };
            fence(Ordering::Acquire);
            let mut tail = unsafe { std::ptr::read_volatile(r.base.add(DATA_TAIL) as *const u64) };
            while tail + 8 <= head {
                // A record may wrap around the end of the buffer: copy it out byte by byte.
                let at = |i: u64| unsafe { *r.base.add(r.data + ((tail + i) as usize % r.size)) };
                let hdr: Vec<u8> = (0..8).map(at).collect();
                let ty = u32::from_ne_bytes(hdr[0..4].try_into().unwrap());
                let misc = u16::from_ne_bytes(hdr[4..6].try_into().unwrap());
                let size = u16::from_ne_bytes(hdr[6..8].try_into().unwrap()) as u64;
                if size < 8 || tail + size > head {
                    break;
                }
                rec.clear();
                rec.extend((0..size).map(at));
                match ty {
                    PERF_RECORD_SAMPLE if rec.len() >= 24 => out.push(Sample {
                        ip: u64::from_ne_bytes(rec[8..16].try_into().unwrap()),
                        pid: i32::from_ne_bytes(rec[16..20].try_into().unwrap()),
                        kernel: misc & PERF_RECORD_MISC_CPUMODE_MASK == PERF_RECORD_MISC_KERNEL,
                    }),
                    PERF_RECORD_LOST if rec.len() >= 24 => {
                        self.lost += u64::from_ne_bytes(rec[16..24].try_into().unwrap());
                    }
                    _ => {}
                }
                tail += size;
            }
            fence(Ordering::Release);
            unsafe { std::ptr::write_volatile(r.base.add(DATA_TAIL) as *mut u64, tail) };
        }
    }
}

/// Loaded kernel modules and their address ranges, from /proc/modules (addresses need root).
pub fn modules() -> Vec<(u64, u64, String)> {
    let mut out: Vec<(u64, u64, String)> = fs::read_to_string("/proc/modules")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let size: u64 = f.get(1)?.parse().ok()?;
            let addr = u64::from_str_radix(f.get(5)?.trim_start_matches("0x"), 16).ok()?;
            (addr != 0).then(|| (addr, addr + size, f[0].to_string()))
        })
        .collect();
    out.sort();
    out
}

/// A process's executable mappings: start, end and the file, or "" for anonymous memory (JIT).
pub fn exec_maps(pid: i32) -> Vec<(u64, u64, String)> {
    let mut out: Vec<(u64, u64, String)> = fs::read_to_string(format!("/proc/{pid}/maps"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.splitn(6, ' ');
            let range = f.next()?;
            let perms = f.next()?;
            if !perms.contains('x') {
                return None;
            }
            let (a, b) = range.split_once('-')?;
            let path = f.nth(3).unwrap_or("").trim();
            let path = path.strip_suffix(" (deleted)").unwrap_or(path);
            Some((
                u64::from_str_radix(a, 16).ok()?,
                u64::from_str_radix(b, 16).ok()?,
                path.to_string(),
            ))
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cpu_lists() {
        assert_eq!(cpu_list("0-3,6\n"), vec![0, 1, 2, 3, 6]);
        assert_eq!(cpu_list("12-19"), (12..=19).collect::<Vec<_>>());
    }
}
