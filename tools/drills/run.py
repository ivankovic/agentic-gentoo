#!/usr/bin/env python3
"""Breakage drills: break a fresh drill machine, let a model (or the reference fix) repair it, check.

    run.py --image DIR --reference [DRILL ...]       validate drills: each must fail its check once
                                                     broken and pass after its fix.sh
    run.py --image DIR --model URL [--label NAME] [--think] [DRILL ...]
                                                     a model behind an OpenAI-compatible chat API
                                                     (llama-server --jinja) repairs each drill
    --variants N --seed S                            N variants of each drill, from seed S on

DIR is build.sh's output. Each drill is a directory under drills/: drill.toml (category, task,
optional reboot_before_check), break.sh, check.sh, fix.sh. The machine boots from a copy-on-write
overlay of base.raw, so every drill starts from the same state; it has no network.

Results go to DIR/results/<label>-<time>.jsonl, one line per drill with the transcript.
"""
import argparse
import json
import random
import shlex
import socket
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path
from string import Template

import modelchat

HERE = Path(__file__).resolve().parent
OUTPUT_LIMIT = 3000   # characters of a command's output the model sees: the head and the tail
COMMAND_TIMEOUT = 300
KILL_AFTER = 10       # seconds the machine's own timeout waits between TERM and KILL
MAX_STEPS = 30
# Printed after a drill's script with its exit status: the status of the ssh command is then the
# harness's alone (ssh's 255, the time limit's 124), and the script's own may be any number.
STATUS_MARK = "[drill script status] "

SYSTEM_PROMPT = """\
You are the system administrator of a Gentoo Linux machine (systemd, Portage). You have a root \
shell through the `run` tool. The machine has no network: emerge installs only from the local \
binary package cache.

How to work:
- Find the cause before changing anything: read the task, then the logs (journalctl, \
systemctl status), the configuration and the package state.
- Make the smallest change that fixes the cause. Never delete data you were not asked to delete.
- Commands must not wait for input: no editors or pagers. Write files with cat <<'EOF' or sed.
- Check that your fix worked.
When you are done, answer without calling a tool: say what was wrong and what you changed, in a \
few sentences. If the task is a question, answer it."""

TOOLS = [{
    "type": "function",
    "function": {
        "name": "run",
        "description": "Run a shell command as root on the machine and return its output and exit code.",
        "parameters": {
            "type": "object",
            "properties": {"command": {"type": "string", "description": "a bash command line"}},
            "required": ["command"],
        },
    },
}]


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Machine:
    """One drill machine: QEMU with KVM, booted from an overlay of the base image."""

    def __init__(self, image: Path, work: Path):
        self.image, self.work = image, work
        self.port = free_port()
        self.overlay = work / "overlay.qcow2"
        subprocess.run(["qemu-img", "create", "-q", "-f", "qcow2", "-b", str(image / "base.raw"),
                        "-F", "raw", str(self.overlay)], check=True)
        self.qemu = None

    def boot(self):
        self.qemu = subprocess.Popen([
            "qemu-system-x86_64", "-enable-kvm", "-cpu", "host", "-smp", "2", "-m", "3072",
            "-kernel", str(self.image / "vmlinuz"), "-initrd", str(self.image / "initramfs"),
            "-append", "root=LABEL=drill rw console=ttyS0 quiet",
            "-drive", f"file={self.overlay},if=virtio,format=qcow2",
            # restrict=on: no traffic leaves the machine; only the SSH port forward comes in.
            "-nic", f"user,model=virtio-net-pci,restrict=on,hostfwd=tcp:127.0.0.1:{self.port}-:22",
            "-display", "none", "-serial", f"file:{self.work / 'console.log'}",
        ], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=(self.work / "qemu.log").open("w"))
        self.wait_ssh()

    def wait_ssh(self, timeout=180):
        end = time.time() + timeout
        while time.time() < end:
            if self.qemu.poll() is not None:
                raise RuntimeError("QEMU exited: " + (self.work / "qemu.log").read_text().strip()[-300:])
            if self.ssh("true", timeout=10).returncode == 0:
                return
            time.sleep(2)
        raise TimeoutError("no SSH")

    def ssh(self, command, timeout=COMMAND_TIMEOUT, stdin=None):
        # The time limit is the machine's own (timeout ends the command and what it started there,
        # with status 124), so a command that ran out of time is not still running during the next
        # one. The limit on ssh itself is only for a machine that no longer answers.
        args = ["ssh", "-q", "-i", str(self.image / "key"), "-p", str(self.port),
                "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
                "-o", "ConnectTimeout=5", "-o", "BatchMode=yes", "root@127.0.0.1",
                "TERM=dumb PAGER=cat SYSTEMD_PAGER=cat SYSTEMD_COLORS=0 "
                f"timeout -k {KILL_AFTER} {timeout} bash -lc " + shlex.quote(command)]
        try:
            return subprocess.run(args, input=stdin, capture_output=True, text=True, errors="replace",
                                  timeout=timeout + KILL_AFTER + 15)
        except subprocess.TimeoutExpired as e:
            out = (e.stdout or b"").decode(errors="replace") if isinstance(e.stdout, bytes) else (e.stdout or "")
            return subprocess.CompletedProcess(args, 124, out, f"timed out after {timeout} s")

    def put(self, path: str, content: str, mode="700"):
        r = self.ssh(f"install -D -m {mode} /dev/stdin {shlex.quote(path)}", stdin=content)
        if r.returncode != 0:
            raise HarnessError(f"cannot write {path}: exit status {r.returncode}: {r.stderr.strip()[-200:]}")

    def script(self, path, exports="", shell=""):
        """Runs one of a drill's scripts, put at path, and removes it. The result carries the
        script's own exit status, the caller's to judge; a script that is not there or did not
        run to its end (ssh failed, the time limit) is the harness's failure."""
        q = shlex.quote(path)
        r = self.ssh(f"if [ ! -x {q} ]; then echo {shlex.quote(STATUS_MARK + 'missing')}; exit 0; fi; "
                     f"{exports}{shell}{q}; s=$?; rm -f {q}; echo; echo {shlex.quote(STATUS_MARK)}$s")
        status, out = script_status(path, r.returncode, r.stdout, r.stderr)
        return subprocess.CompletedProcess(r.args, status, out, r.stderr)

    # What break.sh keeps for check.sh (lib.sh's remember, and the drills' own /var/lib/.drill-*
    # files) does not stay in the machine, where the repair could read or change it: the runner
    # takes it out after break.sh and puts its own copy back for a check.
    def take_state(self):
        r = self.ssh("set -o pipefail; cd /var/lib && find . -maxdepth 1 -name '.drill-*' -print0 | "
                     "tar --null -T - -czpf - | base64 -w0 && rm -rf /var/lib/.drill-*")
        if r.returncode != 0:
            raise HarnessError(f"cannot fetch the drill's state: exit status {r.returncode}: {r.stderr.strip()[-200:]}")
        return r.stdout

    def give_state(self, state):
        r = self.ssh("set -o pipefail; rm -rf /var/lib/.drill-*; base64 -d | tar -C /var/lib -xzpf -", stdin=state)
        if r.returncode != 0:
            raise HarnessError(f"cannot restore the drill's state: exit status {r.returncode}: {r.stderr.strip()[-200:]}")

    def reboot(self):
        self.ssh("systemctl reboot", timeout=10)
        time.sleep(5)
        self.wait_ssh()

    def stop(self):
        if self.qemu and self.qemu.poll() is None:
            self.qemu.kill()
            self.qemu.wait()


def clip(text):
    return modelchat.clip(text, OUTPUT_LIMIT)


class HarnessError(Exception):
    """The runner could not do its own part (copy a file in, run a script to its end): an error,
    not a verdict on the drill or on the model."""


def script_status(name, returncode, stdout, stderr=""):
    """A drill script's own exit status and its output, from what ssh returned for it. Raises
    HarnessError when that says nothing about the script: ssh or the time limit ended it (ssh's
    status is not 0, or the status line is missing) or the script was not there."""
    out, mark, status = stdout.rpartition(STATUS_MARK)
    status = status.strip()
    if returncode != 0 or not mark or not status.isdigit():
        why = "not there" if mark and status == "missing" else f"did not finish (ssh exit status {returncode})"
        raise HarnessError(f"{name} {why}: {clip(stdout + stderr)[-300:]}")
    return int(status), out.removesuffix("\n")


class ContextFull(Exception):
    """The conversation no longer fits the model's context: the model's limit, so a fail."""


class BadToolCall(Exception):
    """The server could not parse the model's tool call: the model's mistake, so a fail."""


def chat(url, messages, think):
    body = {"messages": messages, "tools": TOOLS, "temperature": 0.2, "max_tokens": 4096,
            "chat_template_kwargs": {"enable_thinking": think}}
    try:
        return modelchat.post(url, body, timeout=1800)
    except urllib.error.HTTPError as e:
        body = e.read().decode(errors="replace")
        if "exceeds the available context" in body or "exceed_context_size" in body:
            raise ContextFull() from None
        if "Failed to parse tool call" in body:
            raise BadToolCall() from None
        raise RuntimeError(f"HTTP {e.code}: {body[:300]}") from None


def tool_command(call):
    """The command of a `run` tool call. ValueError says what is wrong with a malformed one: no
    arguments, arguments that are not JSON, or not an object with a string "command"."""
    try:
        arguments = json.loads(call["function"]["arguments"])
    except (KeyError, TypeError, json.JSONDecodeError) as e:
        raise ValueError(f"{type(e).__name__}: {e}") from None
    if not isinstance(arguments, dict) or not isinstance(arguments.get("command"), str):
        raise ValueError('the arguments must be a JSON object with a string "command"')
    return arguments["command"]


def agent(machine, task, url, think, memory=None):
    """The model's loop: tool calls run on the machine until it answers without one.

    With a memory (memory.py), the latest message (the task, or the last command's output) carries
    notes from the most similar past repairs; only the latest, so they do not pile up."""
    messages = [{"role": "system", "content": SYSTEM_PROMPT}, {"role": "user", "content": task}]
    transcript = list(messages)
    tokens = {"prompt": 0, "completion": 0}
    history = []
    for _ in range(MAX_STEPS):
        notes = memory.hints(task, history) if memory else ""
        sent = messages
        if notes:
            sent = messages[:-1] + [{**messages[-1], "content": messages[-1]["content"] + "\n\n" + notes}]
        try:
            reply = chat(url, sent, think)
        except ContextFull:
            return "", transcript, tokens, "context"
        except BadToolCall:
            return "", transcript, tokens, "bad_call"
        usage = reply.get("usage", {})
        tokens["prompt"] += usage.get("prompt_tokens", 0)
        tokens["completion"] += usage.get("completion_tokens", 0)
        msg = reply["choices"][0]["message"]
        # Earlier turns' thinking is not sent back: the transcript keeps it, the context doesn't.
        transcript.append({k: v for k, v in msg.items() if k in ("role", "content", "tool_calls", "reasoning_content")})
        if notes:
            transcript[-1]["memory"] = notes
        messages.append({k: v for k, v in msg.items() if k in ("role", "content", "tool_calls")})
        calls = msg.get("tool_calls") or []
        if not calls:
            return msg.get("content") or "", transcript, tokens, False
        for call in calls:
            command = ""
            # A malformed call is the model's mistake: it gets the error back as the tool's result.
            try:
                command = tool_command(call)
            except ValueError as e:
                result = f"bad tool call: {e}"
            else:
                r = machine.ssh(command)
                result = clip(r.stdout + r.stderr) + f"\n[exit code {r.returncode}]"
            call_id = call.get("id", "") if isinstance(call, dict) else ""
            messages.append({"role": "tool", "tool_call_id": call_id, "content": result})
            transcript.append(messages[-1])
            history.append((command, result))
    return "", transcript, tokens, "steps"


def variant(meta, name, seed):
    """One variant of a drill: its task and the variables its scripts get, chosen by the seed.

    [vars] maps a name to a list (one value is picked) or to "@pool" (a row of pools.toml,
    different for each entry drawing from the same pool, exported field by field as NAME_FIELD).
    task may be a list of phrasings; it is a template over the same variables."""
    rng = random.Random(f"{name}:{seed}")
    pools = tomllib.loads((HERE / "pools.toml").read_text())
    taken = {}
    env = {}
    for var, spec in meta.get("vars", {}).items():
        if isinstance(spec, str) and spec.startswith("@"):
            pool = spec[1:]
            rows = [r for i, r in enumerate(pools[pool]) if i not in taken.setdefault(pool, set())]
            i = pools[pool].index(rng.choice(rows))
            taken[pool].add(i)
            env.update({f"{var}_{k.upper()}": str(v) for k, v in pools[pool][i].items()})
        else:
            env[var] = str(rng.choice(spec))
    task = meta["task"]
    if isinstance(task, list):
        task = rng.choice(task)
    return Template(task.strip()).substitute(env), env


def script(drill, name):
    """A drill's script with lib.sh put in after its #! line."""
    first, _, rest = (drill / name).read_text().partition("\n")
    return f"{first}\n{(HERE / 'lib.sh').read_text()}\n{rest}"


def image_info(image: Path):
    """What build.sh recorded about the image (the base images' digests, the build date), or None
    for an image built before it did."""
    try:
        return json.loads((image / "image.json").read_text())
    except (OSError, json.JSONDecodeError):
        return None


def run_drill(drill: Path, args, seed):
    rec = {"drill": drill.name, "label": args.label, "seed": seed, "image": args.image_info}
    try:
        meta = tomllib.loads((drill / "drill.toml").read_text())
        task, env = variant(meta, drill.name, seed)
        rec.update(category=meta["category"], vars=env, task=task)
    except Exception as e:  # a drill that cannot be read is reported, and the run goes on
        return {**rec, "result": "error", "why": f"drill.toml: {type(e).__name__}: {e}"}
    # The scripts run with the variant's variables in their environment.
    exports = "export " + " ".join(f"{k}={shlex.quote(v)}" for k, v in env.items()) + "; " if env else ""
    t0 = time.time()
    with tempfile.TemporaryDirectory(prefix=f"drill-{drill.name}-", dir=args.image) as tmp:
        m = Machine(args.image, Path(tmp))
        try:
            m.boot()
            m.put("/run/drill/break.sh", script(drill, "break.sh"))
            # bash -e: a break that fails half way stops there, and the drill is invalid.
            r = m.script("/run/drill/break.sh", exports, "bash -e ")
            if r.returncode != 0:
                return {**rec, "result": "invalid", "why": "break.sh failed: " + clip(r.stdout + r.stderr)}
            state = m.take_state()
            if meta.get("reboot_after_break"):
                m.reboot()
            m.put("/run/drill/check.sh", script(drill, "check.sh"))
            m.give_state(state)
            if m.script("/run/drill/check.sh", exports).returncode == 0:
                return {**rec, "result": "invalid", "why": "check passes before any fix"}

            if args.reference:
                # fix.sh may read the state (the address a hosts entry had); a model's repair may not.
                m.put("/run/drill/fix.sh", script(drill, "fix.sh"))
                r = m.script("/run/drill/fix.sh", exports)
                answer, transcript, tokens, gave_up = (r.stdout + r.stderr)[-2000:], None, None, False
            else:
                m.ssh("rm -rf /var/lib/.drill-*")
                answer, transcript, tokens, gave_up = agent(m, task, args.model, args.think, args.memory)
            rec["fix_seconds"] = round(time.time() - t0)
            rec.update(answer=answer, gave_up=gave_up, tokens=tokens, transcript=transcript,
                       steps=sum(1 for x in transcript or [] if x["role"] == "tool"))

            if meta.get("reboot_before_check"):
                try:
                    m.reboot()
                except (TimeoutError, RuntimeError) as e:
                    return {**rec, "result": "fail", "why": f"no SSH after reboot: {e}"}
            # check.sh sees the final answer, for drills that ask a question, and the runner's copy
            # of the state, whatever became of those files in the machine.
            m.put("/run/drill/answer", answer, mode="600")
            m.put("/run/drill/check.sh", script(drill, "check.sh"))
            m.give_state(state)
            c = m.script("/run/drill/check.sh", exports)
            rec.update(result="pass" if c.returncode == 0 else "fail", check=clip(c.stdout + c.stderr))
            return rec
        except Exception as e:  # a drill that cannot run is reported, not fatal to the run
            return {**rec, "result": "error", "why": f"{type(e).__name__}: {e}"}
        finally:
            m.stop()


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--image", type=Path, required=True)
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--reference", action="store_true", help="run each drill's fix.sh")
    g.add_argument("--model", help="base URL of an OpenAI-compatible server")
    p.add_argument("--label", help="name for the results (default: reference or the model URL's host)")
    p.add_argument("--think", action="store_true", help="let the model think before answering")
    p.add_argument("--variants", type=int, default=1, help="variants of each drill (default 1)")
    p.add_argument("--seed", type=int, default=0, help="the first variant's seed (default 0)")
    p.add_argument("--memory", metavar="DIR", help="notes from an action memory (memory.py) at each step")
    p.add_argument("--memory-url", help="the embedding server for --memory")
    p.add_argument("drills", nargs="*", help="drill names (default: all)")
    args = p.parse_args()
    args.image = args.image.resolve()
    args.image_info = image_info(args.image)
    args.label = args.label or ("reference" if args.reference else "model")
    if args.memory:
        if not args.memory_url:
            p.error("--memory needs --memory-url")
        from memory import Memory
        args.memory = Memory(args.memory, args.memory_url)

    names = args.drills or sorted(d.name for d in (HERE / "drills").iterdir() if (d / "drill.toml").exists())
    results = args.image / "results"
    results.mkdir(exist_ok=True)
    out = results / f"{args.label}-{time.strftime('%Y%m%d-%H%M%S')}.jsonl"
    tally = {}
    with out.open("w") as f:
        for name, seed in [(n, s) for n in names for s in range(args.seed, args.seed + args.variants)]:
            t0 = time.time()
            rec = run_drill(HERE / "drills" / name, args, seed)
            rec["seconds"] = round(time.time() - t0)
            f.write(json.dumps(rec) + "\n")
            f.flush()
            tally[rec["result"]] = tally.get(rec["result"], 0) + 1
            print(f"{rec['result']:8} {name:28} {seed:3} {rec.get('seconds', 0):5} s  {rec.get('why', '')[:100]}", flush=True)
    print(" ".join(f"{k}: {v}" for k, v in sorted(tally.items())), f"-> {out}")
    sys.exit(0)


if __name__ == "__main__":
    main()
