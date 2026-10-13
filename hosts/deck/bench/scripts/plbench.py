#!/usr/bin/env python3
"""Sustained power limit benchmark: a clean cargo release build at several MMIO PL1 values.

    sudo BENCH_SRC=/path/to/rust/project python3 -I plbench.py [WATTS...]

WATTS default to "0 35 40 0 35 40"; 0 is the firmware's own value. Builds as the sudo user, without
sccache, after a warm-up build, each from below 50 °C. Records time, package power (RAPL), temperature,
fan, clocks, the MMIO PL1 values seen, and wakeup latency (latprobe.py). Restores the firmware's PL1
and the power profile afterwards. Results: ../2026-10-13-power-and-scheduler.md."""
import glob, json, os, statistics, subprocess, sys, threading, time

S = os.path.dirname(os.path.abspath(__file__))
SRC = os.environ["BENCH_SRC"]
USER = os.environ["SUDO_USER"]  # run with sudo; builds run as that user
USER_HOME = os.path.expanduser("~" + USER)
TARGET = f"{USER_HOME}/.cache/plbench-target"
MMIO = "/sys/class/powercap/intel-rapl-mmio:0"
MSR = "/sys/class/powercap/intel-rapl:0"
LIMITS = [int(x) for x in (sys.argv[1:] or "0 35 40 0 35 40".split())]
COOL = 50
# clang (cargo's linker driver, ~/.cargo/config.toml) lives in the newest LLVM slot.
LLVM_BIN = sorted(glob.glob("/usr/lib/llvm/*/bin"), key=lambda d: int(d.split("/")[4]))[-1]


def rd(p):
    with open(p) as f:
        return f.read().strip()


def hw(name):
    for d in glob.glob("/sys/class/hwmon/hwmon*"):
        if rd(d + "/name") == name:
            return d


CORETEMP, THINKPAD = hw("coretemp"), hw("thinkpad")


def pkg_temp():
    return int(rd(CORETEMP + "/temp1_input")) / 1000


def pl1_path(base):
    for i in range(3):
        if rd(f"{base}/constraint_{i}_name") == "long_term":
            return f"{base}/constraint_{i}_power_limit_uw"


def sampler(stop, out):
    e0, t0 = int(rd(MSR + "/energy_uj")), time.time()
    while not stop.is_set():
        time.sleep(1)
        e1, t1 = int(rd(MSR + "/energy_uj")), time.time()
        mhz = [int(rd(p)) / 1000 for p in glob.glob("/sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_cur_freq")]
        out.append({"w": (e1 - e0) / 1e6 / (t1 - t0) if e1 >= e0 else None, "t": pkg_temp(),
                    "fan": int(rd(THINKPAD + "/fan1_input")) if THINKPAD else None,
                    "mhz": statistics.mean(mhz), "pl1": int(rd(pl1_path(MMIO))) / 1e6})
        e0, t0 = e1, t1


def build():
    subprocess.run(["rm", "-rf", TARGET])
    env = {"HOME": USER_HOME, "PATH": f"{USER_HOME}/.cargo/bin:/usr/local/bin:/usr/bin:/bin:" + LLVM_BIN, "CARGO_TARGET_DIR": TARGET, "RUSTC_WRAPPER": ""}
    probe = subprocess.Popen(["runuser", "-u", USER, "--", "python3", "-I", f"{S}/latprobe.py"],
                             stdout=subprocess.PIPE, text=True)
    t = time.time()
    r = subprocess.run(["runuser", "-u", USER, "--", "env", *[f"{k}={v}" for k, v in env.items()],
                        "cargo", "build", "--release", "--locked", "--offline", "--quiet"], cwd=SRC)
    secs = time.time() - t
    probe.terminate()
    build.latency = json.loads(probe.communicate()[0] or "{}")
    return secs, r.returncode


def main():
    if rd("/sys/class/power_supply/AC/online") != "1":
        sys.exit("plug in the charger first")
    profile = subprocess.run(["powerprofilesctl", "get"], capture_output=True, text=True).stdout.strip()
    subprocess.run(["powerprofilesctl", "set", "performance"])
    time.sleep(2)
    path = pl1_path(MMIO)
    original = rd(path)
    print(f"profile was {profile}; firmware's MMIO PL1 in performance: {int(original) / 1e6:.0f} W", flush=True)
    results = []
    try:
        print("warm-up build (not counted) ...", flush=True)
        secs, rc = build()
        if rc != 0:
            raise SystemExit(f"warm-up build failed (exit {rc}); stopping")
        print(f"  {secs:.0f} s", flush=True)
        for watts in LIMITS:
            while pkg_temp() > COOL:
                print(f"  cooling: {pkg_temp():.0f} °C", end="\r", flush=True)
                time.sleep(5)
            with open(path, "w") as f:  # 0: the firmware's own value
                f.write(str(watts * 1000000) if watts else original)
            stop, samples = threading.Event(), []
            th = threading.Thread(target=sampler, args=(stop, samples)); th.start()
            secs, rc = build()
            stop.set(); th.join()
            if rc != 0:
                raise SystemExit(f"build failed (exit {rc}); stopping")
            w = [s["w"] for s in samples if s["w"] is not None]
            r = {"pl1": watts, "secs": round(secs, 1), "rc": rc, "avg_w": round(statistics.mean(w), 1),
                 "max_t": max(s["t"] for s in samples), "avg_t": round(statistics.mean(s["t"] for s in samples), 1),
                 "max_fan": max((s["fan"] or 0) for s in samples), "avg_mhz": round(statistics.mean(s["mhz"] for s in samples)),
                 "pl1_seen": sorted({s["pl1"] for s in samples}), "latency": build.latency}
            results.append(r)
            print(json.dumps(r), flush=True)
    finally:
        with open(path, "w") as f:
            f.write(original)
        subprocess.run(["powerprofilesctl", "set", profile or "balanced"])
        subprocess.run(["rm", "-rf", TARGET])
        print(f"restored MMIO PL1 {int(original) / 1e6:.0f} W and profile {profile}", flush=True)
    print("\nsummary (median per limit):")
    for watts in sorted(set(LIMITS)):
        rs = [r for r in results if r["pl1"] == watts]
        print(f"  {watts or 'firmware'} W: {statistics.median(r['secs'] for r in rs):.1f} s, "
              f"{statistics.median(r['avg_w'] for r in rs):.1f} W avg, max {max(r['max_t'] for r in rs):.0f} °C, "
              f"fan max {max(r['max_fan'] for r in rs)}, wakeup p99 "
              f"{statistics.median(r['latency'].get('p99_us', 0) for r in rs):.0f} us")


if __name__ == "__main__":
    main()
