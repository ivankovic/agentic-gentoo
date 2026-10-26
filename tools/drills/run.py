#!/usr/bin/env python3
"""Breakage drills: break a fresh drill machine, let a model (or the reference fix) repair it, check.

    run.py --image DIR --reference [DRILL ...]       validate drills: each must fail its check once
                                                     broken and pass after its fix.sh
    run.py --image DIR --model URL [--label NAME] [--think] [DRILL ...]
                                                     a model behind an OpenAI-compatible chat API
                                                     (llama-server --jinja) repairs each drill

DIR is build.sh's output. Each drill is a directory under drills/: drill.toml (category, task,
optional reboot_before_check), break.sh, check.sh, fix.sh. The machine boots from a copy-on-write
overlay of base.raw, so every drill starts from the same state; it has no network.

Results go to DIR/results/<label>-<time>.jsonl, one line per drill with the transcript.
"""
import argparse
import json
import os
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

HERE = Path(__file__).resolve().parent
OUTPUT_LIMIT = 3000   # characters of a command's output the model sees: the head and the tail
COMMAND_TIMEOUT = 300
MAX_STEPS = 30

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
        args = ["ssh", "-q", "-i", str(self.image / "key"), "-p", str(self.port),
                "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
                "-o", "ConnectTimeout=5", "-o", "BatchMode=yes", "root@127.0.0.1",
                "TERM=dumb PAGER=cat SYSTEMD_PAGER=cat SYSTEMD_COLORS=0 bash -lc " + shlex.quote(command)]
        try:
            return subprocess.run(args, input=stdin, capture_output=True, text=True, errors="replace",
                                  timeout=timeout)
        except subprocess.TimeoutExpired as e:
            out = (e.stdout or b"").decode(errors="replace") if isinstance(e.stdout, bytes) else (e.stdout or "")
            return subprocess.CompletedProcess(args, 124, out, f"timed out after {timeout} s")

    def put(self, path: str, content: str, mode="700"):
        self.ssh(f"install -D -m {mode} /dev/stdin {shlex.quote(path)}", stdin=content)

    def reboot(self):
        self.ssh("systemctl reboot", timeout=10)
        time.sleep(5)
        self.wait_ssh()

    def stop(self):
        if self.qemu and self.qemu.poll() is None:
            self.qemu.kill()
            self.qemu.wait()


def clip(text):
    if len(text) <= OUTPUT_LIMIT:
        return text
    half = OUTPUT_LIMIT // 2
    return f"{text[:half]}\n[... {len(text) - OUTPUT_LIMIT} characters left out ...]\n{text[-half:]}"


class ContextFull(Exception):
    """The conversation no longer fits the model's context: the model's limit, so a fail."""


def chat(url, messages, think):
    body = {"messages": messages, "tools": TOOLS, "temperature": 0.2, "max_tokens": 4096,
            "chat_template_kwargs": {"enable_thinking": think}}
    req = urllib.request.Request(url.rstrip("/") + "/v1/chat/completions", json.dumps(body).encode(),
                                 {"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=1800) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        body = e.read().decode(errors="replace")
        if "exceeds the available context" in body or "exceed_context_size" in body:
            raise ContextFull() from None
        raise RuntimeError(f"HTTP {e.code}: {body[:300]}") from None


def agent(machine, task, url, think):
    """The model's loop: tool calls run on the machine until it answers without one."""
    messages = [{"role": "system", "content": SYSTEM_PROMPT}, {"role": "user", "content": task}]
    transcript = list(messages)
    tokens = {"prompt": 0, "completion": 0}
    for _ in range(MAX_STEPS):
        try:
            reply = chat(url, messages, think)
        except ContextFull:
            return "", transcript, tokens, "context"
        usage = reply.get("usage", {})
        tokens["prompt"] += usage.get("prompt_tokens", 0)
        tokens["completion"] += usage.get("completion_tokens", 0)
        msg = reply["choices"][0]["message"]
        # Earlier turns' thinking is not sent back: the transcript keeps it, the context doesn't.
        transcript.append({k: v for k, v in msg.items() if k in ("role", "content", "tool_calls", "reasoning_content")})
        messages.append({k: v for k, v in msg.items() if k in ("role", "content", "tool_calls")})
        calls = msg.get("tool_calls") or []
        if not calls:
            return msg.get("content") or "", transcript, tokens, False
        for call in calls:
            try:
                command = json.loads(call["function"]["arguments"])["command"]
                r = machine.ssh(command)
                result = clip(r.stdout + r.stderr) + f"\n[exit code {r.returncode}]"
            except (KeyError, json.JSONDecodeError) as e:
                result = f"bad tool call: {e}"
            messages.append({"role": "tool", "tool_call_id": call.get("id", ""), "content": result})
            transcript.append(messages[-1])
    return "", transcript, tokens, "steps"


def run_drill(drill: Path, args):
    meta = tomllib.loads((drill / "drill.toml").read_text())
    rec = {"drill": drill.name, "category": meta["category"], "label": args.label}
    t0 = time.time()
    with tempfile.TemporaryDirectory(prefix=f"drill-{drill.name}-", dir=args.image) as tmp:
        m = Machine(args.image, Path(tmp))
        try:
            m.boot()
            m.put("/run/drill/break.sh", (drill / "break.sh").read_text())
            r = m.ssh("/run/drill/break.sh && rm -f /run/drill/break.sh")
            if r.returncode != 0:
                return {**rec, "result": "invalid", "why": "break.sh failed: " + clip(r.stdout + r.stderr)}
            if meta.get("reboot_after_break"):
                m.reboot()
            m.put("/run/drill/check.sh", (drill / "check.sh").read_text())
            if m.ssh("/run/drill/check.sh").returncode == 0:
                return {**rec, "result": "invalid", "why": "check passes before any fix"}
            m.ssh("rm -f /run/drill/check.sh")

            if args.reference:
                m.put("/run/drill/fix.sh", (drill / "fix.sh").read_text())
                r = m.ssh("/run/drill/fix.sh; s=$?; rm -f /run/drill/fix.sh; exit $s")
                answer, transcript, tokens, gave_up = (r.stdout + r.stderr)[-2000:], None, None, False
            else:
                answer, transcript, tokens, gave_up = agent(m, meta["task"].strip(), args.model, args.think)
            rec["fix_seconds"] = round(time.time() - t0)

            if meta.get("reboot_before_check"):
                try:
                    m.reboot()
                except (TimeoutError, RuntimeError) as e:
                    return {**rec, "result": "fail", "why": f"no SSH after reboot: {e}", "answer": answer,
                            "transcript": transcript, "tokens": tokens}
            # check.sh sees the final answer, for drills that ask a question.
            m.put("/run/drill/answer", answer, mode="600")
            m.put("/run/drill/check.sh", (drill / "check.sh").read_text())
            c = m.ssh("/run/drill/check.sh")
            rec.update(result="pass" if c.returncode == 0 else "fail", check=clip(c.stdout + c.stderr),
                       answer=answer, gave_up=gave_up, tokens=tokens, transcript=transcript,
                       steps=sum(1 for x in transcript or [] if x["role"] == "tool"))
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
    p.add_argument("drills", nargs="*", help="drill names (default: all)")
    args = p.parse_args()
    args.image = args.image.resolve()
    args.label = args.label or ("reference" if args.reference else "model")

    names = args.drills or sorted(d.name for d in (HERE / "drills").iterdir() if (d / "drill.toml").exists())
    results = args.image / "results"
    results.mkdir(exist_ok=True)
    out = results / f"{args.label}-{time.strftime('%Y%m%d-%H%M%S')}.jsonl"
    tally = {}
    with out.open("w") as f:
        for name in names:
            t0 = time.time()
            rec = run_drill(HERE / "drills" / name, args)
            rec["seconds"] = round(time.time() - t0)
            f.write(json.dumps(rec) + "\n")
            f.flush()
            tally[rec["result"]] = tally.get(rec["result"], 0) + 1
            print(f"{rec['result']:8} {name:28} {rec.get('seconds', 0):5} s  {rec.get('why', '')[:100]}", flush=True)
    print(" ".join(f"{k}: {v}" for k, v in sorted(tally.items())), f"-> {out}")
    sys.exit(0)


if __name__ == "__main__":
    main()
