//! Two kernel event sources over netlink: the proc connector (fork, exec and exit, as they happen)
//! and taskstats (each exiting thread's CPU time and peak memory). Both need CAP_NET_ADMIN.
//!
//! Offsets are those of linux/taskstats.h and linux/cn_proc.h; checked with offsetof against the
//! 6.18 headers (taskstats version 16).

use std::io;
use std::mem::size_of;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::{Duration, Instant};

const NETLINK_CONNECTOR: i32 = 11;
const NETLINK_GENERIC: i32 = 16;
const CN_IDX_PROC: u32 = 1;
const CN_VAL_PROC: u32 = 1;
const PROC_CN_MCAST_LISTEN: u32 = 1;
const PROC_EVENT_FORK: u32 = 1;
const PROC_EVENT_EXEC: u32 = 2;

const NLMSG_HDRLEN: usize = 16;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_REQUEST: u16 = 1;
const NLM_F_ACK: u16 = 4;
const GENL_ID_CTRL: u16 = 0x10;
const CTRL_CMD_GETFAMILY: u8 = 3;
const CTRL_ATTR_FAMILY_ID: u16 = 1;
const CTRL_ATTR_FAMILY_NAME: u16 = 2;
const TASKSTATS_CMD_GET: u8 = 1;
const TASKSTATS_CMD_ATTR_REGISTER_CPUMASK: u16 = 3;
const TASKSTATS_TYPE_STATS: u16 = 3;
const TASKSTATS_TYPE_AGGR_PID: u16 = 4;

// struct taskstats
const TS_COMM: usize = 80;
const TS_PID: usize = 128;
const TS_UTIME: usize = 152;
const TS_STIME: usize = 160;
const TS_HIWATER_RSS: usize = 200;
const TS_TGID: usize = 368;

pub enum ProcEvent {
    /// A new process; thread creation is left out.
    Fork {
        parent: i32,
        child: i32,
    },
    Exec {
        tgid: i32,
    },
}

pub struct ThreadExit {
    pub tgid: i32,
    /// User plus system time of this thread over its whole life.
    pub cpu_us: u64,
    /// Peak resident memory of the process, in KiB.
    pub hiwater_kb: u64,
    pub comm: String,
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_ne_bytes(b[o..o + 2].try_into().unwrap())
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_ne_bytes(b[o..o + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_ne_bytes(b[o..o + 8].try_into().unwrap())
}

fn socket(proto: i32, kind: i32, groups: u32) -> io::Result<OwnedFd> {
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            kind | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            proto,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut addr: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    addr.nl_family = libc::AF_NETLINK as u16;
    addr.nl_groups = groups;
    let r = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            &addr as *const libc::sockaddr_nl as *const libc::sockaddr,
            size_of::<libc::sockaddr_nl>() as u32,
        )
    };
    if r < 0 {
        return Err(io::Error::last_os_error());
    }
    // Exits come in bursts (a parallel build); whatever overflows the buffer is lost.
    let size: i32 = 16 << 20;
    for opt in [libc::SO_RCVBUFFORCE, libc::SO_RCVBUF] {
        let r = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                opt,
                &size as *const i32 as *const libc::c_void,
                size_of::<i32>() as u32,
            )
        };
        if r == 0 {
            break;
        }
    }
    Ok(fd)
}

fn nlmsg(ty: u16, flags: u16, payload: &[u8]) -> Vec<u8> {
    let len = NLMSG_HDRLEN + payload.len();
    let mut v = Vec::with_capacity(align4(len));
    v.extend((len as u32).to_ne_bytes());
    v.extend(ty.to_ne_bytes());
    v.extend(flags.to_ne_bytes());
    v.extend(0u32.to_ne_bytes()); // seq
    v.extend(0u32.to_ne_bytes()); // port id: the kernel fills it in
    v.extend(payload);
    v.resize(align4(len), 0);
    v
}

fn attr(ty: u16, data: &[u8]) -> Vec<u8> {
    let len = 4 + data.len();
    let mut v = Vec::with_capacity(align4(len));
    v.extend((len as u16).to_ne_bytes());
    v.extend(ty.to_ne_bytes());
    v.extend(data);
    v.resize(align4(len), 0);
    v
}

fn send(fd: &OwnedFd, buf: &[u8]) -> io::Result<()> {
    let mut to: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    to.nl_family = libc::AF_NETLINK as u16;
    let r = unsafe {
        libc::sendto(
            fd.as_raw_fd(),
            buf.as_ptr() as *const libc::c_void,
            buf.len(),
            0,
            &to as *const libc::sockaddr_nl as *const libc::sockaddr,
            size_of::<libc::sockaddr_nl>() as u32,
        )
    };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// One datagram, or WouldBlock when none is queued.
fn recv(fd: &OwnedFd, buf: &mut [u8]) -> io::Result<usize> {
    let r = unsafe {
        libc::recv(
            fd.as_raw_fd(),
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
            0,
        )
    };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r as usize)
    }
}

/// Waits up to `timeout` for a datagram; for the replies to setup requests.
fn recv_wait(fd: &OwnedFd, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
    let end = Instant::now() + timeout;
    loop {
        match recv(fd, buf) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < end => {
                let mut p = libc::pollfd {
                    fd: fd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                unsafe { libc::poll(&mut p, 1, 100) };
            }
            r => return r,
        }
    }
}

/// The netlink messages in a datagram: (type, payload).
fn messages(mut b: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    std::iter::from_fn(move || {
        if b.len() < NLMSG_HDRLEN {
            return None;
        }
        let len = u32_at(b, 0) as usize;
        if len < NLMSG_HDRLEN || len > b.len() {
            return None;
        }
        let msg = (u16_at(b, 4), &b[NLMSG_HDRLEN..len]);
        b = &b[align4(len).min(b.len())..];
        Some(msg)
    })
}

/// The attributes in a payload: (type without the nested and byte-order flags, data).
fn attrs(mut b: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    std::iter::from_fn(move || {
        if b.len() < 4 {
            return None;
        }
        let len = u16_at(b, 0) as usize;
        if len < 4 || len > b.len() {
            return None;
        }
        let a = (u16_at(b, 2) & 0x3fff, &b[4..len]);
        b = &b[align4(len).min(b.len())..];
        Some(a)
    })
}

/// An NLMSG_ERROR payload's errno; 0 is an acknowledgement.
fn nl_error(payload: &[u8]) -> io::Result<()> {
    match payload
        .get(..4)
        .map(|e| i32::from_ne_bytes(e.try_into().unwrap()))
    {
        Some(0) => Ok(()),
        Some(e) => Err(io::Error::from_raw_os_error(-e)),
        None => Err(io::Error::other("short netlink error message")),
    }
}

pub struct ProcConnector {
    fd: OwnedFd,
    buf: Vec<u8>,
}

impl ProcConnector {
    pub fn open() -> io::Result<ProcConnector> {
        let fd = socket(NETLINK_CONNECTOR, libc::SOCK_DGRAM, CN_IDX_PROC)?;
        let mut cn = Vec::new();
        cn.extend(CN_IDX_PROC.to_ne_bytes());
        cn.extend(CN_VAL_PROC.to_ne_bytes());
        cn.extend(0u32.to_ne_bytes()); // seq
        cn.extend(0u32.to_ne_bytes()); // ack
        cn.extend(4u16.to_ne_bytes()); // len
        cn.extend(0u16.to_ne_bytes()); // flags
        cn.extend(PROC_CN_MCAST_LISTEN.to_ne_bytes());
        send(&fd, &nlmsg(NLMSG_DONE, 0, &cn))?;
        Ok(ProcConnector {
            fd,
            buf: vec![0; 64 << 10],
        })
    }

    pub fn fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// Reads every queued event. Returns how many times the queue had overflowed.
    pub fn drain(&mut self, out: &mut Vec<ProcEvent>) -> io::Result<u64> {
        let mut lost = 0;
        loop {
            let n = match recv(&self.fd, &mut self.buf) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(lost),
                Err(e) if e.raw_os_error() == Some(libc::ENOBUFS) => {
                    lost += 1;
                    continue;
                }
                Err(e) => return Err(e),
            };
            for (_, p) in messages(&self.buf[..n]) {
                // struct cn_msg (20 bytes), then struct proc_event: what, cpu, timestamp, event data at 16.
                if p.len() < 20 + 24 || u32_at(p, 0) != CN_IDX_PROC {
                    continue;
                }
                let ev = &p[20..];
                let i32_at = |o: usize| u32_at(ev, o) as i32;
                match u32_at(ev, 0) {
                    PROC_EVENT_FORK if ev.len() >= 32 => {
                        // parent_pid, parent_tgid, child_pid, child_tgid
                        if i32_at(24) == i32_at(28) {
                            out.push(ProcEvent::Fork {
                                parent: i32_at(20),
                                child: i32_at(28),
                            });
                        }
                    }
                    PROC_EVENT_EXEC => out.push(ProcEvent::Exec { tgid: i32_at(20) }),
                    _ => {}
                }
            }
        }
    }
}

pub struct Taskstats {
    fd: OwnedFd,
    family: u16,
    buf: Vec<u8>,
}

impl Taskstats {
    /// Registers for exit statistics of tasks on `cpus`, a list such as "0-19".
    pub fn open(cpus: &str) -> io::Result<Taskstats> {
        let fd = socket(NETLINK_GENERIC, libc::SOCK_RAW, 0)?;
        let mut buf = vec![0; 256 << 10];

        let mut req = vec![CTRL_CMD_GETFAMILY, 1, 0, 0];
        req.extend(attr(CTRL_ATTR_FAMILY_NAME, b"TASKSTATS\0"));
        send(&fd, &nlmsg(GENL_ID_CTRL, NLM_F_REQUEST, &req))?;
        let n = recv_wait(&fd, &mut buf, Duration::from_secs(2))?;
        let mut family = None;
        for (ty, p) in messages(&buf[..n]) {
            if ty == NLMSG_ERROR {
                nl_error(p)?;
            } else if ty == GENL_ID_CTRL && p.len() > 4 {
                family = attrs(&p[4..])
                    .find(|(t, _)| *t == CTRL_ATTR_FAMILY_ID)
                    .map(|(_, d)| u16_at(d, 0));
            }
        }
        let family = family.ok_or_else(|| io::Error::other("no TASKSTATS netlink family"))?;

        let mut req = vec![TASKSTATS_CMD_GET, 1, 0, 0];
        let mut mask = cpus.as_bytes().to_vec();
        mask.push(0);
        req.extend(attr(TASKSTATS_CMD_ATTR_REGISTER_CPUMASK, &mask));
        send(&fd, &nlmsg(family, NLM_F_REQUEST | NLM_F_ACK, &req))?;
        let n = recv_wait(&fd, &mut buf, Duration::from_secs(2))?;
        for (ty, p) in messages(&buf[..n]) {
            if ty == NLMSG_ERROR {
                nl_error(p)?;
            }
        }
        Ok(Taskstats { fd, family, buf })
    }

    pub fn fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// Reads every queued exit report. Returns how many times the queue had overflowed.
    pub fn drain(&mut self, out: &mut Vec<ThreadExit>) -> io::Result<u64> {
        let mut lost = 0;
        loop {
            let n = match recv(&self.fd, &mut self.buf) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(lost),
                Err(e) if e.raw_os_error() == Some(libc::ENOBUFS) => {
                    lost += 1;
                    continue;
                }
                Err(e) => return Err(e),
            };
            for (ty, p) in messages(&self.buf[..n]) {
                if ty != self.family || p.len() < 4 {
                    continue;
                }
                // Only the per-thread part: the per-process aggregate sent when the last thread
                // exits carries delay accounting, not CPU time.
                for (t, nested) in attrs(&p[4..]) {
                    if t != TASKSTATS_TYPE_AGGR_PID {
                        continue;
                    }
                    for (t, s) in attrs(nested) {
                        if t == TASKSTATS_TYPE_STATS && s.len() >= TS_TGID + 4 {
                            let comm = &s[TS_COMM..TS_COMM + 32];
                            let end = comm.iter().position(|&c| c == 0).unwrap_or(32);
                            let tgid = match u32_at(s, TS_TGID) {
                                0 => u32_at(s, TS_PID),
                                t => t,
                            };
                            out.push(ThreadExit {
                                tgid: tgid as i32,
                                cpu_us: u64_at(s, TS_UTIME) + u64_at(s, TS_STIME),
                                hiwater_kb: u64_at(s, TS_HIWATER_RSS),
                                comm: String::from_utf8_lossy(&comm[..end]).into_owned(),
                            });
                        }
                    }
                }
            }
        }
    }
}
