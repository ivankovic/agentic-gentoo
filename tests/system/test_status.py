"""deck-attention's checks, each on its own (a broken state file costs one line, not all), its
drift check through `deploy.sh <host> list`; hoststatus's Gentoo part and what deck-status makes
of it; deck-nosnap's options."""
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import time
import unittest
from unittest import mock

from _load import DECK, ROOT, script

attention = script(DECK / "usr-local/bin/deck-attention")
status = script(DECK / "usr-local/bin/deck-status")
hoststatus = script(DECK / "usr-local/share/deck-status/hoststatus")


class Attention(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp(prefix="attention-test."))
        self.addCleanup(shutil.rmtree, self.tmp)
        (self.tmp / "portage.json").write_text(json.dumps({"world_updated": "2020-01-01T00:00:00+00:00"}))
        quiet = dict(repos=lambda: [], drift=lambda: None, crashes=lambda: {}, snapshotted=lambda: [],
                     disks=lambda: [], seen=lambda: 0, HOME=self.tmp, PORTAGE=self.tmp / "portage.json",
                     UNCLEAN=self.tmp / "unclean", NEWS=self.tmp / "news", REVIEWS=self.tmp / "reviews",
                     SCANS=self.tmp / "scan")
        for name, value in quiet.items():
            p = mock.patch.object(attention, name, value)
            p.start()
            self.addCleanup(p.stop)

    def lines(self):
        return [line for _, line, _ in attention.gather()[0]]

    def test_healthy_state(self):
        self.assertEqual(len(self.lines()), 1)
        self.assertRegex(self.lines()[0], r"@world last updated \d+ days ago")

    def test_broken_files_fail_only_their_check(self):
        (self.tmp / "portage.json").write_text(json.dumps({"world_updated": "last tuesday"}))
        (self.tmp / "reviews").mkdir()
        (self.tmp / "reviews/a.json").write_text(json.dumps({"status": "done", "findings": [{}]}))  # no subject
        (self.tmp / "scan/x").mkdir(parents=True)
        (self.tmp / "scan/x/a.json").write_text(json.dumps({"status": "flagged", "when": time.time()}))  # no cpv
        with mock.patch.object(attention, "disks", lambda: [("/ is 99% full", "  ncdu")]), \
                mock.patch.object(attention, "repos", lambda: 1 / 0):
            items = attention.gather()[0]
        lines = [line for _, line, _ in items]
        self.assertEqual(lines, ["check failed: repos", "check failed: portage", "check failed: scans",
                                 "check failed: reviews", "/ is 99% full"])
        self.assertTrue(all(level == "soon" for level, line, _ in items if line.startswith("check failed")))

    def test_good_files_beside_a_broken_check(self):
        (self.tmp / "portage.json").write_text("{}")
        (self.tmp / "unclean").write_text("not a time\n")
        (self.tmp / "reviews").mkdir()
        (self.tmp / "reviews/a.json").write_text(json.dumps({"status": "done", "subject": "ours/x", "findings": [{}]}))
        (self.tmp / "reviews/b.json").write_text("{ not json")
        self.assertEqual(self.lines(), ["No @world update on record", "Review findings to decide: ours/x"])

    def test_json_output_survives(self):
        (self.tmp / "portage.json").write_text(json.dumps({"world_updated": 5}))
        with mock.patch("sys.argv", ["deck-attention", "--json"]), mock.patch("builtins.print") as out:
            self.assertEqual(attention.main(), 0)
        self.assertEqual([i["line"] for i in json.loads(out.call_args[0][0])], ["check failed: portage"])


class Drift(unittest.TestCase):
    """The drift check against a made-up repository and root, through the real deploy.sh."""

    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp(prefix="drift-test."))
        self.addCleanup(shutil.rmtree, self.tmp)
        self.repo, self.root, self.home = self.tmp / "repo", self.tmp / "root", self.tmp / "home"
        files = {"hosts/base/etc/shared.conf": "shared\n", "hosts/t/etc/a.conf": "a\n", "hosts/t/usr-local/bin/tool": "#!/bin/sh\n",
                 "hosts/t/recovery/README.md": "r\n", "hosts/t/home/.config/h.conf": "h\n",
                 "hosts/t/inherit": "hosts/base/etc/shared.conf\n"}
        for rel, text in files.items():
            (self.repo / rel).parent.mkdir(parents=True, exist_ok=True)
            (self.repo / rel).write_text(text)
        (self.repo / "hosts/t/usr-local/bin/tool").chmod(0o755)
        shutil.copy(ROOT / "deploy.sh", self.repo / "deploy.sh")
        git = ["git", "-c", "user.name=test", "-c", "user.email=test@example.invalid"]
        for args in (["init", "-q", "."], ["add", "-A"], ["commit", "-q", "-m", "test"]):
            subprocess.run(git + args, cwd=self.repo, check=True, capture_output=True)
        self.home.mkdir()
        env = {k: v for k, v in os.environ.items() if k != "DECK_HOST_DIR"}
        for p in (mock.patch.object(attention, "REPO", self.repo), mock.patch.object(attention, "HOST", "t"),
                  mock.patch.object(attention, "HOME", self.home),
                  mock.patch.dict(os.environ, dict(env, HOME=str(self.home), DEPLOY_ROOT=str(self.root)), clear=True)):
            p.start()
            self.addCleanup(p.stop)

    def put(self, rel, text, mode=0o644):
        f = self.root / rel
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text)
        f.chmod(mode)

    def test_content_mode_inherit_and_recovery(self):
        self.put("etc/a.conf", "a\n")
        self.put("etc/shared.conf", "edited\n")
        self.put("usr/local/bin/tool", "#!/bin/sh\n")  # not executable
        (self.home / ".config").mkdir()
        (self.home / ".config/h.conf").write_text("h\n")
        found, how = attention.drift()
        self.assertEqual(sorted(found), [("changed:", f"{self.root}/etc/shared.conf"),
                                         ("mode:", f"{self.root}/usr/local/bin/tool"),
                                         ("new:", f"{self.root}/etc/deck-recovery/RECOVERY.md")])
        self.assertEqual(how, "./deploy.sh t")
        self.assertIn("Config differs from repo: 3 files", attention.drift_items()[0][1])

    def test_host_dir_file(self):
        ext = self.tmp / "home/elsewhere"
        (ext / "etc").mkdir(parents=True)
        (ext / "etc/ext.conf").write_text("x\n")
        git = ["git", "-c", "user.name=test", "-c", "user.email=test@example.invalid"]
        for args in (["init", "-q", "."], ["add", "-A"], ["commit", "-q", "-m", "test"]):
            subprocess.run(git + args, cwd=ext, check=True, capture_output=True)
        (self.home / ".config/deck").mkdir(parents=True)
        (self.home / ".config/deck/host-dir").write_text("elsewhere\n")
        found, how = attention.drift()
        self.assertEqual(found, [("new:", f"{self.root}/etc/ext.conf")])
        self.assertEqual(how, f"DECK_HOST_DIR={ext} ./deploy.sh t")

    def test_no_host_directory_is_no_item(self):
        with mock.patch.object(attention, "HOST", "nowhere"):
            self.assertIsNone(attention.drift())
            self.assertEqual(attention.drift_items(), [])

    def test_failing_list_is_one_failed_check(self):
        (self.repo / "hosts/t/inherit").write_text("hosts/base/nonsense\n")
        self.assertEqual([line for _, line, _ in attention.guarded("drift", attention.drift_items)()],
                         ["check failed: drift"])


class Hosts(unittest.TestCase):
    def test_old_report_is_unchanged(self):
        self.assertEqual(status.gentoo(None), ([], []))
        report = {"uptime": 86400, "load": 0.1, "cpus": 2, "failed": [], "disks": {"/": 10}, "containers": []}
        state, line, items, detail = status.judge({"name": "server"}, report, {})
        self.assertEqual((state, line, items), ("ok", "server", []))
        self.assertEqual(len(detail), 3)

    def test_world_age_and_advisories_are_problems(self):
        g = {"world_age_days": 20, "advisories": 2, "scrubs": [{"mount": "/", "result": "ok", "age_days": 3}],
             "hints": ["no sync timer"]}
        problems, detail = status.gentoo(g)
        self.assertEqual(problems, ["world update 20 days old", "2 advisories"])
        self.assertIn("scrub /: ok, 3 days ago", detail)
        self.assertIn("hint: no sync timer", detail)
        report = {"uptime": 0, "disks": {}, "containers": [], "gentoo": g}
        state, line, items, _ = status.judge({"name": "server"}, report, {})
        self.assertEqual((state, line, items[0]["level"]), ("partial", "server: world update 20 days old, 2 advisories", "soon"))

    def test_fresh_host_is_ok(self):
        self.assertEqual(status.gentoo({"world_age_days": 14, "advisories": 0, "scrubs": [], "hints": []})[0], [])
        self.assertEqual(status.gentoo({"world_age_days": None})[0], ["no world update on record"])
        self.assertEqual(status.gentoo({"advisories": 1})[0], ["1 advisory"])

    def test_unit_names_for_mount_points(self):
        for mount, name in (("/", "-"), ("/srv/data", "srv-data"), ("/srv/my-data", "srv-my\\x2ddata"),
                            ("/.x/y", "\\x2ex-y"), ("/a b", "a\\x20b")):
            self.assertEqual(hoststatus.escape(mount), name)

    def test_btrfs_filesystems(self):
        mounts = ("/dev/mapper/root / btrfs rw,subvol=/@ 0 0\n/dev/mapper/root /home btrfs rw 0 0\n"
                  "/dev/sdb1 /srv/data btrfs rw 0 0\n/dev/sda1 /efi vfat rw 0 0\ntmpfs /tmp tmpfs rw 0 0\n")
        self.assertEqual(hoststatus.btrfs_mounts(mounts), {"/dev/mapper/root": ["/", "/home"], "/dev/sdb1": ["/srv/data"]})

    def test_report_is_json_with_the_old_keys(self):
        r = subprocess.run(["python3", str(DECK / "usr-local/share/deck-status/hoststatus")], capture_output=True, text=True)
        report = json.loads(r.stdout)
        for key in ("host", "uptime", "load", "cpus", "reboot", "reboot_pkgs", "failed", "disks", "containers", "listening"):
            self.assertIn(key, report)


class Nosnap(unittest.TestCase):
    NOSNAP = DECK / "usr-local/bin/deck-nosnap"

    def run_it(self, *args):
        # A PATH without btrfs: no argument here may reach it.
        with tempfile.TemporaryDirectory() as d:
            for tool in ("sed", "bash", "stat"):
                os.symlink(shutil.which(tool), os.path.join(d, tool))
            return subprocess.run([shutil.which("bash"), str(self.NOSNAP), *args], capture_output=True, text=True,
                                  cwd=d, env={"PATH": d})

    def test_help(self):
        for flag in ("-h", "--help"):
            r = self.run_it(flag)
            self.assertEqual(r.returncode, 0)
            self.assertIn("deck-nosnap DIR...", r.stdout)

    def test_unknown_option(self):
        r = self.run_it("--bogus", "dir")
        self.assertEqual(r.returncode, 2)
        self.assertIn("unknown option --bogus", r.stderr)

    def test_no_folder(self):
        self.assertEqual(self.run_it().returncode, 2)
        self.assertEqual(self.run_it("--").returncode, 2)


if __name__ == "__main__":
    unittest.main()
