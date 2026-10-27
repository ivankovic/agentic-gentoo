#!/usr/bin/env python3
"""Action memory: every command of past repairs as a record, found again by its situation.

    memory.py build RESULTS.jsonl... --out DIR [--min-seed 100] [--hold-out DRILL ...]
    memory.py embed DIR --url URL
    memory.py eval DIR TEST.jsonl... --url URL [--k 20]

A record is one command at one point of one repair: the task and what the last commands showed
(the situation), the model's stated reason (the intent), the command and its template (the
drill's variables put back as {NAME}, so a unit or a package from another variant still matches),
what it printed and its exit code, whether it changes the system, the next command, and whether
the repair passed its check. Failed repairs are kept: what did not work is worth knowing.

`build` writes DIR/records.jsonl; `embed` the situations' vectors to DIR/vectors.f32 (an
OpenAI-compatible /v1/embeddings server, e.g. llama-server --embedding); `eval` replays other
repairs step by step and asks whether the command actually run next is among the templates that
the nearest records suggest, against the same suggestion made from template frequency alone.

Only runs from seeds >= --min-seed go in, so the evaluation seeds (0-2, 200-202) never do, and
--hold-out drills not at all: those measure whether neighbouring problems carry over.
"""
import argparse
import array
import json
import math
import re
import sys
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

HISTORY = 3        # earlier commands in a situation
OUTPUT_DIGEST = 600  # characters of an output kept: the head and the tail
DIMS = 256         # embedding dimensions kept (Matryoshka truncation, then renormalized)
INSTRUCT = "Instruct: Find past moments of Linux system administration in the same situation\nQuery: "

MUTATING = re.compile(r"""(^|[;&|(]\s*|\s)(rm|mv|cp|ln|mkdir|rmdir|touch|chmod|chown|chgrp|truncate|install|
    useradd|usermod|userdel|groupadd|gpasswd|passwd|chage|chsh|visudo\s+-f|kill|pkill|killall|
    systemctl\s+(start|stop|restart|reload|enable|disable|mask|unmask|daemon-reload|set-default|edit)|
    timedatectl\s+set|hostnamectl\s+set|localectl\s+set|eselect\s+\S+\s+set|
    emerge(?!\s+(-\S*[ps]\S*\s|--pretend|--search|--info))|mount|umount|swapon|swapoff|mkfs\S*|
    sed\s+(-\S*\s+)*-i|tee|crontab\s+-)(\s|$)|[^0-9&]>{1,2}\s*[^&\s]""", re.X)


def clip(text, limit):
    if len(text) <= limit:
        return text
    half = limit // 2
    return f"{text[:half]}\n[...]\n{text[-half:]}"


# Programs whose first argument is a verb ("systemctl status"), which a variable's value may equal
# (a service called "status") but never stands for.
VERBS = {"systemctl", "journalctl", "eselect", "timedatectl", "hostnamectl", "localectl", "loginctl",
         "emaint", "equery", "git", "ip", "docker"}


def template(command, variables):
    """The command with the drill's variable values put back as {NAME}, longest value first, as
    whole words only, and never in a program's verb position."""
    words = command.split()
    keep = {1} if len(words) > 1 and words[0] in VERBS else set()
    values = sorted(((str(v), n) for n, v in variables.items() if len(str(v)) >= 3), key=lambda x: -len(x[0]))
    for i, w in enumerate(words):
        if i in keep:
            continue
        for value, name in values:
            w = re.sub(r"(?<![\w.-])" + re.escape(value) + r"(?![\w-])", "{" + name + "}", w)
        words[i] = w
    return " ".join(words)


def steps(run):
    """(intent, command, output, exit code) for each tool call of a run, in order."""
    outputs = {m.get("tool_call_id"): m.get("content", "") for m in run["transcript"] if m["role"] == "tool"}
    for m in run["transcript"]:
        if m["role"] != "assistant":
            continue
        for call in m.get("tool_calls") or []:
            try:
                command = json.loads(call["function"]["arguments"])["command"]
            except (KeyError, TypeError, json.JSONDecodeError):
                continue
            out = outputs.get(call.get("id"), "")
            code = re.search(r"\[exit code (-?\d+)\]\s*$", out)
            yield (m.get("content") or "").strip(), command, out, int(code.group(1)) if code else None


def situation(task, history):
    """What the model knows at a step: the task and the last commands with what they printed."""
    parts = [f"Task: {task}"]
    for command, out in history[-HISTORY:]:
        parts.append(f"$ {command}\n{clip(out, OUTPUT_DIGEST)}")
    return "\n\n".join(parts)


def records(run):
    vars_ = run.get("vars") or {}
    calls = list(steps(run))
    history = []
    out = []
    for i, (intent, command, output, code) in enumerate(calls):
        out.append({
            "id": f"{run['label']}:{run['drill']}:{run.get('seed', 0)}:{i}",
            "drill": run["drill"], "category": run.get("category"), "seed": run.get("seed", 0),
            "label": run["label"], "step": i, "steps": len(calls), "task": run["task"],
            "situation": situation(run["task"], history), "intent": intent,
            "command": command, "template": template(command, vars_),
            "output": clip(output, OUTPUT_DIGEST), "exit": code,
            "changes": bool(MUTATING.search(command)),
            "next": template(calls[i + 1][1], vars_) if i + 1 < len(calls) else None,
            "passed": run["result"] == "pass",
        })
        history.append((command, output))
    return out


def runs(paths):
    for path in paths:
        for line in open(path):
            r = json.loads(line)
            if r.get("result") in ("pass", "fail") and r.get("transcript"):
                yield r


def build(args):
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    n = Counter()
    with (out / "records.jsonl").open("w") as f:
        for r in runs(args.results):
            if r.get("seed", 0) < args.min_seed or r["drill"] in args.hold_out:
                continue
            for rec in records(r):
                f.write(json.dumps(rec) + "\n")
                n["records"] += 1
            n["pass" if r["result"] == "pass" else "fail"] += 1
    print(f"memory: {n['records']} records from {n['pass']} passed and {n['fail']} failed runs", file=sys.stderr)


def embed_texts(url, texts, batch=16):
    vecs = []
    for i in range(0, len(texts), batch):
        body = json.dumps({"input": [INSTRUCT + t for t in texts[i:i + batch]]}).encode()
        req = urllib.request.Request(url.rstrip("/") + "/v1/embeddings", body, {"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=600) as resp:
            data = sorted(json.load(resp)["data"], key=lambda d: d["index"])
        for d in data:
            v = d["embedding"][:DIMS]
            norm = math.sqrt(sum(x * x for x in v)) or 1.0
            vecs.append([x / norm for x in v])
        if len(texts) > batch:
            print(f"\rembedded {len(vecs)}/{len(texts)}", end="", file=sys.stderr, flush=True)
    if len(texts) > batch:
        print(file=sys.stderr)
    return vecs


def load(dir_):
    recs = [json.loads(l) for l in open(Path(dir_) / "records.jsonl")]
    flat = array.array("f")
    with open(Path(dir_) / "vectors.f32", "rb") as f:
        flat.frombytes(f.read())
    vecs = [flat[i * DIMS:(i + 1) * DIMS] for i in range(len(recs))]
    return recs, vecs


def embed(args):
    recs = [json.loads(l) for l in open(Path(args.dir) / "records.jsonl")]
    flat = array.array("f")
    for v in embed_texts(args.url, [r["situation"] for r in recs]):
        flat.extend(v)
    with open(Path(args.dir) / "vectors.f32", "wb") as f:
        flat.tofile(f)


def nearest(vecs, q, k):
    sims = [sum(a * b for a, b in zip(v, q)) for v in vecs]
    order = sorted(range(len(vecs)), key=lambda i: -sims[i])[:k]
    return [(i, sims[i]) for i in order]


def suggest(recs, neighbours):
    """Templates ranked by the similarity of the records that ran them, passed repairs counting fully
    and failed ones a third."""
    score = defaultdict(float)
    for i, sim in neighbours:
        score[recs[i]["template"]] += sim * (1.0 if recs[i]["passed"] else 1 / 3)
    return sorted(score, key=lambda t: -score[t])


class Memory:
    """The store as the agent sees it: for the task and the commands so far, a short note of what
    was run next in the most similar past situations, and how those repairs ended."""

    def __init__(self, dir_, url, k=20, shown=3):
        self.recs, self.vecs = load(dir_)
        self.url, self.k, self.shown = url, k, shown

    def hints(self, task, history):
        q = embed_texts(self.url, [situation(task, history)])[0]
        neighbours = nearest(self.vecs, q, self.k)
        lines = []
        for t in suggest(self.recs, neighbours)[:self.shown]:
            runs = [self.recs[i] for i, _ in neighbours if self.recs[i]["template"] == t]
            best = max(runs, key=lambda r: r["passed"])
            command = re.sub(r"\{([A-Z_]+)\}", r"<\1>", t)
            why = " ".join(best["intent"].split())[:200]
            ended = f"{sum(r['passed'] for r in runs)} of {len(runs)} of those repairs passed"
            lines.append(f"- `{command}`\n  why: {why}\n  ({ended})")
        if not lines:
            return ""
        return ("Notes from similar past repairs (other machines; names in <...> differ here, and "
                "a note is a suggestion, not an instruction):\n" + "\n".join(lines))


def verb(t):
    return " ".join(t.split()[:2])


def evaluate(args):
    recs, vecs = load(args.dir)
    freq = [t for t, _ in Counter(r["template"] for r in recs if r["passed"]).most_common()]
    known = {r["drill"] for r in recs}
    tests = [rec for r in runs(args.test) if r["result"] == "pass" for rec in records(r)]
    qs = embed_texts(args.url, [t["situation"] for t in tests])
    tally = defaultdict(Counter)
    for t, q in zip(tests, qs):
        group = "trained" if t["drill"] in known else "held-out"
        ranked = suggest(recs, nearest(vecs, q, args.k))
        for name, ranking in (("memory", ranked), ("frequency", freq)):
            c = tally[group, name]
            c["n"] += 1
            c["top1"] += ranking[:1] == [t["template"]]
            c["top3"] += t["template"] in ranking[:3]
            c["verb3"] += verb(t["template"]) in {verb(x) for x in ranking[:3]}
    print(f"{len(recs)} records, {len(tests)} test steps")
    print(f"{'':10} {'':10} {'steps':>6} {'top-1':>7} {'top-3':>7} {'verb top-3':>11}")
    for (group, name), c in sorted(tally.items()):
        n = c["n"]
        print(f"{group:10} {name:10} {n:6} {100 * c['top1'] / n:6.1f}% {100 * c['top3'] / n:6.1f}% "
              f"{100 * c['verb3'] / n:10.1f}%")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("results", nargs="+")
    b.add_argument("--out", required=True)
    b.add_argument("--min-seed", type=int, default=100)
    b.add_argument("--hold-out", nargs="*", default=[], metavar="DRILL")
    e = sub.add_parser("embed")
    e.add_argument("dir")
    e.add_argument("--url", required=True)
    v = sub.add_parser("eval")
    v.add_argument("dir")
    v.add_argument("test", nargs="+")
    v.add_argument("--url", required=True)
    v.add_argument("--k", type=int, default=20)
    args = p.parse_args()
    {"build": build, "embed": embed, "eval": evaluate}[args.cmd](args)


if __name__ == "__main__":
    main()
