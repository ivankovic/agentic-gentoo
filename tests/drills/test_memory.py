import array
import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

import support  # noqa: F401
import memory


def store(dir_, ids, vectors=None, stamp=True):
    recs = [{"id": i, "situation": i} for i in ids]
    (Path(dir_) / "records.jsonl").write_text("".join(json.dumps(r) + "\n" for r in recs))
    if vectors is not None:
        flat = array.array("f", [0.5] * (vectors * memory.DIMS))
        with open(Path(dir_) / "vectors.f32", "wb") as f:
            flat.tofile(f)
        if stamp:
            (Path(dir_) / "vectors.json").write_text(json.dumps(memory.stamp(recs)))
    return recs


class Consistency(unittest.TestCase):
    def test_matching_store_loads(self):
        with tempfile.TemporaryDirectory() as d:
            store(d, ["a", "b", "c"], vectors=3)
            recs, vecs = memory.load(d)
            self.assertEqual((len(recs), len(vecs), len(vecs[2])), (3, 3, memory.DIMS))

    def test_records_rebuilt_with_the_same_count_are_refused(self):
        with tempfile.TemporaryDirectory() as d:
            store(d, ["a", "b", "c"], vectors=3)
            store(d, ["a", "c", "b"])            # rebuilt: same number, another order
            with self.assertRaises(SystemExit) as e:
                memory.load(d)
            self.assertIn("memory.py embed", str(e.exception))

    def test_another_count_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            store(d, ["a", "b", "c"], vectors=3)
            store(d, ["a", "b", "c", "d"])
            with self.assertRaises(SystemExit) as e:
                memory.load(d)
            self.assertIn("3 vectors", str(e.exception))

    def test_a_damaged_stamp_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            store(d, ["a"], vectors=1)
            (Path(d) / "vectors.json").write_text("{")
            with self.assertRaises(SystemExit):
                memory.load(d)

    def test_a_store_from_before_the_stamp_loads_with_a_warning(self):
        with tempfile.TemporaryDirectory() as d:
            store(d, ["a", "b"], vectors=2, stamp=False)
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                recs, vecs = memory.load(d)
            self.assertEqual(len(vecs), 2)
            self.assertIn("vectors.json is missing", err.getvalue())

    def test_embed_writes_the_stamp(self):
        import types
        from unittest import mock
        with tempfile.TemporaryDirectory() as d:
            recs = store(d, ["a", "b"])
            with mock.patch.object(memory, "embed_texts", return_value=[[1.0] * memory.DIMS] * 2):
                memory.embed(types.SimpleNamespace(dir=d, url="http://embeddings"))
            self.assertEqual(json.loads((Path(d) / "vectors.json").read_text()), memory.stamp(recs))
            self.assertEqual(len(memory.load(d)[1]), 2)


if __name__ == "__main__":
    unittest.main()
