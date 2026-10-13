# Power limit, scheduler and huge pages — deck, 2026-10-13

What limits sustained performance on deck, and whether anything in software or firmware
settings moves it. Answer: cooling. At full fan the chassis sheds about 26 W; none of a higher
power limit, sched_ext `scx_lavd` or `transparent_hugepage=always` made builds faster.

## Setup

- Workload: clean `cargo build --release --locked` of omnidiff `3ac22e19` (fat LTO, one codegen
  unit for the binary), rustc 1.99.0, linked with mold via clang, no sccache (`RUSTC_WRAPPER=`).
  One warm-up build first; each run starts with the CPU package below 50 °C.
- On AC, power-profiles-daemon `performance`, room temperature, laptop flat on a desk.
- Measured once a second: package power (RAPL), package temperature, fan, mean core clock, the
  MMIO PL1 in force. Alongside each build, an ordinary process sleeping 2 ms in a loop records
  how late it wakes up (a stand-in for desktop responsiveness under load).
- Scripts: [scripts/plbench.py](scripts/plbench.py) (power limits),
  [scripts/benchB.py](scripts/benchB.py) (scheduler and huge pages),
  [scripts/latprobe.py](scripts/latprobe.py) (wakeup latency).

## Power limits

Lenovo's firmware sets the sustained limit (MMIO PL1, `intel-rapl-mmio:0`) per platform profile:
14 W in balanced on battery, 30 W in performance on AC. The MSR copy (`intel-rapl:0`) says 55 W
and isn't the one in force. Two runs per limit, medians:

| Sustained limit | Build | Package power | Temp avg / max | Fan |
|---|---|---|---|---|
| firmware (30 W) | 105.2 s | 26.5 W | 85.7 / 100 °C | 4,700 RPM |
| 35 W | 105.3 s | 26.1 W | 85.4 / 100 °C | 4,696 RPM |
| 40 W | 105.5 s | 26.1 W | 85.4 / 97 °C | 4,692 RPM |

Power never reaches the limit: the CPU sits at 96–100 °C with the fan at its maximum, and during
every run the firmware itself dropped PL1 to 21 W for a while, overriding the value written. Earlier
vitals samples at ~21 W and 68 °C were from lower profiles, not performance.

## Scheduler and huge pages

At the firmware's limit, two runs each, medians:

| Setup | Build | Wakeup late p50 | p99 | p99.9 | max |
|---|---|---|---|---|---|
| EEVDF, THP madvise (deck's default) | 105.7 s | 0.16 ms | 0.32 ms | 3.7 ms | 11.4 ms |
| THP always | 106.9 s | 0.16 ms | 0.43 ms | 4.4 ms | 32.3 ms |
| sched_ext `scx_lavd` (scx 1.1.2) | 107.1 s | 0.17 ms | 4.56 ms | 5.8 ms | 9.4 ms |

Neither helps: build times are within noise or slower, and the latency tail gets worse (THP's
worst wakeup tripled; lavd's p99 went from 0.3 to 4.6 ms). Both stay off; scx was removed.

## Not yet tried

Better cooling: a fan under the laptop blowing into the intake. If it lets power reach the 30 W
limit, re-run the power limits above with it in place.
