"""deploy.sh against a made-up repository and a made-up root (DEPLOY_ROOT): list, diff, apply,
untracked, and the errors that used to read as "up to date". Nothing outside the temporary
directory is read or written; nothing runs through sudo."""
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

from _load import ROOT


class Deploy(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp(prefix="deploy-test."))
        self.addCleanup(shutil.rmtree, self.tmp)
        self.repo, self.root, self.home = self.tmp / "repo", self.tmp / "root", self.tmp / "home"
        for d in (self.repo, self.root / "etc", self.home):
            d.mkdir(parents=True)
        shutil.copy(ROOT / "deploy.sh", self.repo / "deploy.sh")
        self.write("hosts/base/etc/shared.conf", "from base\n")
        self.write("hosts/base/etc/both.conf", "base's\n")
        self.write("hosts/base/usr-local/bin/tool", "#!/bin/sh\n", mode=0o755)
        self.write("hosts/t/etc/both.conf", "t's own\n")
        self.write("hosts/t/etc/systemd/system/a.service", "[Unit]\n")
        self.write("hosts/t/portage/make.conf", "USE=x\n")
        self.write("hosts/t/home/.config/app.conf", "home\n")
        self.write("hosts/t/recovery/README.md", "recovery\n")
        self.write("hosts/t/state/record.txt", "not deployed\n")
        self.write("hosts/t/inherit", "# shared\nhosts/base/etc\nhosts/base/usr-local/bin/tool\n")
        self.git("init", "-q", ".")
        self.commit()

    def write(self, rel, text, mode=0o644, base=None):
        f = (base or self.repo) / rel
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(text)
        f.chmod(mode)
        return f

    def git(self, *args, cwd=None):
        subprocess.run(["git", "-c", "user.name=test", "-c", "user.email=test@example.invalid", *args],
                       cwd=cwd or self.repo, check=True, capture_output=True)

    def commit(self, cwd=None):
        self.git("add", "-A", cwd=cwd)
        self.git("commit", "-q", "-m", "test", cwd=cwd)

    def deploy(self, *args, answer="", env=None):
        e = dict(os.environ, HOME=str(self.home), DEPLOY_ROOT=str(self.root))
        e.pop("DECK_HOST_DIR", None)
        e.pop("XDG_STATE_HOME", None)
        e.update(env or {})
        return subprocess.run(["bash", str(self.repo / "deploy.sh"), *args], input=answer, capture_output=True,
                              text=True, env=e, cwd=self.tmp)

    def test_list_maps_own_and_inherited(self):
        r = self.deploy("t", "list")
        self.assertEqual(r.returncode, 0, r.stderr)
        got = dict(l.split("\t") for l in r.stdout.splitlines())
        root = str(self.root)
        self.assertEqual(got, {
            "hosts/t/etc/both.conf": root + "/etc/both.conf",  # its own wins over the inherited one
            "hosts/t/etc/systemd/system/a.service": root + "/etc/systemd/system/a.service",
            "hosts/t/portage/make.conf": root + "/etc/portage/make.conf",
            "hosts/t/home/.config/app.conf": str(self.home / ".config/app.conf"),
            "hosts/t/recovery/README.md": root + "/etc/deck-recovery/RECOVERY.md",
            "hosts/base/etc/shared.conf": root + "/etc/shared.conf",
            "hosts/base/usr-local/bin/tool": root + "/usr/local/bin/tool",
        })

    def test_diff_then_apply_then_up_to_date(self):
        self.write("etc/both.conf", "edited on the system\n", base=self.root)
        self.write("usr/local/bin/tool", "#!/bin/sh\n", base=self.root)  # same content, not executable
        r = self.deploy("t")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(f"changed:  {self.root}/etc/both.conf", r.stdout)
        self.assertIn("-edited on the system", r.stdout)
        self.assertIn(f"mode:     {self.root}/usr/local/bin/tool", r.stdout)
        self.assertIn(f"new:      {self.root}/etc/shared.conf", r.stdout)
        self.assertIn("7 file(s) differ", r.stdout)

        r = self.deploy("t", "apply", answer="n\n")
        self.assertEqual(r.returncode, 1)
        self.assertIn("-edited on the system", r.stdout)  # apply shows the diff before it asks
        self.assertEqual((self.root / "etc/both.conf").read_text(), "edited on the system\n")

        r = self.deploy("t", "apply", answer="y\n")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual((self.root / "etc/both.conf").read_text(), "t's own\n")
        self.assertEqual((self.home / ".config/app.conf").read_text(), "home\n")
        self.assertTrue(os.access(self.root / "usr/local/bin/tool", os.X_OK))
        kept = list((self.root / "var/backups/deploy").iterdir())
        self.assertEqual(len(kept), 1)
        self.assertEqual((kept[0] / "etc/both.conf").read_text(), "edited on the system\n")
        self.assertIn(f"what was there before is in {kept[0]}", r.stdout)

        r = self.deploy("t")
        self.assertIn("t is up to date", r.stdout)

    def test_apply_backs_up_home_files(self):
        self.write(".config/app.conf", "mine\n", base=self.home)
        r = self.deploy("t", "apply", answer="y\n")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        kept = list((self.home / ".local/state/deck/deploy-backups").iterdir())
        self.assertEqual((kept[0] / ".config/app.conf").read_text(), "mine\n")

    def test_apply_marks_uncommitted(self):
        self.write("hosts/t/portage/make.conf", "USE=y\n")
        r = self.deploy("t", "apply", answer="n\n")
        marked = [l for l in r.stdout.splitlines() if "NOT COMMITTED" in l and l.startswith("  ")]
        self.assertEqual(len(marked), 1, r.stdout)
        self.assertIn("/etc/portage/make.conf", marked[0])

    def test_deleted_tracked_file_is_skipped(self):
        (self.repo / "hosts/t/portage/make.conf").unlink()
        r = self.deploy("t", "apply", answer="y\n")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("missing:  hosts/t/portage/make.conf", r.stdout)
        self.assertFalse((self.root / "etc/portage/make.conf").exists())
        self.assertTrue((self.root / "etc/shared.conf").exists())  # the rest went in

    def test_pam_module_must_exist(self):
        self.write("lib64/security/pam_unix.so", "", base=self.root)
        pam = ("# comment\nauth required pam_unix.so nullok\n-session optional pam_absent_ok.so\n"
               "auth include system-auth\naccount [success=1 default=ignore] pam_unix.so\n")
        self.write("hosts/t/etc/pam.d/login", pam)
        self.commit()
        r = self.deploy("t", "apply", answer="y\n")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

        self.write("hosts/t/etc/pam.d/login", pam + "auth [success=1 default=ignore] pam_gone.so try\n")
        self.write("hosts/t/etc/another.conf", "x\n")
        self.commit()
        r = self.deploy("t", "apply", answer="y\n")
        self.assertEqual(r.returncode, 1)
        self.assertIn("pam_gone.so", r.stderr)
        self.assertNotIn("pam_unix.so", r.stderr)
        self.assertFalse((self.root / "etc/another.conf").exists())  # nothing was installed

    def test_fstab_must_verify(self):
        self.write("hosts/t/etc/fstab", "this is not an fstab line\n")
        self.commit()
        r = self.deploy("t", "apply", answer="y\n")
        self.assertEqual(r.returncode, 1)
        self.assertIn("findmnt --verify", r.stderr)
        self.assertFalse((self.root / "etc/fstab").exists())

    def test_untracked(self):
        self.deploy("t", "apply", answer="y\n")
        for rel, text in (("etc/portage/package.use/local", "x\n"), ("etc/portage/gnupg/key", "x\n"),
                          ("etc/portage/savedconfig/k", "x\n"), ("etc/portage/old.conf.disabled", "x\n"),
                          ("etc/portage/sets/tool", "# Written by a tool\nfoo\n"), ("etc/portage/sets/mine", "foo\n"),
                          ("etc/systemd/system/b.service.d/override.conf", "[Unit]\n"),
                          ("etc/systemd/system/ignored.service", "[Unit]\n"), ("etc/other/file", "x\n")):
            self.write(rel, text, base=self.root)
        (self.root / "etc/systemd/system/multi-user.target.wants").mkdir()
        (self.root / "etc/systemd/system/multi-user.target.wants/a.service").symlink_to("../a.service")
        self.write("etc/deck/host.conf", 'DEPLOY_UNTRACKED_IGNORE="/etc/systemd/system/ignored.*"\n', base=self.root)
        r = self.deploy("t", "untracked")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(sorted(r.stdout.split()), sorted(str(self.root / p) for p in (
            "etc/portage/package.use/local", "etc/portage/sets/mine", "etc/systemd/system/b.service.d/override.conf")))

    def test_host_dir_outside_the_repo(self):
        ext = self.tmp / "elsewhere"
        self.write("etc/ext.conf", "external\n", base=ext)
        self.write("inherit", "hosts/base/etc/shared.conf\n", base=ext)
        self.git("init", "-q", ".", cwd=ext)
        self.commit(cwd=ext)
        r = self.deploy("other", "list", env={"DECK_HOST_DIR": str(ext)})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout.splitlines(), [f"{ext}/etc/ext.conf\t{self.root}/etc/ext.conf",
                                                 f"hosts/base/etc/shared.conf\t{self.root}/etc/shared.conf"])

    def test_host_dir_that_is_no_checkout_fails(self):
        ext = self.tmp / "plain"
        self.write("etc/ext.conf", "external\n", base=ext)
        for mode in ("diff", "list", "untracked"):
            r = self.deploy("other", mode, env={"DECK_HOST_DIR": str(ext), "GIT_CEILING_DIRECTORIES": str(self.tmp)})
            self.assertNotEqual(r.returncode, 0, mode)
            self.assertIn("can't list the tracked files", r.stderr)
            self.assertNotIn("up to date", r.stdout)

    def test_inherit_without_tracked_files_fails(self):
        self.write("hosts/base/portage/untracked.conf", "x\n")  # there, but not added to git
        self.write("hosts/t/inherit", "hosts/base/portage\n")
        r = self.deploy("t")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("has no tracked files", r.stderr)
        self.assertNotIn("up to date", r.stdout)
        self.write("hosts/t/inherit", "hosts/base/nonsense\n")
        self.assertNotEqual(self.deploy("t").returncode, 0)


if __name__ == "__main__":
    unittest.main()
