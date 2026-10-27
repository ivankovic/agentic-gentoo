#!/usr/bin/env python3
"""Training data from drill results: the repairs that passed their check, as chat transcripts.

    export.py RESULTS.jsonl... > train.jsonl [--max-steps N]

One line per passing repair: {"messages": [...], "tools": [...], "drill", "seed", "label"}, the
messages in the OpenAI chat format with tool calls (system, the task, the model's calls and the
outputs it saw, its final answer), which chat templates with tool support take as they are. A
drill variant (drill and seed) appears once: the shortest passing repair of it.
"""
import argparse
import json
import sys

from run import TOOLS


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("results", nargs="+")
    p.add_argument("--max-steps", type=int, default=20, help="leave out repairs longer than this")
    args = p.parse_args()

    best = {}
    for path in args.results:
        for line in open(path):
            r = json.loads(line)
            if r["result"] != "pass" or not r.get("transcript") or r.get("gave_up"):
                continue
            if r["steps"] > args.max_steps:
                continue
            key = (r["drill"], r.get("seed", 0))
            if key not in best or r["steps"] < best[key]["steps"]:
                best[key] = r
    for (drill, seed), r in sorted(best.items()):
        # Thinking is left out: the student learns the actions, not the teacher's private notes;
        # so are memory notes, which were never part of the conversation.
        messages = [{k: v for k, v in m.items() if k not in ("reasoning_content", "memory")} for m in r["transcript"]]
        print(json.dumps({"messages": messages, "tools": TOOLS, "drill": drill, "seed": seed,
                          "label": r["label"]}))
    print(f"export: {len(best)} repairs", file=sys.stderr)


if __name__ == "__main__":
    main()
