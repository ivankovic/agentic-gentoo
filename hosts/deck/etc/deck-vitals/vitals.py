#!/usr/bin/env python3
"""deck-vitals: one line of hardware state every 2 s on AC, 10 s on battery.

Run by etc/systemd/system/deck-vitals.service into its own journal namespace ("vitals"), which
keeps a few days (etc/systemd/journald@vitals.conf). After an unexplained reset, the last lines
before it show what the machine was doing:

    journalctl --namespace=vitals -b -1 -n 50

One resident process reading sysfs and procfs directly: no subprocesses, about 1 ms per sample.
Fields, all optional (a missing sensor is left out):

  pkg core gpu nvme wifi ec   temperatures in °C (CPU package, hottest core, the EC's GPU
                              sensor, SSD, Wi-Fi card, hottest EC sensor)
  fan                         RPM
  psys pkg_w core_w uncore_w  power in W over the last interval (RAPL: whole platform, CPU
                              package, cores, uncore/graphics)
  pl1 pl2                     the package power limits in force, W
  mhz mhz_max gpu_mhz         average and fastest CPU core, graphics frequency
  load                        1-minute load average
  mem_avail swap_used         GiB
  psi_cpu psi_mem psi_io      pressure: % of time stalled over the last 10 s ("some")
  throttle                    package thermal-throttle events since the previous line
  aer                         PCIe errors (correctable+nonfatal+fatal) since the previous line
  ac bat bat_w profile        on AC (1/0), battery %, battery discharge W, platform profile

The EC's sensors (gpu, ec, fan) and the battery go through ACPI at about 1 ms a read, so on AC
they're in every fifth line; the SSD temperature wakes the drive, so every fifteenth.
"""
import glob
import os
import sys
import time


def read(path, default=None):
    try:
        with open(path) as f:
            return f.read().strip()
    except OSError:
        return default


def num(path, scale=1.0):
    v = read(path)
    try:
        return float(v) / scale
    except (TypeError, ValueError):
        return None


def hwmon(name):
    """The hwmon directory for a driver name; the numbers change between boots."""
    for d in glob.glob("/sys/class/hwmon/hwmon*"):
        if read(d + "/name") == name:
            return d
    return None


CORETEMP = hwmon("coretemp")
THINKPAD = hwmon("thinkpad")
NVME = hwmon("nvme")
WIFI = next((d for d in glob.glob("/sys/class/hwmon/hwmon*") if (read(d + "/name") or "").startswith("iwlwifi")), None)
FAN = THINKPAD or hwmon("acpi_fan")


def labelled(d, label):
    for p in glob.glob(f"{d}/temp*_label"):
        if read(p) == label:
            return num(p.replace("_label", "_input"), 1000)
    return None


def temps(slow):
    """slow: also the EC's sensors, about 0.75 ms each through ACPI, so only every 10 s."""
    out = {}
    if CORETEMP:
        out["pkg"] = labelled(CORETEMP, "Package id 0")
        cores = [num(p, 1000) for p in glob.glob(CORETEMP + "/temp*_input")
                 if (read(p.replace("_input", "_label")) or "").startswith("Core")]
        cores = [c for c in cores if c is not None]
        out["core"] = max(cores) if cores else None
    if THINKPAD and slow:
        out["gpu"] = labelled(THINKPAD, "GPU")
        ec = [num(p, 1000) for p in glob.glob(THINKPAD + "/temp*_input")]
        ec = [t for t in ec if t is not None and 0 < t < 128]  # unused EC slots read as 0 or -128
        out["ec"] = max(ec) if ec else None
    if WIFI:
        out["wifi"] = num(WIFI + "/temp1_input", 1000)
    return out


RAPL = {"psys": "/sys/class/powercap/intel-rapl:1", "pkg_w": "/sys/class/powercap/intel-rapl:0",
        "core_w": "/sys/class/powercap/intel-rapl:0:0", "uncore_w": "/sys/class/powercap/intel-rapl:0:1"}


class Energy:
    """Watts from RAPL's cumulative energy counters, handling wrap-around."""

    def __init__(self):
        self.prev = {}

    def sample(self, now):
        out = {}
        for key, d in RAPL.items():
            e = num(d + "/energy_uj")
            if e is None:
                continue
            if key in self.prev:
                pe, pt = self.prev[key]
                de = e - pe
                if de < 0:
                    de += num(d + "/max_energy_range_uj") or 0
                if now > pt:
                    out[key] = de / 1e6 / (now - pt)
            self.prev[key] = (e, now)
        return out


def limits():
    d = "/sys/class/powercap/intel-rapl:0"
    out = {}
    for i in range(3):
        name = read(f"{d}/constraint_{i}_name")
        if name in ("long_term", "short_term"):
            out["pl1" if name == "long_term" else "pl2"] = num(f"{d}/constraint_{i}_power_limit_uw", 1e6)
    return out


def freqs():
    f = [num(p, 1000) for p in glob.glob("/sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_cur_freq")]
    f = [x for x in f if x is not None]
    out = {"mhz": sum(f) / len(f), "mhz_max": max(f)} if f else {}
    g = glob.glob("/sys/class/drm/card*/gt_act_freq_mhz")
    if g:
        out["gpu_mhz"] = num(g[0])
    return out


def memory():
    m = {}
    for line in (read("/proc/meminfo") or "").splitlines():
        k, _, v = line.partition(":")
        m[k] = float(v.split()[0]) / 1048576  # kB to GiB
    out = {}
    if "MemAvailable" in m:
        out["mem_avail"] = m["MemAvailable"]
    if "SwapTotal" in m and "SwapFree" in m:
        out["swap_used"] = m["SwapTotal"] - m["SwapFree"]
    return out


def pressure():
    out = {}
    for res in ("cpu", "memory", "io"):
        line = (read(f"/proc/pressure/{res}") or "").splitlines()
        if line and line[0].startswith("some"):
            for part in line[0].split():
                if part.startswith("avg10="):
                    out["psi_" + ("mem" if res == "memory" else res)] = float(part[6:])
    return out


def counter_sum(pattern, field=None):
    total = 0
    for p in glob.glob(pattern):
        text = read(p) or ""
        if field is None:
            try:
                total += int(text)
            except ValueError:
                pass
        else:  # aer_dev_*: lines "NAME count", the total on "TOTAL_ERR_..."
            for line in text.splitlines():
                if line.startswith("TOTAL_ERR"):
                    total += int(line.split()[1])
    return total


class Delta:
    def __init__(self):
        self.prev = {}

    def __call__(self, key, value):
        d = value - self.prev.get(key, value)
        self.prev[key] = value
        return d


def power_supply(slow):
    """slow: also the battery, read through ACPI (about 3 ms)."""
    out = {}
    ac = read("/sys/class/power_supply/AC/online")
    if ac is not None:
        out["ac"] = int(ac)
    if not slow:
        return out
    cap = read("/sys/class/power_supply/BAT0/capacity")
    if cap is not None:
        out["bat"] = int(cap)
    p = num("/sys/class/power_supply/BAT0/power_now", 1e6)
    if p is not None and read("/sys/class/power_supply/BAT0/status") == "Discharging":
        out["bat_w"] = p
    prof = read("/sys/firmware/acpi/platform_profile")
    if prof:
        out["profile"] = prof
    return out


def fmt(fields):
    parts = []
    for k, v in fields.items():
        if v is None:
            continue
        if isinstance(v, float):
            v = f"{v:.1f}" if abs(v) < 100 else f"{v:.0f}"
        parts.append(f"{k}={v}")
    return " ".join(parts)


def main():
    energy, delta = Energy(), Delta()
    n = 0
    while True:
        now = time.monotonic()
        on_ac = read("/sys/class/power_supply/AC/online") == "1"
        slow = not on_ac or n % 5 == 0  # EC and battery: every 10 s on AC, every sample on battery
        f = {}
        f.update(temps(slow))
        if NVME and n % 15 == 0:  # reading it wakes the SSD: every ~30 s on AC
            f["nvme"] = labelled(NVME, "Composite")
        if FAN and slow:
            f["fan"] = num(FAN + "/fan1_input")
        f.update(energy.sample(now))
        f.update(limits())
        f.update(freqs())
        f["load"] = float((read("/proc/loadavg") or "0").split()[0])
        f.update(memory())
        f.update(pressure())
        f["throttle"] = delta("throttle", counter_sum("/sys/devices/system/cpu/cpu*/thermal_throttle/package_throttle_count"))
        if n % 30 == 0:
            f["aer"] = delta("aer", counter_sum("/sys/bus/pci/devices/*/aer_dev_*", field=True))
        f.update(power_supply(slow))
        if n > 0:  # the first line has no power or deltas yet
            print(fmt(f), flush=True)
        n += 1
        time.sleep(2 if on_ac else 10)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(0)
