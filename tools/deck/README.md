# deck

What is installed on a Gentoo machine, where each package came from, which carry local patches,
and where each patch stands upstream. A TUI, plus tab-separated output for scripts and agents.

```sh
cargo install --path tools/deck --locked --bin deck   # → ~/.cargo/bin/deck
deck                                       # the TUI
deck packages [--filter here|compiled|patched|overlay|…]
deck patches
deck mark <category/package/file> <state> [--url URL] [--note TEXT]
deck upstream <category/package/file> <URL>
deck usage [--days N] [--spikes]           # CPU and memory per package, from deck-usaged
```

It reads the host's directory, `~/src/agentic-gentoo/hosts/$(hostname)` unless `--host-dir` or
`DECK_HOST_DIR` says otherwise. Nothing is cached: every start reads the system afresh.

## Origin

From Portage's database in `/var/db/pkg`:

| Origin | Meaning | How it is told |
|---|---|---|
| `binhost` | binary package from the binhost | `BINPKGMD5` or `BUILD_ID` recorded |
| `compiled` | built here, the build made machine code | an installed file is ELF, PE/EFI, LLVM bitcode or SPIR-V (firmware and docs aside) |
| `prebuilt` | built here from upstream's binaries | the package name ends in `-bin` |
| `noarch` | built here, no machine code | none of the above: scripts, data, fonts, firmware |

`Eco` names the ecosystem a package vendors from (`rust`, `go`, `python`, `ruby`), read off the
eclasses it inherits.

## Patches

Patch files in the host's `portage/patches/` (user patches, deployed to `/etc/portage/patches`)
and in `overlay/<cat>/<pkg>/files/`. A patch's id is `category/package/file`, so it keeps its
history when the version directory is renamed on a bump. `Applies to` is the installed package
the directory matches the way `eapply_user` matches it (PN, P or PF, with an optional `:slot`);
`(no match)` means it applies to nothing installed. `Deployed` compares the file with the
system's copy.

Upstream state lives in the host's `upstream.toml`: one `[[patch]]` per patch, one
`[[patch.event]]` per change of state, each with a timestamp and an optional URL and note. The
last event is the current state. A patch removed from the repository stays listed, as
`(file gone)`, so its history is kept.

| State | Meaning |
|---|---|
| `unsent` | ours, not offered upstream yet |
| `sent` | offered upstream, no response yet |
| `in-review` | upstream is reviewing it |
| `changes-requested` | upstream asked for changes |
| `accepted` | merged upstream, not in a release yet |
| `released` | in an upstream release; drop it on the next bump |
| `backport` | taken from upstream; drop it on the next bump |
| `rejected` | upstream said no for good; ours to carry |
| `withdrawn` | we took it back |
| `dropped` | no longer carried here |

The first five count as open.

## Keys

| Key | |
|---|---|
| `Tab`, `1`, `2` | packages / patches |
| `j` `k` `d` `u` `g` `G` | move, page, top, bottom (arrows, PgUp/PgDn, Home/End too) |
| `/`, `Esc` | search by name, clear it |
| `f` `F` | next / previous package filter |
| `Enter` | on a patched package: its patches; on a patch: change its state |
| `m` | change a patch's state: pick it, then URL and note (both optional) |
| `U` | set the patch's upstream project |
| `r` | reload |
| `q` | quit |

## Usage accounting: deck-usaged

A root service (`hosts/deck/etc/systemd/system/deck-usaged.service`) that records CPU and memory
per package into `/var/lib/deck/usage.db`, hour by hour; `deck usage` reads it. It is the second
binary in this crate and must be root-owned, so it is not installed with cargo:

```sh
cargo build --release --manifest-path tools/deck/Cargo.toml
sudo install -o root -g root -m 0755 tools/deck/target/release/deck-usaged /usr/local/sbin/
./deploy.sh deck apply && sudo systemctl daemon-reload && sudo systemctl enable --now deck-usaged
```

**CPU.** Each process's CPU counter (`/proc/<pid>/stat`, all threads) is read once a second
and the increase credited to its package. Only processes in cgroups whose CPU total moved are
read; all of them, kernel threads included, every 10 s. The kernel's process events (proc
connector) name a program when it starts, and its exit reports (taskstats) give the whole CPU
time of every thread, so a process too short for a reading is still counted in full. A process
older than the daemon is counted from the daemon's start.

**Which package.** The program's path (`/proc/<pid>/exe`) looked up among every file in
`/var/db/pkg/*/*/CONTENTS`, with directories resolved (`/bin` is `/usr/bin` here). For an
interpreter (python, perl, ruby, sh, bash, lua, node) the script it runs is looked up instead.
Programs no package owns get a label from where they live: `user:claude`, `user:rustup:rustc`,
`user:build:<crate>` (cargo's `target/`), `local:<file>` (`/usr/local`), `tmp:`, and
`container:<cgroup>` for a program from another root filesystem. Kernel threads are
`kernel:<family>` (`kworker`, `kswapd`, `irq-iwlwifi`). The map is rebuilt after every merge.

**Context.** `build` for anything in `build.slice` or started by Portage, `jail` for anything
under `jail`'s bwrap, both inherited by children; otherwise `user` or `system` from the cgroup,
and `kernel`.

**Memory.** Every 10 s each process's peak RSS since the last reading (`VmHWM`, then reset
through `clear_refs`), summed per package: the peak catches a spike however short. Every 5 min
PSS (`smaps_rollup`), which splits shared pages fairly, for the steady state. Exit reports add
the peak of processes that never got a reading.

**Spikes.** Seconds are sorted by how much CPU the package used in them (up to 5% of a core,
25%, one core, two, more). A burst starts at half a core, ends after 3 s under a tenth, and is
kept in `spikes` with its length, CPU, peak cores and peak memory if it used a CPU-second.

**Cost.** About 0.3% of one core with the machine busy, less idle; 30 MiB of memory, most of it
the file map.

Tables (`sqlite3 -readonly /var/lib/deck/usage.db .schema`): `usage` (hour, package, context:
CPU, seconds alive, launches, peak RSS, PSS, seconds per CPU band), `spikes`, `meta` (start
time, netlink batches lost to overflow). The data stays on the machine; nothing in it goes to
git.
