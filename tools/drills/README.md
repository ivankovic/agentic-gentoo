# drills

Breakage drills for a sysadmin model: a fresh Gentoo machine is broken in a known way, a model
repairs it through a root shell, and a script checks the result. They measure how much of the
everyday administration (installing, removing, services, logs, disks, users, Portage, boot) a
model gets right, and they are where its training data comes from: only repairs that pass the
check are kept.

The model is whatever serves an OpenAI-compatible chat API with tool calls (llama-server
`--jinja`, for one); nothing here is tied to a particular model or provider.

## The machine

A QEMU/KVM virtual machine: Gentoo systemd stage3, a dist kernel booted directly, sshd, the
Gentoo repository, and the binary packages (or sources) the drills install, cached, because it
has **no network** (user-mode networking with `restrict=on`; only the runner's SSH port forward
reaches it). Each drill boots from a copy-on-write overlay of the same base image and is thrown
away after.

```sh
image/build.sh <dir>          # Docker, no root: <dir>/base.raw, vmlinuz, initramfs, key
```

Needs Docker, QEMU and `/dev/kvm`. The packages cached for the drills are listed in
[image/packages.txt](image/packages.txt).

## Running

```sh
./run.py --image <dir> --reference                     # every drill: broken, fails its check;
                                                       # its fix.sh, passes it
./run.py --image <dir> --model http://host:8080 --label qwen3.5-2b [drill ...]
./run.py --image <dir> --model http://host:8080 --label qwen3.5-2b-think --think
./run.py --image <dir> --reference --variants 5 --seed 100  # five variants of each, seeds 100-104
```

Results go to `<dir>/results/<label>-<time>.jsonl`, one line per drill and variant: pass, fail,
invalid (the drill itself is wrong) or error, with the variant's seed and variables, the task, the
model's transcript, its final answer and token counts.

The model gets one tool, `run` (a bash command as root, at most 5 minutes, 3,000 characters of
output: head and tail), the system prompt in [run.py](run.py), and the drill's task, worded the way a
user would report it. It stops when it answers without a tool call, after 30 commands, or when the
conversation no longer fits its context or the server cannot parse its tool call; the check
decides pass or fail either way. Earlier turns' thinking is kept in the transcript but not sent
back to the model.

## Drills

One directory per drill under [drills/](drills/):

| File | |
|---|---|
| `drill.toml` | `category`, `task` (what the user says; a list of phrasings to pick from), `[vars]`, optionally `reboot_after_break`, `reboot_before_check` |
| `break.sh` | breaks the machine; run as root, then deleted |
| `check.sh` | exit 0 when fixed; copied in only to check. The model's final answer is in `/run/drill/answer` |
| `fix.sh` | the reference repair, which proves the drill can be passed |

### Variants

Each drill is a family: its `[vars]` pick, by seed, what this variant breaks and how. A var is a
list (one value is picked) or `"@pool"`, a row of [pools.toml](pools.toml) (packages, users, web
services, daemons; a different row for each var drawing from the same pool), exported as
`VAR_FIELD`. The scripts get the variables in their environment and the task is a template over
them, so `install-package` with `PKG = "@packages"` asks for a different package, by any of its
phrasings, in each variant. A variant is reproducible from the drill's name and seed.

[lib.sh](lib.sh) is put into every script after its `#!` line: helpers for web units, package
state, files that must not change, and checking the final answer.

A check tests the outcome, not the commands: any repair that works passes. Checks also test that
nothing else was damaged (other packages, the user's files, unrelated configuration), and for
questions, that nothing was changed at all.
