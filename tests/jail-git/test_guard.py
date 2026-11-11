"""The host git guard (hosts/deck/usr-local/bin/git), in a home of its own.

Each test gets a temporary HOME with src/ in it; the wrapper is run from the repository by path
and runs the real git (/usr/bin/git) itself. What a jailed program would do is done here with the
real git or by writing files. Nothing in the real home is read or written.

    python3 -m unittest discover -s tests/jail-git
"""
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

BIN = pathlib.Path(__file__).resolve().parents[2] / "hosts/deck/usr-local/bin"
GUARD = BIN / "git"
REAL = "/usr/bin/git"
REFUSED = 128


@unittest.skipUnless(os.access(REAL, os.X_OK), "no /usr/bin/git")
class Guard(unittest.TestCase):
    def setUp(self):
        self.home = pathlib.Path(tempfile.mkdtemp(prefix="jail-git-test."))
        self.addCleanup(shutil.rmtree, self.home, ignore_errors=True)
        (self.home / "src").mkdir()
        (self.home / ".gitconfig").write_text(
            "[user]\n\tname = Test\n\temail = test@example.invalid\n[init]\n\tdefaultBranch = main\n"
            "[advice]\n\tdetachedHead = false\n")
        self.env = {"HOME": str(self.home), "PATH": "/usr/bin:/bin", "GIT_CONFIG_NOSYSTEM": "1",
                    "LC_ALL": "C"}
        self.repo = self.home / "src/repo"
        self.git("init", "-q", str(self.repo), cwd=self.home / "src")
        (self.repo / "file").write_text("one\n")
        self.git("add", "file")
        self.git("commit", "-q", "-m", "one")

    def run_git(self, exe, *args, cwd=None, check=True):
        p = subprocess.run([str(exe), *args], cwd=cwd or self.repo, env=self.env, text=True,
                           capture_output=True, stdin=subprocess.DEVNULL)
        if check:
            self.assertEqual(p.returncode, 0, f"git {' '.join(args)}: {p.stderr}")
        return p

    def git(self, *args, **kw):
        """Host git: through the guard."""
        return self.run_git(GUARD, *args, **kw)

    def real(self, *args, **kw):
        """Git as a jailed program has it: no guard."""
        return self.run_git(REAL, *args, **kw)

    def assertRefused(self, *args, **kw):
        p = self.git(*args, check=False, **kw)
        self.assertEqual(p.returncode, REFUSED, p.stderr)
        self.assertIn("trust", p.stderr)

    def hook(self, path, marker):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"#!/bin/sh\necho ran >>'{self.home}/{marker}'\n")
        path.chmod(0o755)

    def ran(self, marker):
        return (self.home / marker).exists()

    def tracked_hooks(self):
        """A repository whose hooks are tracked files (.githooks), trusted, and a branch `other`
        made without the guard where post-checkout and post-merge differ."""
        self.hook(self.repo / ".githooks/pre-commit", "pre-commit-main")
        self.hook(self.repo / ".githooks/post-checkout", "post-checkout-main")
        self.git("add", ".githooks")
        self.git("commit", "-q", "-m", "hooks")
        self.git("config", "core.hooksPath", ".githooks")
        self.git("trust")
        self.git("branch", "plain")
        self.real("checkout", "-q", "-b", "other")
        self.hook(self.repo / ".githooks/post-checkout", "post-checkout-other")
        self.hook(self.repo / ".githooks/post-merge", "post-merge-other")
        self.real("-c", "core.hooksPath=/dev/null", "commit", "-q", "-a", "-m", "other hooks")
        self.real("add", ".githooks")
        self.real("-c", "core.hooksPath=/dev/null", "commit", "-q", "-m", "other hooks, new file")
        self.real("-c", "core.hooksPath=/dev/null", "checkout", "-q", "main")
        for m in ("pre-commit-main", "post-checkout-main"):
            (self.home / m).unlink(missing_ok=True)
        self.git("status")

    # Everyday use stays without questions.

    def test_commit_branch_switch_and_config_need_no_trust(self):
        self.git("branch", "b")
        self.git("switch", "-q", "b")
        (self.repo / "file").write_text("two\n")
        self.git("commit", "-q", "-a", "-m", "two")
        self.git("checkout", "-q", "main")
        self.git("merge", "-q", "b")
        self.git("config", "user.name", "Other")
        self.git("remote", "add", "origin", "https://example.invalid/x.git")
        self.git("status")
        self.git("log", "--oneline")

    def test_clone_and_init_by_host_git_are_trusted(self):
        self.git("clone", "-q", str(self.repo), "copy", cwd=self.home / "src")
        self.git("status", cwd=self.home / "src/copy")
        self.git("init", "-q", "new", cwd=self.home / "src")
        self.git("status", cwd=self.home / "src/new")

    def test_outside_the_work_roots_git_is_not_guarded(self):
        other = self.home / "elsewhere"
        self.real("init", "-q", str(other), cwd=self.home)
        self.git("status", cwd=other)

    def test_work_roots_file_adds_a_root(self):
        (self.home / "uni").mkdir()
        (self.home / ".config/jail").mkdir(parents=True)
        (self.home / ".config/jail/roots").write_text("# more roots\nuni\n")
        self.real("init", "-q", str(self.home / "uni/repo"), cwd=self.home)
        self.assertRefused("status", cwd=self.home / "uni/repo")

    # What host git didn't leave is refused.

    def test_repository_not_seen_before_is_refused(self):
        self.real("init", "-q", str(self.home / "src/made"), cwd=self.home)
        self.assertRefused("status", cwd=self.home / "src/made")
        self.git("trust", cwd=self.home / "src/made")
        self.git("status", cwd=self.home / "src/made")

    def test_new_hook_is_refused(self):
        self.hook(self.repo / ".git/hooks/pre-commit", "new-hook")
        self.assertRefused("status")
        self.assertRefused("commit", "-q", "--allow-empty", "-m", "x")
        self.assertFalse(self.ran("new-hook"))
        self.git("trust")
        self.git("status")

    def test_changed_hook_is_refused(self):
        self.hook(self.repo / ".git/hooks/pre-commit", "hook-a")
        self.git("trust")
        self.git("status")
        self.hook(self.repo / ".git/hooks/pre-commit", "hook-b")
        self.assertRefused("status")
        self.assertFalse(self.ran("hook-b"))

    def test_changed_config_is_refused(self):
        with open(self.repo / ".git/config", "a") as f:
            f.write("[core]\n\tpager = cat\n")
        self.assertRefused("status")

    def test_commondir_file_is_refused(self):
        (self.repo / ".git/commondir").write_text(".\n")
        self.assertRefused("status")

    def test_hooks_directory_made_later_is_refused(self):
        self.git("config", "core.hooksPath", ".hk")
        self.git("trust")
        self.git("log", "-1")  # writes nothing in .git
        self.hook(self.repo / ".hk/pre-commit", "late-hook")
        self.assertRefused("log", "-1")
        self.assertFalse(self.ran("late-hook"))

    def test_init_again_does_not_trust_what_is_there(self):
        with open(self.repo / ".git/config", "a") as f:
            f.write("[core]\n\tpager = cat\n")
        self.git("init", "-q", check=False)
        self.assertRefused("status")

    def test_setting_a_hooks_directory_is_not_trusted_unseen(self):
        self.hook(self.repo / "tools/pre-commit", "tools-hook")
        self.git("config", "core.hooksPath", "tools")
        self.assertRefused("status")
        self.assertFalse(self.ran("tools-hook"))

    # Bare repositories are only used when named.

    def bare(self, where):
        self.real("init", "-q", "--bare", str(where), cwd=self.home)
        with open(where / "config", "a") as f:
            f.write(f"[alias]\n\tmark = !touch '{self.home}/bare-used'\n")

    def test_embedded_bare_repository_is_not_used(self):
        self.bare(self.repo / "vendor/thing")
        p = self.git("mark", cwd=self.repo / "vendor/thing", check=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertFalse(self.ran("bare-used"))
        p = self.git("-C", str(self.repo / "vendor/thing"), "rev-parse", "--git-dir", check=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("bare", p.stderr)

    def test_bare_repository_in_a_work_root_is_not_used(self):
        self.bare(self.home / "src/thing.git")
        p = self.git("mark", cwd=self.home / "src/thing.git", check=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertFalse(self.ran("bare-used"))

    def test_named_bare_repository_is_checked_like_any_other(self):
        self.bare(self.home / "src/thing.git")
        self.assertRefused("--git-dir", str(self.home / "src/thing.git"), "mark", cwd=self.home)
        self.assertFalse(self.ran("bare-used"))
        env = dict(self.env, GIT_DIR=str(self.home / "src/thing.git"))
        p = subprocess.run([str(GUARD), "mark"], cwd=self.home, env=env, capture_output=True)
        self.assertEqual(p.returncode, REFUSED)
        self.assertFalse(self.ran("bare-used"))

    def test_git_directory_worktrees_and_submodules_still_work(self):
        self.git("rev-parse", "--git-dir", cwd=self.repo / ".git")
        self.git("--git-dir", str(self.repo / ".git"), "log", "--oneline", cwd=self.home)
        # A worktree or submodule has a record of its own: trusted once, as before.
        self.git("worktree", "add", "-q", str(self.home / "src/tree"))
        self.git("trust", cwd=self.home / "src/tree")
        self.git("status", cwd=self.home / "src/tree")
        self.git("-c", "protocol.file.allow=always", "submodule", "--quiet", "add",
                 str(self.home / "src/tree"), "sub")
        self.git("trust", cwd=self.repo / "sub")
        self.git("status", cwd=self.repo / "sub")
        self.git("status")

    # Hooks that are tracked files.

    def test_commit_runs_trusted_tracked_hooks(self):
        self.tracked_hooks()
        self.git("commit", "-q", "--allow-empty", "-m", "x")
        self.assertTrue(self.ran("pre-commit-main"))

    def test_switch_between_branches_with_the_same_hooks_needs_no_trust(self):
        self.tracked_hooks()
        self.git("switch", "-q", "plain")
        self.git("status")
        self.git("checkout", "-q", "main")
        self.git("status")
        self.assertFalse(self.ran("post-checkout-main"))  # hooks in the worktree: commit and push only

    def changed_hooks_not_run_not_trusted(self, *command):
        self.tracked_hooks()
        self.git(*command)
        for m in ("post-checkout-other", "post-merge-other", "post-checkout-main"):
            self.assertFalse(self.ran(m), m)
        self.assertRefused("status")
        self.assertRefused("commit", "-q", "--allow-empty", "-m", "x")
        self.assertFalse(self.ran("pre-commit-main"))
        self.git("trust")
        self.git("status")

    def test_switch_to_changed_hooks_does_not_run_or_trust_them(self):
        self.changed_hooks_not_run_not_trusted("switch", "-q", "other")

    def test_checkout_of_changed_hooks_does_not_run_or_trust_them(self):
        self.changed_hooks_not_run_not_trusted("checkout", "-q", "other")

    def test_merge_of_changed_hooks_does_not_run_or_trust_them(self):
        self.changed_hooks_not_run_not_trusted("merge", "-q", "other")

    def test_reset_to_changed_hooks_does_not_run_or_trust_them(self):
        self.changed_hooks_not_run_not_trusted("reset", "-q", "--hard", "other")

    def test_alias_for_checkout_does_not_run_changed_hooks(self):
        with open(self.home / ".gitconfig", "a") as f:
            f.write("[alias]\n\tco = checkout\n\tci = commit\n")
        self.changed_hooks_not_run_not_trusted("co", "-q", "other")

    def test_alias_for_commit_runs_trusted_tracked_hooks(self):
        with open(self.home / ".gitconfig", "a") as f:
            f.write("[alias]\n\tci = commit\n")
        self.tracked_hooks()
        self.git("ci", "-q", "--allow-empty", "-m", "x")
        self.assertTrue(self.ran("pre-commit-main"))

    def test_hooks_in_the_git_directory_run_on_checkout(self):
        self.hook(self.repo / ".git/hooks/post-checkout", "post-checkout-gitdir")
        self.git("trust")
        self.git("branch", "b")
        self.git("checkout", "-q", "b")
        self.assertTrue(self.ran("post-checkout-gitdir"))
        self.git("status")

    def test_in_the_jail_it_is_plain_git(self):
        self.real("init", "-q", str(self.home / "src/made"), cwd=self.home)
        env = dict(self.env, JAIL="1")
        p = subprocess.run([str(GUARD), "status"], cwd=self.home / "src/made", env=env,
                           capture_output=True)
        self.assertEqual(p.returncode, 0)


if __name__ == "__main__":
    unittest.main()
