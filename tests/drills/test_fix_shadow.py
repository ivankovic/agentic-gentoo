import contextlib
import io
import json
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest import mock

import support

fs = support.fix_shadow()


class Classifier(unittest.TestCase):
    def test_sysadmin_commands(self):
        for c in ("systemctl status sshd",
                  "sudo cat /etc/fstab",
                  "cd /etc && ip addr",
                  "ls /tmp; df -h",
                  "journalctl -b | grep -i error",
                  "grep foo /etc/hosts | ps aux",
                  "FOO=1 BAR=2 emerge -pv vim",
                  "env LC_ALL=C nice -n 5 emerge -1 vim",
                  "timeout 30 ping -c1 host",
                  "/usr/bin/systemctl restart nginx",
                  "mkfs.ext4 /dev/sdz1",
                  "if ! systemctl is-active -q sshd; then echo down; fi",
                  "echo start\nss -tlnp",
                  "(cd /tmp && du -sh .)",
                  "cat /etc/hosts 2>&1 >/dev/null && lsblk",
                  "cat <<'EOF' | sudo tee /etc/x\nit's text\nEOF",
                  "cat > f <<EOF\nit's text\nEOF\nsystemctl daemon-reload",
                  "bash -c 'cd /etc && systemctl daemon-reload'",
                  "setsid -f xterm -e sh -c 'read x; emerge -1 vim' >/dev/null 2>&1",
                  "true || kill 1"):
            self.assertTrue(fs.sysadmin(c), c)

    def test_words_in_quotes_and_arguments_are_not_commands(self):
        for c in ('git commit -m "fix ip parsing; ps and ss output"',
                  "git commit -m 'du | ss && ip'",
                  "echo ip ss ps du",
                  "grep -r 'systemctl restart' docs/",
                  "cat ps",
                  "ls > ip",
                  "python3 du.py --mount /x",
                  "rg mount src/",
                  "git log --grep='kill -9'",
                  "cat notes.txt # ip",
                  "git commit -q -F - <<'EOF'\nfix: units\n\nip addr was parsed wrongly\nmount it\nEOF",
                  "python3 - <<'EOF'\nimport os\nkill = 1\nEOF\necho done",
                  "cat <<<ip",
                  "bash -c 'echo ip'",
                  "gcc -c mount.c",
                  ""):
            self.assertFalse(fs.sysadmin(c), c)

    def test_an_unclosed_quote_keeps_what_came_before(self):
        self.assertTrue(fs.sysadmin("systemctl status x; echo 'unclosed"))
        self.assertFalse(fs.sysadmin("echo 'unclosed ip"))

    def test_programs(self):
        self.assertEqual(fs.programs("A=1 env B=2 /usr/bin/ip -4 addr | grep inet > out; sudo -u x ls"),
                         ["ip", "grep", "sudo"])

    def test_questions_use_it(self):
        events = [{"kind": "user", "text": "hi"},
                  {"kind": "command", "id": "1", "command": 'git commit -m "ip"', "output": "", "time": ""},
                  {"kind": "command", "id": "2", "command": "ip addr", "output": "", "time": ""}]
        self.assertEqual([ev["id"] for ev, _, _ in fs.questions(events)], ["2"])


class Results(unittest.TestCase):
    def test_a_truncated_line_is_skipped_with_a_warning(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "shadow.jsonl"
            path.write_text('{"id": "a", "match": "same"}\n{"id": "b", "mat\n\n[1]\n{"id": "c", "match": "none"}\n')
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                recs = list(fs.read_lines(path))
            self.assertEqual([r["id"] for r in recs], ["a", "c"])
            self.assertIn(":2:", err.getvalue())
            self.assertIn(":4:", err.getvalue())
            self.assertEqual(list(fs.read_lines(Path(d) / "absent.jsonl")), [])

    def test_append_starts_a_new_line_after_a_truncated_one(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "shadow.jsonl"
            path.write_text('{"id": "a"}\n{"id": "b", "ma')
            fs.append(path, {"id": "c"})
            fs.append(path, {"id": "d"})
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual([r["id"] for r in fs.read_lines(path)], ["a", "c", "d"])
            self.assertTrue(path.read_bytes().endswith(b'{"id": "d"}\n'))

    def test_state(self):
        recs = [{"id": "a", "match": "same", "predicted": "x", "error": None},
                {"id": "b", "match": "none", "predicted": None, "error": "no command: hm"},
                {"id": "c", "match": "none", "predicted": None, "error": "HTTP 500: boom"},   # as once written
                {"id": "d", "match": "error", "error": "HTTP 400: context"},
                {"id": "d", "match": "error", "error": "HTTP 400: context"},
                {"id": "e", "match": "error", "error": "HTTP 500"},
                {"id": "e", "match": "different", "predicted": "y", "error": None}]
        compared, attempts = fs.state(recs)
        self.assertEqual(compared, {"a", "b", "e"})
        self.assertEqual(attempts, {"c": 1, "d": 2})


def session(*ids):
    events = [{"kind": "user", "text": "fix the network"}]
    events += [{"kind": "command", "id": i, "command": "ip addr", "output": "", "time": "2026-01-01T00:00:00Z"}
               for i in ids]
    return [("s1", events)]


class Compare(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.results = Path(self.tmp.name) / "shadow.jsonl"
        self.errors = Path(self.tmp.name) / "shadow-errors.jsonl"

    def night(self, ids, ask):
        with contextlib.redirect_stderr(io.StringIO()):
            compared, attempts = fs.state([*fs.read_lines(self.results), *fs.read_lines(self.errors)])
        with mock.patch.object(fs, "ask", side_effect=ask) as asked:
            counts = fs.compare_all(session(*ids), "http://model", "claude-code", 200, compared, attempts,
                                    self.results, self.errors)
        return counts, asked.call_count

    def test_a_server_error_is_not_a_comparison_and_is_retried(self):
        def failing(url, msgs):
            raise fs.ServerError("HTTP 500: overloaded")

        self.assertEqual(self.night(["a"], failing), ((0, 1), 1))
        self.assertFalse(self.results.exists())
        self.assertEqual(json.loads(self.errors.read_text())["match"], "error")
        # The next night it is asked again, and the answer counts.
        self.assertEqual(self.night(["a"], lambda url, msgs: ("ip addr", None)), ((1, 0), 1))
        rec = json.loads(self.results.read_text())
        self.assertEqual((rec["id"], rec["match"]), ("a", "same"))
        # Compared: not asked a third time.
        self.assertEqual(self.night(["a"], failing), ((0, 0), 0))

    def test_attempts_are_capped(self):
        def failing(url, msgs):
            raise fs.ServerError("HTTP 400: exceeds the available context size")

        for _ in range(fs.MAX_ATTEMPTS):
            self.assertEqual(self.night(["a"], failing), ((0, 1), 1))
        self.assertEqual(self.night(["a"], failing), ((0, 0), 0))
        self.assertFalse(self.results.exists())

    def test_the_models_own_failure_is_a_comparison(self):
        self.assertEqual(self.night(["a"], lambda url, msgs: (None, "no command: hm")), ((1, 0), 1))
        self.assertEqual(json.loads(self.results.read_text())["match"], "none")
        self.assertFalse(self.errors.exists())

    def test_the_limit_counts_failures(self):
        def failing(url, msgs):
            raise fs.ServerError("HTTP 500")

        with mock.patch.object(fs, "ask", side_effect=failing) as asked:
            fs.compare_all(session("a", "b", "c"), "u", "claude-code", 2, set(), {}, self.results, self.errors)
        self.assertEqual(asked.call_count, 2)


class Ask(unittest.TestCase):
    def ask(self, effect):
        with mock.patch.object(fs, "post", side_effect=effect):
            return fs.ask("http://model", [])

    def reply(self, **msg):
        return [{"choices": [{"message": msg}]}]

    def test_http_errors_and_timeouts_are_server_errors(self):
        http = urllib.error.HTTPError("http://model", 500, "boom", {}, io.BytesIO(b"exceed_context_size"))
        for effect in (http, TimeoutError(), urllib.error.URLError(TimeoutError()), [{"error": "x"}]):
            with self.assertRaises(fs.ServerError):
                self.ask(effect)

    def test_an_unreachable_server_is_not_about_the_question(self):
        with self.assertRaises(urllib.error.URLError):
            self.ask(urllib.error.URLError(ConnectionRefusedError()))

    def test_answers(self):
        call = {"function": {"name": "run", "arguments": '{"command": "ip a"}'}}
        self.assertEqual(self.ask(self.reply(tool_calls=[call])), ("ip a", None))
        for arguments in ("[1]", "{", '{"command": 3}'):
            bad = {"function": {"name": "run", "arguments": arguments}}
            self.assertEqual(self.ask(self.reply(tool_calls=[bad])), (None, "bad tool call"))
        self.assertEqual(self.ask(self.reply(content="hm")), (None, "no command: hm"))


if __name__ == "__main__":
    unittest.main()
