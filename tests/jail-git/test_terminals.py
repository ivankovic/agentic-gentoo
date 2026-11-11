"""deck-terminals: which command line is recorded for a terminal whose foreground program is jailed.

The process table is made up here (pid: parent, argv, jailed); nothing is read from /proc and
nothing is written.

    python3 -m unittest discover -s tests/jail-git
"""
import importlib.machinery
import importlib.util
import pathlib
import sys
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[2] / "hosts/deck/usr-local/bin/deck-terminals"


def load():
    loader = importlib.machinery.SourceFileLoader("deck_terminals", str(SCRIPT))
    spec = importlib.util.spec_from_loader("deck_terminals", loader)
    module = importlib.util.module_from_spec(spec)
    dont = sys.dont_write_bytecode
    sys.dont_write_bytecode = True  # no __pycache__ next to the script
    try:
        loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = dont
    return module


SHELL = 100
JAIL = ["/bin/bash", "/usr/local/bin/jail"]


class Terminals(unittest.TestCase):
    def setUp(self):
        self.dt = load()
        self.procs = {SHELL: (10, ["bash"], False)}
        self.dt.parent = lambda pid: self.procs[pid][0] if pid in self.procs else 0
        self.dt.argv = lambda pid: list(self.procs[pid][1]) if pid in self.procs else []
        self.dt.in_jail = lambda pid: self.procs[pid][2] if pid in self.procs else True
        self.dt.children = lambda pid: [SHELL] if pid == 1 else []
        self.dt.os.readlink = lambda path: "/home/user/src/project"
        self.dt.claude_session = lambda *a: None

    def jail(self, *args, pid=200):
        """A jail started from the shell: the script outside, pasta and bwrap inside; returns the
        pid jailed programs hang under."""
        self.procs[pid] = (SHELL, [*JAIL, *args], False)
        self.procs[pid + 1] = (pid, ["/usr/bin/pasta", "--quiet"], True)
        self.procs[pid + 2] = (pid + 1, ["bwrap", "--unshare-user"], True)
        self.procs[pid + 3] = (pid + 2, ["bwrap", "--unshare-user"], True)
        return pid + 3

    def recorded(self, fg):
        self.dt.foreground = lambda shell: fg
        t = self.dt.terminal({"pid": 1, "title": "", "is_focused": False})
        return t["cmd"], t["run"]

    def test_host_editor_runs_again(self):
        self.procs[300] = (SHELL, ["nvim", "notes.md"], False)
        self.assertEqual(self.recorded(300), ("nvim notes.md", True))

    def test_jail_in_the_foreground_comes_back_as_the_jail(self):
        self.jail("nvim", "notes.md")
        self.assertEqual(self.recorded(200), ("jail nvim notes.md", True))

    def test_editor_from_a_jail_shell_comes_back_as_the_jail_shell(self):
        top = self.jail()
        self.procs[300] = (top, ["/bin/bash", "-i"], True)
        self.procs[301] = (300, ["nvim", "notes.md"], True)
        self.assertEqual(self.recorded(301), ("jail", True))
        self.assertEqual(self.recorded(300), ("jail", True))

    def test_jailed_foreground_program_is_recorded_as_the_jail_command(self):
        top = self.jail("claude")
        self.procs[300] = (top, ["claude"], True)
        self.procs[301] = (300, ["ssh", "host.example"], True)
        self.assertEqual(self.recorded(301), ("jail claude --continue", True))

    def test_jailed_program_calling_itself_jail_is_not_taken_for_one(self):
        top = self.jail("make")
        self.procs[300] = (top, ["/bin/bash", "/tmp/jail", "ssh", "host.example"], True)
        self.procs[301] = (300, ["ssh", "host.example"], True)
        self.assertEqual(self.recorded(301), ("jail make", False))
        self.assertEqual(self.recorded(300), ("jail make", False))

    def test_jailed_program_with_no_jail_above_it_is_not_recorded(self):
        self.procs[300] = (1, ["nvim", "notes.md"], True)  # its jail is gone
        self.assertEqual(self.recorded(300), (None, False))
        self.procs[301] = (SHELL, ["nvim", "notes.md"], True)  # no jail script between
        self.assertEqual(self.recorded(301), (None, False))

    def test_process_that_cannot_be_looked_at_is_not_recorded(self):
        self.assertEqual(self.recorded(999), (None, False))

    def test_jail_options_are_kept(self):
        self.procs[150] = (SHELL, [*JAIL, "--rw", "/data", "nvim"], False)
        self.procs[151] = (150, ["/usr/bin/pasta"], True)
        self.procs[152] = (151, ["nvim"], True)
        self.assertEqual(self.recorded(152), ("jail --rw /data nvim", False))


if __name__ == "__main__":
    unittest.main()
