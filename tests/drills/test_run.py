import json
import subprocess
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

import support  # noqa: F401
import run


def marked(out, status):
    return f"{out}\n{run.STATUS_MARK}{status}\n"


class ScriptStatus(unittest.TestCase):
    def test_the_scripts_own_status(self):
        self.assertEqual(run.script_status("check.sh", 0, marked("fine", 0)), (0, "fine"))
        self.assertEqual(run.script_status("check.sh", 0, marked("no", 1)), (1, "no"))

    def test_statuses_that_look_like_the_harness_are_the_scripts(self):
        # A check that ends on its own ssh, timeout or missing command says "not fixed".
        for status in (124, 126, 127, 255):
            self.assertEqual(run.script_status("check.sh", 0, marked("", status))[0], status)

    def test_ssh_and_the_time_limit_are_the_harness(self):
        for returncode in (255, 124, 137, 1):
            with self.assertRaises(run.HarnessError):
                run.script_status("check.sh", returncode, "partial output")
        with self.assertRaises(run.HarnessError):   # ended before its status was printed
            run.script_status("check.sh", 0, "partial output")
        with self.assertRaises(run.HarnessError):   # ssh failed after the status line
            run.script_status("check.sh", 255, marked("", 0))

    def test_a_missing_script_is_the_harness(self):
        with self.assertRaisesRegex(run.HarnessError, "not there"):
            run.script_status("/run/drill/check.sh", 0, run.STATUS_MARK + "missing\n")

    def test_the_last_status_line_counts(self):
        out = marked(f"{run.STATUS_MARK}0", 1)
        self.assertEqual(run.script_status("check.sh", 0, out)[0], 1)


class FakeMachine:
    """Machine with ssh answered from a list of (returncode, stdout)."""

    def __init__(self, replies=()):
        self.replies, self.commands, self.stdin = list(replies), [], []

    def ssh(self, command, timeout=None, stdin=None):
        self.commands.append(command)
        self.stdin.append(stdin)
        code, out = self.replies.pop(0) if self.replies else (0, "")
        return subprocess.CompletedProcess([], code, out, "")

    put, script, take_state, give_state = (run.Machine.put, run.Machine.script, run.Machine.take_state,
                                           run.Machine.give_state)


class MachineSteps(unittest.TestCase):
    def test_put_raises_when_the_copy_fails(self):
        with self.assertRaises(run.HarnessError):
            FakeMachine([(255, "")]).put("/run/drill/check.sh", "x")
        FakeMachine([(0, "")]).put("/run/drill/check.sh", "x")

    def test_script_runs_break_under_bash_e_and_returns_its_status(self):
        m = FakeMachine([(0, marked("out", 3))])
        r = m.script("/run/drill/break.sh", "export A=b; ", "bash -e ")
        self.assertEqual((r.returncode, r.stdout), (3, "out"))
        self.assertIn("export A=b; bash -e /run/drill/break.sh; s=$?", m.commands[0])

    def test_script_raises_on_ssh_failure(self):
        with self.assertRaises(run.HarnessError):
            FakeMachine([(255, "")]).script("/run/drill/check.sh")

    def test_state_leaves_the_machine_and_comes_back(self):
        m = FakeMachine([(0, "QUJD"), (0, "")])
        state = m.take_state()
        self.assertIn("rm -rf /var/lib/.drill-*", m.commands[0])
        m.give_state(state)
        self.assertEqual(m.stdin[1], "QUJD")
        with self.assertRaises(run.HarnessError):
            FakeMachine([(1, "")]).take_state()
        with self.assertRaises(run.HarnessError):
            FakeMachine([(2, "")]).give_state("QUJD")

    def test_ssh_limits_the_command_on_the_machine(self):
        m = run.Machine.__new__(run.Machine)
        m.image, m.port = Path("/nonexistent"), 1
        with mock.patch.object(run.subprocess, "run") as sub:
            m.ssh("sleep 1000", timeout=7)
        args, kw = sub.call_args
        self.assertIn(f"timeout -k {run.KILL_AFTER} 7 bash -lc 'sleep 1000'", args[0][-1])
        self.assertGreater(kw["timeout"], 7 + run.KILL_AFTER)


def call(arguments, **extra):
    return {"id": "c1", "type": "function", "function": {"name": "run", "arguments": arguments, **extra}}


class ToolCommand(unittest.TestCase):
    def test_a_good_call(self):
        self.assertEqual(run.tool_command(call('{"command": "ls /"}')), "ls /")

    def test_malformed_calls(self):
        bad = [call("[1, 2]"), call('"ls"'), call("null"), call("42"), call("{not json"), call("{}"),
               call('{"command": ["ls"]}'), call('{"command": null}'), call({"command": "ls"}), call(None),
               {"id": "c1"}, {"function": None}, "run"]
        for c in bad:
            with self.assertRaises(ValueError, msg=repr(c)):
                run.tool_command(c)


class Agent(unittest.TestCase):
    def reply(self, *calls, content=""):
        msg = {"role": "assistant", "content": content}
        if calls:
            msg["tool_calls"] = list(calls)
        return {"choices": [{"message": msg}], "usage": {"prompt_tokens": 1, "completion_tokens": 1}}

    def test_non_dict_arguments_go_back_to_the_model_as_a_tool_error(self):
        replies = [self.reply(call("[1, 2]")), self.reply(call('{"command": "true"}')), self.reply(content="done")]
        m = FakeMachine([(0, "ok")])
        with mock.patch.object(run, "chat", side_effect=replies):
            answer, transcript, tokens, gave_up = run.agent(m, "task", "http://model", False)
        self.assertEqual((answer, gave_up), ("done", False))
        tools = [x for x in transcript if x["role"] == "tool"]
        self.assertTrue(tools[0]["content"].startswith("bad tool call:"))
        self.assertEqual(tools[1]["content"], "ok\n[exit code 0]")
        self.assertEqual(m.commands, ["true"])     # the malformed call ran nothing


class RunDrill(unittest.TestCase):
    def args(self):
        return types.SimpleNamespace(label="t", image_info={"stage3": "x"}, image=Path("/nonexistent"))

    def test_a_drill_that_cannot_be_read_is_an_error_not_an_exception(self):
        with tempfile.TemporaryDirectory() as tmp:
            drill = Path(tmp) / "broken"
            drill.mkdir()
            for text in (None, "task = [", 'category = "x"\ntask = "do ${MISSING}"\n',
                         'category = "x"\ntask = "t"\n[vars]\nA = "@nosuchpool"\n'):
                if text is not None:
                    (drill / "drill.toml").write_text(text)
                rec = run.run_drill(drill, self.args(), 0)
                self.assertEqual(rec["result"], "error", text)
                self.assertEqual((rec["drill"], rec["seed"], rec["image"]), ("broken", 0, {"stage3": "x"}))
                json.dumps(rec)

    def test_image_info(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertIsNone(run.image_info(Path(tmp)))
            (Path(tmp) / "image.json").write_text('{"stage3": "gentoo/stage3@sha256:ab", "built": "d"}\n')
            self.assertEqual(run.image_info(Path(tmp))["stage3"], "gentoo/stage3@sha256:ab")


class Drills(unittest.TestCase):
    def test_every_drill_has_a_variant(self):
        import tomllib
        for d in sorted((support.DRILLS / "drills").iterdir()):
            meta = tomllib.loads((d / "drill.toml").read_text())
            task, env = run.variant(meta, d.name, 0)
            self.assertTrue(task, d.name)


if __name__ == "__main__":
    unittest.main()
