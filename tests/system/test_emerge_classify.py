"""The emerge wrapper's reading of its arguments (usr-local/sbin/emerge --classify): which
invocations change the system, pass the scan gate, get a snapshot, and keep the directory."""
import os
import subprocess
import tempfile
import unittest

from _load import DECK

EMERGE = DECK / "usr-local/sbin/emerge"


def classify(*args, cwd="/"):
    out = subprocess.run(["bash", str(EMERGE), "--classify", *args], capture_output=True, text=True,
                         cwd=cwd, check=True).stdout
    return dict(word.split("=") for word in out.split())


class Classify(unittest.TestCase):
    def test_install_is_gated_and_snapshotted(self):
        for args in (["app-misc/foo"], ["-av", "foo"], ["-avuDN", "@world"], ["--update", "--deep", "@world"],
                     ["--oneshot", "foo"], ["-1", "foo"], ["--jobs", "4", "foo"]):
            c = classify(*args)
            self.assertEqual((c["changes"], c["installs"], c["gate"], c["snapshot"]), ("1", "1", "1", "1"), args)

    def test_looking_changes_nothing(self):
        for args in (["-p", "foo"], ["-pv", "foo"], ["--pretend", "foo"], ["-s", "foo"], ["--search", "foo"],
                     ["-S", "foo"], ["--info"], ["--sync"], ["-V"], ["--version"], ["-h"], ["--help"],
                     ["--check-news"], ["--list-sets"], ["-avp", "@world"]):
            c = classify(*args)
            self.assertEqual((c["changes"], c["gate"], c["snapshot"]), ("0", "0", "0"), args)

    def test_removing_is_snapshotted_not_gated(self):
        for args in (["--depclean"], ["-c"], ["-C", "foo"], ["--unmerge", "foo"], ["--deselect", "foo"],
                     ["--prune", "foo"], ["--config", "foo"], ["-ac"]):
            c = classify(*args)
            self.assertEqual((c["changes"], c["installs"], c["gate"], c["snapshot"]), ("1", "0", "0", "1"), args)

    def test_fetching_takes_no_snapshot(self):
        for args in (["--fetchonly", "foo"], ["-f", "foo"], ["-F", "foo"], ["--fetch-all-uri", "foo"], ["-fv", "foo"]):
            c = classify(*args)
            self.assertEqual((c["fetch"], c["gate"], c["snapshot"]), ("1", "0", "0"), args)

    def test_allow_unscanned_skips_only_the_gate(self):
        c = classify("--allow-unscanned", "foo")
        self.assertEqual((c["changes"], c["installs"], c["gate"], c["snapshot"]), ("1", "1", "0", "1"))

    def test_directory_is_kept_only_for_a_relative_file(self):
        with tempfile.TemporaryDirectory() as d:
            open(os.path.join(d, "foo-1.ebuild"), "w").close()
            self.assertEqual(classify("foo-1.ebuild", cwd=d)["cwd"], "keep")
            self.assertEqual(classify("./foo-1.ebuild", cwd=d)["cwd"], "keep")
            self.assertEqual(classify(os.path.join(d, "foo-1.ebuild"), cwd=d)["cwd"], "neutral")
            self.assertEqual(classify("app-misc/foo", "-av", cwd=d)["cwd"], "neutral")


if __name__ == "__main__":
    unittest.main()
