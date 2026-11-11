# fix

An offline sysadmin model: a small local model that installs and removes software, reads logs and
repairs a broken system without the network, from the running system or from the recovery
partition. The model is any GGUF that llama.cpp serves; nothing here is tied to one.

How it gets there:

1. **Drills** ([tools/drills](../drills/)): throwaway VMs, each broken in a known way, with a check
   that decides whether a repair worked. They measure a model, and the repairs that pass are its
   training data.
2. **Shadow** (`fix-shadow`, here): next to the main agent on the laptop, the local model is asked
   at each of the main agent's sysadmin commands what it would have run, and both are recorded.
3. **Training** on core, from drill repairs that passed.
4. **Recovery**: the model and a `fix` command on the recovery partition, asking before each change.

## fix-shadow

```sh
fix-shadow run --serve ~/models/Qwen3.5-2B-Q5_K_M.gguf   # the last two days' sessions
fix-shadow run --model http://127.0.0.1:8080             # a model already being served
fix-shadow report                                        # agreement over the last 30 days
```

It reads the main agent's sessions through a **source** that turns its log into a neutral trace:
what the user asked, and each command with its output. `claude-code` reads Claude Code's
`~/.claude/projects`; another agent needs only another source. For every sysadmin command (systemd,
Portage, disks, users, processes, networking, ssh; by the programs the command line runs, not by
words in its arguments), the local model gets the user's last messages
and the commands run for the current request with their output, and names its next command.

Each comparison is one of: **same** (after dropping `sudo` and a leading `cd`), **similar** (same
program and subcommand: `systemctl restart`), **same-program**, **different**, or **none** (no
command). A different command is not necessarily a wrong one; `report` lists the latest
differences to read.

Results: `~/.local/state/fix/shadow.jsonl`. Commands already compared are skipped. A command the
model server failed on (an HTTP error, a prompt longer than its context) is not a comparison: it is
noted in `shadow-errors.jsonl` and asked again on later runs, three times at most.

**Only a measure.** The main agent's sessions are not used as training data: its provider's terms
may not allow that, and the measure stays honest only while the model has not seen them.

Installed by `app-admin/fix` (the deck overlay) with a user timer that runs it nightly:

```sh
systemctl --user enable --now fix-shadow.timer
```

The model is `FIX_MODEL` in `fix-shadow.service` (default `~/models/Qwen3.5-2B-Q5_K_M.gguf`);
another goes in a drop-in.
