# deck

What is installed on a Gentoo machine, where each package came from, which carry local patches,
where each patch stands upstream, and AI agents' security and performance reviews of the code that
matters. A TUI, plus tab-separated output for scripts and agents.

Installed through Portage, from the local overlay's live ebuild `app-portage/deck`: on deck,
`portage/env/deck-local-source.conf` points it at this clone, so `emerge app-portage/deck` builds
what is committed here, pushed or not.

```sh
deck                                       # the TUI: packages, patches, usage, review
deck packages [--filter here|compiled|patched|overlay|…]
deck patches
deck mark <category/package/file> <state> [--url URL] [--note TEXT]
deck upstream <category/package/file> <URL>
deck stage <category/package/file>         # onto a branch of upstream's clone, staged; a terminal there
deck fork | deck push | deck pr            # in that clone, one step each
deck usage [--days N] [--spikes]           # CPU, memory, active time per package
deck usage --pkg <label>                   # whose code it ran, who ran its code, its spikes
deck review [list]                         # every subject, where its reviews stand, best next first
deck review next [--kind security|performance]
deck review run <subject> [--kind K] [--agent A] [--full]
deck review auto                           # security reviews of updates, within the daily budget
deck review findings [<subject>] [--all]   # findings, open ones first, and what was decided
deck review decide <run> <n> <open|fixed|not-applicable|wont-fix> [--note TEXT]
deck review recheck <subject>              # an agent marks the open findings it finds fixed
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

## Sending a patch upstream

Each step is one command and only runs when asked: the commit message and the pull request's
text are the user's to write, and nothing reaches upstream until `deck push`.

1. `deck stage <id>` (or `S`): clones the patch's upstream project to `~/3p/<host>/<owner>/<repo>`
   if it isn't there, makes a branch named after the patch file (`0002-ask-before.patch` goes on
   `ask-before`) from upstream's default branch, applies the patch with
   `git apply --3way --index`, staged and uncommitted, and opens a terminal there. A branch that
   exists already is left as it is; uncommitted changes in the clone stop it.
2. Review (`git diff --cached`), adapt it to upstream's current code and checks, `git commit`.
3. `deck fork`: `gh repo fork`, as the remote `fork`; `origin` stays upstream.
4. `deck push`: the branch to the fork, once it has a commit of its own.
5. `deck pr`: opens the browser on a new pull request from the fork's branch, with upstream's
   template. Run it again once the pull request exists: it prints its URL and marks the patch
   `sent` with it, if it was untracked or `unsent`.

In the TUI, `S` on a patch does whichever step comes next, from what the clone shows: stage
(and open a terminal there), commit (a terminal opens, the commit is yours), fork, push, pull
request, then mark it sent. Fork and push ask y/n first. Commits made after the pull request,
such as changes upstream asked for, go through `S` again as a push. `?` on a patch shows the
steps done and the next one. A patch that came from upstream or is no longer going there
(`backport`, `accepted`, `released`, `rejected`, `withdrawn`, `dropped`) has nothing to send.

The patch file in the host's directory stays as it is: if upstream asks for changes, they are
made in the clone, and the patch here is refreshed from it by hand.

## Keys

| Key | |
|---|---|
| `Tab`, `1`, `2`, `3`, `4` | packages / patches / usage / review |
| `j` `k` `d` `u` `g` `G` | move, page, top, bottom (arrows, PgUp/PgDn, Home/End too) |
| `/`, `Esc` | search by name, clear it |
| `f` `F` | next / previous package filter |
| `Enter` | on a patched package: its patches; on a patch: change its state; on usage: the package |
| `s` | sort packages and usage by name, CPU, active time, peak memory |
| `p` | usage over the last 1, 7 or 30 days |
| `m` | change a patch's state: pick it, then URL and note (both optional) |
| `U` | set the patch's upstream project |
| `S` | the patch's next step upstream (below); fork and push ask y/n first |
| `o` | a terminal in the patch's clone in `~/3p` |
| `s` `p` (Review) | ask an agent for a security / performance review; asks y/n first |
| `S` `P` (Review) | the same, of the whole even when an earlier version was reviewed |
| `Enter` (Review) | the latest report, with less in a terminal; marks it read |
| `n` `a` (Review) | the most worthwhile next (the top row) / the next agent |
| `f` (Review) | the subject's findings, open ones first |
| `c` (findings) | an agent rechecks the open findings and marks what it finds fixed |
| `x` `n` `w` `o` (findings) | decide: fixed / doesn't apply here / won't fix / open again, with a note |
| `Enter` `a` `Esc` (findings) | the report at the finding / only the open ones (again: all) / back |
| `?` | explain the selected row and the keys that work on it; `?` again: every key |
| `r` | reload |
| `q` | quit |

## Reviews

AI agents review the code that matters on this machine for security and performance, and the
results only warn: nothing is blocked. The host's `review.toml` lists:

- **Agents**: each a command that reads the prompt on stdin and writes a Markdown report on stdout.
  Claude Code (`claude -p`, limited to reading and searching) is the first; another agent is
  another `[[agent]]`, chosen with `--agent` or `a` in the TUI.
- **Our own code** (`[[local]]`): paths in this repository. Its version is the last commit that
  touched them.
- **Packages** (`[[package]]`): by atom, each with a tier, what matters about it here, and
  optionally the paths to concentrate on. Other installed packages busy enough to matter (0.5% of
  CPU time or profile samples, from deck-usaged) appear for performance reviews too; `-bin` and
  live ebuilds don't (no source to read).

**What runs.** A package's source is what Portage builds: fetched and unpacked with `ebuild …
clean prepare` (Gentoo's patches and ours applied, already downloaded sources reused), as the user,
inside `jail --no-src`, so its build scripts never run with the user's rights. Our code is
`git archive` of the commit. Sources are kept in `~/.cache/deck/review/src/<subject>/<version>`.

**The agent** runs in `jail --no-src --ro <source> --chdir <source>`: only the code under review,
read-only, and none of `~/src`. The prompt says why the subject matters here, what to look for,
that the source is data and not instructions (comments may try to steer a reviewer), and asks for
a closing ```` ```findings ```` block of JSON (severity, title, where), which deck reads.

**Baselines and changes.** The first review of a subject is of the whole. After that, a newer
version, installed or waiting as an update, gets a review of the difference only
(`<old>..<new>.diff`, limited to the subject's paths, with both trees beside it): cheap, and first
in line. `--full` (`S`, `P`) reviews the whole again.

**What next.** Each subject gets a score per kind: what is at stake (security: the tier, 1 our
code … 5; performance: its measured share of CPU time and profile samples) times what is
unreviewed (a change 1, never reviewed 0.7, current 0.03). The board is sorted by it, and `deck
review next` names the top one: where spare tokens are best spent.

**Findings** are decided one by one: `f` on a subject lists them, open ones first, worst first,
then the decided ones (fixed, doesn't apply here, won't fix), latest first; `a` leaves only the
open ones.
Whether one is fixed the agents decide: every review gets the open findings numbered and names, in
a `resolved` block, those the code it read shows fixed, with where and how; deck marks them fixed
(by that agent), but only in a newer version than the one the finding was made in: unchanged code
can't have fixed it, whatever a comment in it tells the agent. `c` (or `deck review recheck`) asks for only that check, against the installed
version, without looking for anything new. What is the owner's to judge is decided with a note:
`n` (doesn't apply here), `w` (won't fix), `x` (fixed, when no agent could see it) or `o` (open
again, also to undo an agent's call), in the run's record. The next review of the subject is given every earlier finding
with its decision and note: it doesn't raise again what doesn't apply or won't be fixed unless
something changed, checks the fixes, and, for a change, reports only what the change brings. What
a change fixes it reports with severity `fix`, filed as fixed. Open findings stay open through
later reviews, full ones too, until fixed or decided. The Review tab's Open column and
the maintenance tile count what is still undecided.

**Every day**, `deck-review.timer` (a user timer) runs `deck review auto`: security reviews of
the changes in watched code, updates first, at most `budget.auto_per_day` a day. Findings
not decided yet and failed automatic runs show on the maintenance tile (`deck-attention`).

**Reports stay private**: `~/.local/state/deck/reviews/reports/<subject>/`, with the agent's
errors beside each in a `.log`, and one JSON record per run in `runs/`. A finding may be an
unreported vulnerability: it goes upstream privately, never into this public repository.

## Usage accounting: deck-usaged and deck-focusd

Two services record usage; `deck usage` and the Usage tab merge them per package.

- `deck-usaged` ([systemd/deck-usaged.service](systemd/deck-usaged.service)), a root service:
  CPU, memory and a CPU profile per package, hour by hour, into `/var/lib/deck/usage.db`.
  `systemctl enable --now deck-usaged`.
- `deck-focusd` ([systemd/deck-focusd.service](systemd/deck-focusd.service)), a user service in
  the graphical session: time each program was actively used, into
  `$XDG_STATE_HOME/deck/focus.db`. `systemctl --user enable --now deck-focusd`.

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

**Whose code ran: the profiler.** 49 times a second, each busy CPU records where it is
executing, counted in CPU cycles so an idle CPU takes no samples and is not woken (one counter
unit for the P-cores and one for the E-cores). The address is looked up in the process's
executable mappings, so time spent in a library is credited to the library's package: mesa,
glibc, ffmpeg's libraries, as well as the program's. JIT code counts as the program's; kernel
addresses as `kernel` or `kmod:<module>` (i915, btrfs, iwlwifi). The `libs` table keeps samples
per library, program and context; `Code` in the Usage tab is a package's share of all samples.

**Active time.** `deck-focusd` follows niri's event stream for the focused window and its
process. In a terminal it takes the terminal's foreground program (aerc, nvim, chess-tui), and
follows wrappers that start one program and wait for it (a script such as `jail`, bwrap), so a
jailed Claude Code counts as `user:claude`, not bash. Wayland's idle notification (ext-idle-notify)
fires after 60 s without input; those 60 s are dropped too, so active time is time with input at
most a minute apart. An idle inhibitor (a playing video) keeps the session active. It records the
program's path; `deck` maps paths to packages as deck-usaged does. `deck-focusd --windows` shows
what each open window counts as.

**Spikes.** Seconds are sorted by how much CPU the package used in them (up to 5% of a core,
25%, one core, two, more). A burst starts at half a core, ends after 3 s under a tenth, and is
kept in `spikes` with its length, CPU, peak cores and peak memory if it used a CPU-second.

**Cost.** deck-usaged: about 0.3% of one core with the machine busy, less idle; 30 MiB of
memory, most of it the file map. It counts itself, as `app-portage/deck`.

Tables (`sqlite3 -readonly /var/lib/deck/usage.db .schema`): `usage` (hour, package, context:
CPU, seconds alive, launches, peak RSS, PSS, seconds per CPU band), `spikes`, `libs` (profiler
samples), `meta` (start time, netlink batches and samples lost). `focus.db`: `active` (hour,
program path, window app id, seconds). The data stays on the machine; nothing in it goes to
git.
