import argparse
import unittest

import support  # noqa: F401
import export
import memory
import seeds


def run(seed, drill="service-typo", steps=3, result="pass"):
    return {"drill": drill, "seed": seed, "steps": steps, "result": result, "transcript": [{}], "label": "t"}


class ParseSeeds(unittest.TestCase):
    def test_ranges_and_numbers(self):
        self.assertEqual(seeds.parse_seeds("0-2,200-202"), {0, 1, 2, 200, 201, 202})
        self.assertEqual(seeds.parse_seeds("7, 9-9"), {7, 9})
        self.assertEqual(seeds.parse_seeds(""), frozenset())

    def test_bad(self):
        for text in ("a", "3-1", "1-b", "-"):
            with self.assertRaises(argparse.ArgumentTypeError):
                seeds.parse_seeds(text)

    def test_default_is_the_evaluation_seeds(self):
        p = argparse.ArgumentParser()
        seeds.add_exclude_seeds(p)
        self.assertEqual(p.parse_args([]).exclude_seeds, seeds.parse_seeds(seeds.EVAL_SEEDS))
        self.assertEqual(p.parse_args(["--exclude-seeds", ""]).exclude_seeds, frozenset())


class MemoryKeeps(unittest.TestCase):
    def test_evaluation_seeds_above_min_seed_stay_out(self):
        ex = seeds.parse_seeds(seeds.EVAL_SEEDS)
        self.assertTrue(memory.kept(run(100), 100, ex, []))
        self.assertTrue(memory.kept(run(203), 100, ex, []))
        for seed in (0, 2, 99, 200, 201, 202):
            self.assertFalse(memory.kept(run(seed), 100, ex, []), seed)
        self.assertFalse(memory.kept(run(100), 100, ex, ["service-typo"]))

    def test_min_seed_alone_lets_them_in(self):
        self.assertTrue(memory.kept(run(200), 100, frozenset(), []))


class ExportSelects(unittest.TestCase):
    def test_excluded_seeds_are_left_out(self):
        runs = [run(1), run(100), run(100, steps=2), run(201), run(101, result="fail"), run(102, steps=50)]
        best = export.select(runs, 20, seeds.parse_seeds(seeds.EVAL_SEEDS))
        self.assertEqual(list(best), [("service-typo", 100)])
        self.assertEqual(best["service-typo", 100]["steps"], 2)

    def test_without_exclusion(self):
        self.assertEqual(len(export.select([run(1), run(201)], 20, frozenset())), 2)


if __name__ == "__main__":
    unittest.main()
