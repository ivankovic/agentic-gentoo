"""The seeds set aside for evaluation, and the --exclude-seeds option that keeps them out.

A model is scored on the variants of these seeds, so no run of them may reach what it learns
from: neither the action memory (memory.py build) nor the training data (export.py).
"""
import argparse

EVAL_SEEDS = "0-2,200-202"


def parse_seeds(text):
    """The set of seeds in "0-2,200-202": numbers and inclusive ranges, separated by commas."""
    seeds = set()
    for part in text.split(","):
        part = part.strip()
        if not part:
            continue
        first, dash, last = part.partition("-")
        try:
            a = int(first)
            b = int(last) if dash else a
        except ValueError:
            raise argparse.ArgumentTypeError(f"not a seed or a range of seeds: {part!r}") from None
        if b < a:
            raise argparse.ArgumentTypeError(f"a range that ends before it starts: {part!r}")
        seeds.update(range(a, b + 1))
    return frozenset(seeds)


def add_exclude_seeds(parser):
    parser.add_argument("--exclude-seeds", type=parse_seeds, default=parse_seeds(EVAL_SEEDS), metavar="SEEDS",
                        help=f"leave out runs of these seeds (default: the evaluation seeds, {EVAL_SEEDS}; "
                             "'' for none)")
