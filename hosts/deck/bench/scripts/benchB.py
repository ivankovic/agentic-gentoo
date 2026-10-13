#!/usr/bin/env python3
"""Scheduler and huge-page benchmark: the same omnidiff release build under
  base  EEVDF, transparent_hugepage=madvise (deck's default)
  thp   transparent_hugepage=always
  lavd  sched_ext scx_lavd
at the firmware's power limit. Reuses plbench.py's build, sampler and latency probe; needs scx_lavd
(sys-kernel/scx). As root: sudo BENCH_SRC=/path/to/rust/project python3 -I benchB.py [CONFIG...]"""
import importlib.util, json, os, signal, statistics, subprocess, sys, threading, time

S = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("plbench", f"{S}/plbench.py")
pb = importlib.util.module_from_spec(spec); spec.loader.exec_module(pb)

THP = "/sys/kernel/mm/transparent_hugepage/enabled"
SCX_STATE = "/sys/kernel/sched_ext/state"
CONFIGS = sys.argv[1:] or "base thp lavd base thp lavd".split()


def thp_mode():
    return pb.rd(THP).split("[")[1].split("]")[0]


def set_thp(mode):
    with open(THP, "w") as f:
        f.write(mode)


def run(config):
    lavd = None
    if config == "thp":
        set_thp("always")
    if config == "lavd":
        lavd = subprocess.Popen(["scx_lavd"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(50):
            if pb.rd(SCX_STATE) == "enabled":
                break
            time.sleep(0.1)
        else:
            lavd.terminate()
            raise SystemExit("scx_lavd didn't attach")
    try:
        stop, samples = threading.Event(), []
        th = threading.Thread(target=pb.sampler, args=(stop, samples)); th.start()
        secs, rc = pb.build()
        stop.set(); th.join()
        state = pb.rd(SCX_STATE)
    finally:
        if lavd:
            lavd.send_signal(signal.SIGINT); lavd.wait(timeout=10)
        set_thp("madvise")
    if rc != 0:
        raise SystemExit(f"{config}: build failed (exit {rc}); stopping")
    w = [s["w"] for s in samples if s["w"] is not None]
    return {"config": config, "secs": round(secs, 1), "avg_w": round(statistics.mean(w), 1),
            "avg_t": round(statistics.mean(s["t"] for s in samples), 1), "max_t": max(s["t"] for s in samples),
            "avg_mhz": round(statistics.mean(s["mhz"] for s in samples)), "scx_during": state,
            "latency": pb.build.latency}


def main():
    if pb.rd("/sys/class/power_supply/AC/online") != "1":
        sys.exit("plug in the charger first")
    if thp_mode() != "madvise" or pb.rd(SCX_STATE) != "disabled":
        sys.exit(f"unexpected start state: thp={thp_mode()} scx={pb.rd(SCX_STATE)}")
    profile = subprocess.run(["powerprofilesctl", "get"], capture_output=True, text=True).stdout.strip()
    subprocess.run(["powerprofilesctl", "set", "performance"])
    results = []
    try:
        print("warm-up build (not counted) ...", flush=True)
        secs, rc = pb.build()
        if rc != 0:
            raise SystemExit(f"warm-up build failed (exit {rc}); stopping")
        print(f"  {secs:.0f} s", flush=True)
        for config in CONFIGS:
            while pb.pkg_temp() > pb.COOL:
                print(f"  cooling: {pb.pkg_temp():.0f} °C", end="\r", flush=True)
                time.sleep(5)
            r = run(config)
            results.append(r)
            print(json.dumps(r), flush=True)
    finally:
        set_thp("madvise")
        subprocess.run(["powerprofilesctl", "set", profile or "balanced"])
        subprocess.run(["rm", "-rf", pb.TARGET])
        print(f"restored: thp={thp_mode()} scx={pb.rd(SCX_STATE)} profile={profile}", flush=True)
    print("\nsummary (median per config):")
    for config in dict.fromkeys(CONFIGS):
        rs = [r for r in results if r["config"] == config]
        if not rs:
            continue
        med = lambda k: statistics.median(r["latency"].get(k, 0) for r in rs)
        print(f"  {config:5} {statistics.median(r['secs'] for r in rs):6.1f} s  "
              f"{statistics.median(r['avg_w'] for r in rs):.1f} W  {statistics.median(r['avg_t'] for r in rs):.0f} °C avg  "
              f"wakeup late p50 {med('p50_us'):.0f} us, p99 {med('p99_us'):.0f} us, p99.9 {med('p999_us'):.0f} us, "
              f"max {max(r['latency'].get('max_us', 0) for r in rs)} us")


if __name__ == "__main__":
    main()
