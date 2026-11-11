#!/usr/bin/env python3
"""Training data from drill results: the repairs that passed their check, as chat transcripts.

    export.py RESULTS.jsonl... > train.jsonl [--max-steps N] [--exclude-seeds 0-2,200-202]

One line per passing repair: {"messages": [...], "tools": [...], "drill", "seed", "label"}, the
messages in the OpenAI chat format with tool calls (system, the task, the model's calls and the
outputs it saw, its final answer), which chat templates with tool support take as they are. A
drill variant (drill and seed) appears once: the shortest passing repair of it. Runs of the
--exclude-seeds are left out: by default the evaluation seeds (seeds.py), so the variants a model
is scored on are never among those it was trained on. Drills are held out at training (train.py
--hold-out), which needs their repairs here for the validation loss.
"""
import argparse
import json
import sys

from run import TOOLS
from seeds import add_exclude_seeds


def select(runs, max_steps, exclude_seeds):
    """The runs to train on, by (drill, seed): the shortest passing repair of each variant, none of
    an excluded seed."""
    best = {}
    for r in runs:
        if r["result"] != "pass" or not r.get("transcript") or r.get("gave_up"):
            continue
        if r["steps"] > max_steps or r.get("seed", 0) in exclude_seeds:
            continue
        key = (r["drill"], r.get("seed", 0))
        if key not in best or r["steps"] < best[key]["steps"]:
            best[key] = r
    return best


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("results", nargs="+")
    p.add_argument("--max-steps", type=int, default=20, help="leave out repairs longer than this")
    add_exclude_seeds(p)
    args = p.parse_args()

    best = select((json.loads(line) for path in args.results for line in open(path)),
                  args.max_steps, args.exclude_seeds)
    for (drill, seed), r in sorted(best.items()):
        # Thinking is left out: the student learns the actions, not the teacher's private notes;
        # so are memory notes, which were never part of the conversation.
        messages = [{k: v for k, v in m.items() if k not in ("reasoning_content", "memory")} for m in r["transcript"]]
        print(json.dumps({"messages": messages, "tools": TOOLS, "drill": drill, "seed": seed,
                          "label": r["label"]}))
    print(f"export: {len(best)} repairs", file=sys.stderr)


if __name__ == "__main__":
    main()
