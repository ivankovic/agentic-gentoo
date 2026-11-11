#!/usr/bin/env python3
"""deck-health: disk and filesystem health, checked 2 minutes after boot and every 6 hours.

Run by etc/systemd/system/deck-health.service (deck-health.timer). Fails the unit on a problem,
which the omni panel's maintenance tile shows; the last report is in /var/lib/deck-health/report.

  btrfs     the error counters btrfs keeps on disk (read, write, flush, checksum, generation):
            any non-zero is a problem
  kernel    BTRFS errors and warnings, NVMe I/O errors and timeouts in this boot's kernel log
  nvme      the SSD's own health log: a critical warning, spare below its threshold, or new
            media errors are problems; wear (percent used) over 80% too
  unclean   the SSD counts shutdowns it wasn't told about (crash, reset, power loss). When the
            count has gone up since the last check, the previous boot ended uncleanly: that's
            recorded in /var/lib/deck-health/unclean (for the panel) and a scrub starts
            (btrfs-scrub.service, idle I/O), which fails if it finds uncorrectable damage.
  kernels   a kernel that got this far into a boot is good: the last two good ones are kept in
            the Portage set @deck-kernels (/etc/portage/sets/deck-kernels, in world_sets), so
            `emerge --depclean` leaves the previous kernel and its boot menu entry in place.
  esp       the EFI system partition (/efi: FAT, no checksums) holds every kernel and the recovery
            system's UKI. Its files are copied to /var/lib/deck-health/esp, inside @, so btrbk's
            hourly snapshots keep its history; esp.blkid records the partition's identity, which
            /etc/fstab refers to. Restoring it needs a live USB: runbook.md, "The ESP is damaged".

The SSD is every NVMe controller, the one under the root filesystem first; a machine without one
skips nvme and unclean. The optional /etc/deck/host.conf (lines KEY=value) can say otherwise:
NVME="/dev/nvme0 /dev/nvme1" (empty: none), ESP=/efi (empty: no ESP to copy).

btrfs needs no repair pass after a crash (copy-on-write: it mounts at its last commit); this is
about noticing damage early. The offline `btrfs check --readonly` is in the recovery system:
`deck-mount check`.
"""
import datetime
import glob
import json
import os
import pathlib
import re
import subprocess
import sys

STATE = pathlib.Path("/var/lib/deck-health")
KERNEL_SET = pathlib.Path("/etc/portage/sets/deck-kernels")
WORLD_SETS = pathlib.Path("/var/lib/portage/world_sets")
HOST_CONF = pathlib.Path("/etc/deck/host.conf")
WEAR_LIMIT = 80


def run(*cmd):
    return subprocess.run(cmd, capture_output=True, text=True)


def stored(name):
    try:
        return int((STATE / name).read_text().strip())
    except (OSError, ValueError):
        return None


def store(name, value):
    tmp = STATE / (name + ".new")
    tmp.write_text(f"{value}\n")
    tmp.rename(STATE / name)


def host_conf(path=HOST_CONF):
    """The optional host configuration: {KEY: value} from lines KEY=value (quotes are dropped)."""
    conf = {}
    try:
        text = path.read_text()
    except OSError:
        return conf
    for line in text.splitlines():
        key, eq, value = line.strip().partition("=")
        if eq and key and not key.startswith("#"):
            conf[key.strip()] = value.strip().strip("\"'")
    return conf


def nvme_controllers(sys="/sys"):
    """The NVMe controllers, as /dev/nvmeN: the one under the root filesystem first (through
    dm-crypt and partitions), then the others. Empty on a machine without NVMe."""
    found = sorted(os.path.basename(d) for d in glob.glob(f"{sys}/class/nvme/nvme*"))
    source = run("findmnt", "-no", "SOURCE", "/").stdout.split("[")[0].strip()
    todo, under = [os.path.basename(os.path.realpath(source))] if source else [], []
    while todo:
        name = todo.pop()
        m = re.match(r"(nvme\d+)n\d+", name)
        if m:
            under.append(m.group(1))
        todo += [os.path.basename(s) for s in glob.glob(f"{sys}/class/block/{name}/slaves/*")]
    return ["/dev/" + c for c in sorted(set(under) & set(found)) + [c for c in found if c not in under]]


def kernel_packages(names, version):
    """The installed packages, of the names in /var/db/pkg/sys-kernel, that are exactly kernel
    `version`: gentoo-kernel[-bin]-<version>, with or without a revision (-r1). 6.18.5 is not
    6.18.54. [(package name, directory name)]."""
    match = re.compile(rf"(gentoo-kernel(?:-[a-z]+)*)-{re.escape(version)}(?:-r\d+)?")
    return [(m.group(1), n) for n in sorted(names) if (m := match.fullmatch(n))]


def keep_kernels():
    """Record the running kernel as good; keep the last two good ones out of --depclean's reach."""
    version = os.uname().release.split("-gentoo")[0]  # 6.18.54-gentoo-dist -> 6.18.54
    atom = None
    try:
        names = os.listdir("/var/db/pkg/sys-kernel")
    except OSError:
        names = []
    for name, d in kernel_packages(names, version):
        slot = pathlib.Path("/var/db/pkg/sys-kernel", d, "SLOT").read_text().split("/")[0].strip()
        atom = f"sys-kernel/{name}:{slot}"
    if atom is None:
        return f"kernels: {os.uname().release} isn't from a gentoo-kernel package; nothing kept"
    try:
        good = (STATE / "good-kernels").read_text().split()
    except OSError:
        good = []
    good = ([g for g in good if g != atom] + [atom])[-2:]
    store_text(STATE / "good-kernels", "\n".join(good) + "\n")
    KERNEL_SET.parent.mkdir(parents=True, exist_ok=True)
    store_text(KERNEL_SET, "# Written by /etc/deck-health/check.py: the last two kernels that booted.\n"
               + "\n".join(good) + "\n")
    sets = WORLD_SETS.read_text().split() if WORLD_SETS.exists() else []
    if "@deck-kernels" not in sets:
        store_text(WORLD_SETS, "\n".join(sets + ["@deck-kernels"]) + "\n")
    return "kernels kept: " + ", ".join(good)


def backup_esp(esp):
    """Mirror the ESP's files into @; returns (report line, problem or None)."""
    dest = STATE / "esp"
    if not esp:
        return "esp: none on this host; nothing copied", None
    if not os.path.ismount(esp):
        return f"esp: {esp} isn't mounted; not copied", f"ESP ({esp}) isn't mounted"
    # FAT keeps times to 2 seconds: --modify-window stops rsync recopying unchanged files. Root-only,
    # like the ESP itself (systemd-boot's random seed is on it).
    r = run("rsync", "-rt", "--delete", "--modify-window=1", "--chmod=go=", esp + "/", str(dest) + "/")
    if r.returncode != 0:
        return f"esp: copy failed: {r.stderr.strip()[:160]}", "ESP copy failed"
    source = run("findmnt", "-no", "SOURCE", esp).stdout.strip()
    store_text(STATE / "esp.blkid", run("blkid", "-o", "export", source).stdout)
    size = run("du", "-sh", str(dest)).stdout.split("\t")[0]
    return f"esp: copied to {dest} ({size})", None


def store_text(path, text):
    tmp = path.with_name(path.name + ".new")
    tmp.write_text(text)
    tmp.rename(path)


def main():
    now = datetime.datetime.now().astimezone()
    problems, lines = [], [f"deck-health {now:%Y-%m-%d %H:%M}"]

    # btrfs error counters, kept on disk across boots.
    r = run("btrfs", "device", "stats", "--check", "/")
    nonzero = [l.strip() for l in r.stdout.splitlines() if l.split() and l.split()[-1] != "0"]
    if r.returncode != 0:
        problems.append("btrfs error counters: " + ("; ".join(nonzero) or r.stderr.strip()))
    lines.append("btrfs counters: " + ("; ".join(nonzero) if nonzero else "all zero"))

    # This boot's kernel log.
    r = run("journalctl", "-k", "-b", "-o", "cat", "--no-pager")
    pattern = re.compile(r"BTRFS (error|critical|warning)|nvme\S*: .*(I/O error|timeout|resetting|failed)", re.I)
    hits = [l for l in r.stdout.splitlines() if pattern.search(l)]
    if hits:
        problems.append(f"kernel log: {len(hits)} disk message(s), last: {hits[-1][:160]}")
    lines.append(f"kernel log: {len(hits)} disk errors or warnings this boot")

    # Each SSD's health log. The first keeps its counts under the plain names, the others under
    # <name>.<controller>.
    conf = host_conf()
    devices = conf["NVME"].split() if "NVME" in conf else nvme_controllers()
    if not devices:
        lines.append("nvme: no NVMe drive; not checked")
    for i, dev in enumerate(devices):
        suffix = "." + os.path.basename(dev) if i else ""
        tag = f" {os.path.basename(dev)}" if len(devices) > 1 else ""
        r = run("nvme", "smart-log", dev, "-o", "json")
        try:
            smart = json.loads(r.stdout)
        except ValueError:
            problems.append(f"nvme smart-log {dev} failed: {r.stderr.strip()[:160]}")
            smart = None
        if smart is not None:
            warning = smart.get("critical_warning", 0)
            if isinstance(warning, dict):  # newer nvme-cli spells out the bits
                warning = warning.get("value", sum(v for v in warning.values() if isinstance(v, int)))
            spare, spare_min = smart.get("avail_spare", 100), smart.get("spare_thresh", 0)
            used = smart.get("percent_used", smart.get("percentage_used", 0))
            media = smart.get("media_errors", 0)
            unsafe = smart.get("unsafe_shutdowns", 0)
            lines.append(f"nvme{tag}: critical warning {warning}, spare {spare}% (min {spare_min}%), "
                         f"wear {used}%, media errors {media}, unsafe shutdowns {unsafe}")
            if warning:
                problems.append(f"SSD{tag} critical warning {warning:#x}")
            if spare < spare_min:
                problems.append(f"SSD{tag} spare {spare}% below its threshold {spare_min}%")
            if used > WEAR_LIMIT:
                problems.append(f"SSD{tag} wear {used}%")
            prev_media = stored("media_errors" + suffix)
            if prev_media is not None and media > prev_media:
                problems.append(f"SSD{tag} media errors {prev_media} -> {media}")
            store("media_errors" + suffix, media)

            prev_unsafe = stored("unsafe_shutdowns" + suffix)
            if prev_unsafe is not None and unsafe > prev_unsafe:
                (STATE / "unclean").write_text(f"{now.isoformat(timespec='seconds')}\n")
                started = run("systemctl", "start", "--no-block", "btrfs-scrub.service")
                lines.append(f"unclean shutdown before this boot ({prev_unsafe} -> {unsafe}); scrub "
                             + ("started" if started.returncode == 0 else "failed to start"))
            store("unsafe_shutdowns" + suffix, unsafe)

    lines.append(keep_kernels())
    line, problem = backup_esp(conf.get("ESP", "/efi"))
    lines.append(line)
    if problem:
        problems.append(problem)

    lines += [f"PROBLEM: {p}" for p in problems] or ["no problems"]
    report = "\n".join(lines) + "\n"
    (STATE / "report.new").write_text(report)
    (STATE / "report.new").rename(STATE / "report")
    print(report, end="")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
