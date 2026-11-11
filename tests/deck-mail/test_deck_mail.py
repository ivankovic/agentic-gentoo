"""Tests for hosts/deck/usr-local/bin/deck-mail: its pure functions, the searches it builds, and
its IMAP and WebDAV handling against fakes. Nothing here reads mail, notmuch or the network.

  python3 -m unittest discover -s tests/deck-mail
"""
import contextlib
import imaplib
import io
import json
import pathlib
import sys
import tempfile
import unittest
import urllib.error
import urllib.parse
import urllib.request
import warnings
from importlib.machinery import SourceFileLoader
from importlib.util import module_from_spec, spec_from_loader
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[2] / "hosts/deck/usr-local/bin/deck-mail"


def load():
    sys.dont_write_bytecode = True  # no __pycache__ beside the script
    loader = SourceFileLoader("deck_mail", str(SCRIPT))
    module = module_from_spec(spec_from_loader("deck_mail", loader))
    loader.exec_module(module)
    return module


dm = load()
SHA = "0123456789abcdef" * 4


class Case(unittest.TestCase):
    """Every path the script writes to points into a temporary folder."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmp = pathlib.Path(tmp.name)
        for name, value in (("STATE", self.tmp / "state"), ("MAIL", self.tmp / "Mail"),
                            ("FILTER_LOG", self.tmp / "state/filter.json"),
                            ("ARCHIVE_NAMES", self.tmp / "config/archive-names"),
                            ("SENDERS", self.tmp / "config/senders"),
                            ("CATEGORIES", self.tmp / "config/categories")):
            patch = mock.patch.object(dm, name, value)
            patch.start()
            self.addCleanup(patch.stop)
        (self.tmp / "config").mkdir()

    def names(self, text):
        dm.ARCHIVE_NAMES.write_text(text)
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            return dm.archive_names(), err.getvalue()


class ArchiveNames(Case):
    def test_lines(self):
        names, err = self.names("# comment\n\n"
                                f"sha256:{SHA[:16]} | . | Bills | Power 2026-10\n"
                                r"billing@example\.org | invoice-(\d+) | Bills | Invoice {1}" "\n"
                                "news@example.org | . | - | -\n")
        self.assertEqual(err, "")
        self.assertEqual(len(names), 3)
        self.assertEqual(names[0][0], f"sha256:{SHA[:16]}")
        self.assertEqual(names[0][2:], ("Bills", "Power 2026-10"))
        self.assertTrue(names[1][0].search("Billing <BILLING@example.org>"))
        self.assertEqual(dm.suggested(names, "billing@example.org", "invoice-42"),
                         ("Bills", "Invoice 42"))
        self.assertEqual(dm.suggested(names, "news@example.org", "x"), ("-", "-"))
        self.assertIsNone(dm.suggested(names, "other@example.org", "x"))

    def test_missing_file(self):
        self.assertEqual(dm.archive_names(), [])

    def test_wrong_fields(self):
        with self.assertRaises(SystemExit):
            self.names("a | b | c\n")

    def test_bad_regex(self):
        with self.assertRaises(SystemExit):
            self.names("( | . | Bills | x\n")

    def test_sha256_prefix_rule(self):
        for prefix in ("", "0", SHA[:11], "zz" * 8, SHA + "0"):
            names, err = self.names(f"sha256:{prefix} | . | Bills | x\n")
            self.assertEqual(names, [], prefix)
            self.assertIn("ignored", err)
            self.assertIsNone(dm.confirmed(names, SHA))
        for prefix in (SHA[:12], SHA[:16], SHA, SHA[:16].upper()):
            names, err = self.names(f"sha256:{prefix} | . | Bills | x\n")
            self.assertEqual(err, "")
            self.assertEqual(dm.confirmed(names, SHA), ("Bills", "x"), prefix)


class Confirmed(Case):
    def test_confirmed(self):
        names, _ = self.names(f"sha256:{SHA[:16]} | . | Bills | Power\n"
                              f"sha256:{'f' * 16} | . | - | -\n"
                              "anyone@example.org | . | Home | Suggested only\n")
        self.assertEqual(dm.confirmed(names, SHA), ("Bills", "Power"))
        self.assertEqual(dm.confirmed(names, "f" * 64), ("-", "-"))
        self.assertIsNone(dm.confirmed(names, "a" * 64))


class Names(Case):
    def test_hyphenated(self):
        self.assertEqual(dm.hyphenated("Kuća - račun 12/3"), "Kuca-racun-12-3")
        self.assertEqual(dm.hyphenated("Đorđe Straße"), "Dorde-Strasse")
        self.assertEqual(dm.hyphenated("  --  "), "unnamed")
        self.assertEqual(dm.hyphenated("a.pdf"), "a-pdf")

    def test_one_edit(self):
        self.assertTrue(dm.one_edit("example.org", "exemple.org"))  # changed
        self.assertTrue(dm.one_edit("example.org", "exampl.org"))  # removed
        self.assertTrue(dm.one_edit("example.org", "examplee.org"))  # added
        self.assertTrue(dm.one_edit("xexample.org", "example.org"))
        self.assertFalse(dm.one_edit("example.org", "example.org"))
        self.assertFalse(dm.one_edit("example.org", "exmaple.org"))  # two letters swapped
        self.assertFalse(dm.one_edit("example.org", "example.org.uk"))
        self.assertFalse(dm.one_edit("abc", "xyz"))

    def test_lookalike(self):
        known = {"example.org", "ab.io"}
        self.assertIsNone(dm.lookalike("example.org", known))
        self.assertIsNone(dm.lookalike("mail.example.org", known))  # a known domain's own host
        self.assertEqual(dm.lookalike("examp1e.org", known), "examp1e.org looks like example.org")
        self.assertEqual(dm.lookalike("mail.exampl.org", known), "exampl.org looks like example.org")
        self.assertIn("punycode", dm.lookalike("xn--exmple-cua.org", known))
        self.assertIsNone(dm.lookalike("ac.io", known))  # too short to tell
        self.assertIsNone(dm.lookalike("unrelated.example", known))

    def test_rule_for(self):
        rules = {"allow": {"ann@x.example", "@y.example"}, "block": {"@x.example", "bob@y.example"}}
        self.assertEqual(dm.rule_for(rules, "ann@x.example"), "allow")  # her own line first
        self.assertEqual(dm.rule_for(rules, "mary.ann@x.example"), "block")  # the domain's
        self.assertEqual(dm.rule_for(rules, "bob@y.example"), "block")
        self.assertEqual(dm.rule_for(rules, "eve@y.example"), "allow")
        self.assertIsNone(dm.rule_for(rules, "ann@sub.x.example"))
        self.assertIsNone(dm.rule_for(rules, "ann@z.example"))

    def test_sender_rules(self):
        dm.SENDERS.write_text("# rules\nallow Ann@X.example\nblock @y.example # all of them\n"
                              "nonsense line here\n")
        self.assertEqual(dm.sender_rules(), {"allow": {"ann@x.example"}, "block": {"@y.example"}})


class Block(Case):
    def test_exact_address(self):
        self.assertTrue(dm.sent_by("ann@x.example", "Ann <Ann@X.example>"))
        self.assertTrue(dm.sent_by("ann@x.example", "ann@x.example"))
        self.assertFalse(dm.sent_by("ann@x.example", "Mary Ann <mary.ann@x.example>"))
        self.assertFalse(dm.sent_by("ann@x.example", "ann@x.example.org"))
        self.assertFalse(dm.sent_by("ann@x.example", '"ann@x.example" <eve@y.example>'))
        self.assertFalse(dm.sent_by("ann@x.example", ""))

    def test_exact_domain(self):
        self.assertTrue(dm.sent_by("@x.example", "Ann <ann@X.example>"))
        self.assertFalse(dm.sent_by("@x.example", "ann@sub.x.example"))
        self.assertFalse(dm.sent_by("@x.example", "ann@notx.example"))
        self.assertFalse(dm.sent_by("@x.example", "x.example <eve@y.example>"))
        self.assertFalse(dm.sent_by("@x.example", None))

    def test_from_sender_filters_what_notmuch_found(self):
        def message(i, sender, match=True):
            return {"id": i, "match": match, "headers": {"From": sender}}
        shown = [[[message("1@example.org", "Ann <ann@x.example>"),
                   [[message("2@example.org", "Mary Ann <mary.ann@x.example>"), []],
                    [message("3@example.org", "ann@x.example", match=False), []]]]],
                 [[message("-4@example.org", "ann@x.example"), []]]]
        run = mock.Mock(return_value=mock.Mock(stdout=json.dumps(shown)))
        with mock.patch.object(dm.subprocess, "run", run):
            ids = dm.from_sender("ann@x.example", "from:ann@x.example and tag:inbox")
        self.assertEqual(ids, ["1@example.org", "-4@example.org"])
        self.assertEqual(run.call_args.args[0][0:2], ["notmuch", "show"])
        self.assertEqual(run.call_args.args[0][-1], "from:ann@x.example and tag:inbox")

    def test_screener_block_tags_only_the_exact_sender(self):
        with mock.patch.object(dm, "from_sender", return_value=["-4@example.org"]) as found, \
                mock.patch.object(dm.subprocess, "run") as run, \
                contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(dm.screener(["block", "--", "Ann@x.example"]), 0)
        self.assertIn("block ann@x.example", dm.SENDERS.read_text().splitlines())
        found.assert_called_once_with("ann@x.example",
                                      "from:ann@x.example and tag:inbox and not tag:deleted")
        self.assertEqual(run.call_args.kwargs["input"], "+deleted -- id:-4@example.org\n")
        self.assertEqual(json.loads(out.getvalue()), [["+deleted", ["-4@example.org"]]])


class Operands(Case):
    def test_operands(self):
        self.assertEqual(dm.operands(["a", "b"]), ([], ["a", "b"]))
        self.assertEqual(dm.operands(["--", "-a", "--check"], ("--check",)), ([], ["-a", "--check"]))
        self.assertEqual(dm.operands(["--check", "--", "--check"], ("--check",)),
                         (["--check"], ["--check"]))
        self.assertEqual(dm.operands(["id", "--check"], ("--check",)), (["--check"], ["id"]))
        self.assertEqual(dm.operands(["--", "a", "--", "b"]), ([], ["a", "--", "b"]))

    def file(self, *args):
        with mock.patch.object(dm, "refresh_waiting"), contextlib.redirect_stderr(io.StringIO()):
            return dm.file_pdf(list(args))

    def test_file_takes_dashes_after_separator(self):
        self.assertEqual(self.file("--", SHA, "-Bills", "--forget"), 0)
        self.assertEqual(dm.review_overrides(), {SHA[:16]: ("-Bills", "--forget")})
        self.assertEqual(self.file(SHA, "-"), 0)
        self.assertEqual(dm.review_overrides(), {SHA[:16]: ("-", "-")})
        self.assertEqual(self.file(SHA, "Bills", "Power"), 0)
        self.assertEqual(dm.review_overrides(), {SHA[:16]: ("Bills", "Power")})
        self.assertEqual(self.file(SHA, "--forget"), 0)
        self.assertEqual(dm.review_overrides(), {})
        self.assertEqual(self.file("--", SHA, "--forget"), 2)  # a name with no category
        self.assertEqual(self.file(SHA), 2)
        self.assertEqual(self.file("--", "-x", "-"), 2)  # not a sha256

    def test_why_takes_an_id_with_a_dash(self):
        dm.STATE.mkdir()
        dm.FILTER_LOG.write_text(
            json.dumps({"-odd@example.org": {"sender": "a@x.example", "reasons": ["r"]}}))
        with contextlib.redirect_stdout(io.StringIO()) as out:
            dm.why(["--", "-odd@example.org"])
        self.assertEqual(json.loads(out.getvalue())[0]["message"], "-odd@example.org")


class Categories(Case):
    def test_default_without_file(self):
        self.assertEqual(dm.categories(), dm.CATEGORIES_DEFAULT)
        self.assertEqual(len(dm.CATEGORIES_DEFAULT), 12)

    def test_file(self):
        dm.CATEGORIES.write_text("# mine\nRačuni\n  Kuća  # the house\n\nRačuni\nA | B\n")
        self.assertEqual(dm.categories(), ("Računi", "Kuća", "A B"))

    def test_empty_file_and_too_many(self):
        dm.CATEGORIES.write_text("# nothing\n")
        self.assertEqual(dm.categories(), dm.CATEGORIES_DEFAULT)
        dm.CATEGORIES.write_text("".join(f"c{i}\n" for i in range(14)))
        with contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(len(dm.categories()), 12)
        self.assertIn("first 12", err.getvalue())


class Review(Case):
    def test_update_merges_with_the_file(self):
        dm.ARCHIVE_NAMES.write_text("# head\nann@example.org | . | Home | Letter\n")
        dm.review_update({"a" * 16: ("Bills", "One")})
        # Another writer (deck-mail file, from mmm) meanwhile; then review, with its old picture.
        dm.review_update({"b" * 16: ("Home", "Two")})
        merged = dm.review_update({"c" * 16: ("-", "-"), "a" * 16: None})
        self.assertEqual(merged, {"b" * 16: ("Home", "Two"), "c" * 16: ("-", "-")})
        self.assertEqual(dm.review_overrides(), merged)
        text = dm.ARCHIVE_NAMES.read_text()
        self.assertEqual(text.count(dm.REVIEW_START), 1)
        self.assertTrue(text.startswith("# head\n"))
        self.assertIn("ann@example.org | . | Home | Letter", text)
        self.assertEqual(dm.confirmed(dm.archive_names(), "b" * 64), ("Home", "Two"))
        self.assertFalse(list(dm.ARCHIVE_NAMES.parent.glob("*.tmp")))


class PushQueries(Case):
    def test_new_mail_is_left_alone(self):
        q = dm.push_queries("acct")
        self.assertEqual(set(q), {"archive", "delete", "spam", "unspam"})
        for kind, query in q.items():
            self.assertIn("and not tag:new", query, kind)
        self.assertEqual(q["archive"], 'folder:"acct/INBOX" and not tag:inbox and not tag:new')
        self.assertEqual(q["unspam"], 'folder:"acct/Spam" and not tag:spam and not tag:new')
        self.assertTrue(q["delete"].startswith("tag:deleted and not tag:spam and not tag:new and "))
        self.assertIn('not folder:"acct/Trash"', q["delete"])
        self.assertTrue(q["spam"].startswith("tag:spam and not tag:new and "))
        self.assertIn('not folder:"acct/Spam"', q["spam"])

    def test_push_asks_notmuch_exactly_these(self):
        asked = []
        with mock.patch.object(dm, "pending", lambda q: asked.append(q) or []):
            self.assertTrue(dm.push({"name": "acct"}))
        self.assertEqual(asked, list(dm.push_queries("acct").values()))

    def test_other_searches_leave_new_mail_alone(self):
        self.assertIn("not tag:new", dm.ARCHIVE_QUERY)


class FakeImap:
    """Gmail, as far as push() talks to it: `found` is {Message-ID: [uids]}, `refuse` the UIDs
    whose command gets NO."""

    def __init__(self, found=None, refuse=(), validity=b"7"):
        self.found, self.refuse, self.validity = found or {}, set(refuse), validity
        self.commands, self.searches, self.selected = [], [], None

    def select(self, box):
        self.selected = box
        return "OK", [b"1"]

    def response(self, code):
        return code, [self.validity]

    def uid(self, command, *args):
        if command == "SEARCH":
            self.searches.append(args[-1])
            uids = [x for mid, u in self.found.items() if mid in args[-1] for x in u]
            return "OK", [b" ".join(str(x).encode() for x in uids)]
        self.commands.append((self.selected, command, *args))
        if set(args[0].split(",")) & self.refuse:
            return "NO", [b"refused"]
        return "OK", [None]

    def close(self):
        pass

    def logout(self):
        pass


class Push(Case):
    NAMES = {r"\All": "[Gmail]/All Mail", r"\Trash": "[Gmail]/Bin", r"\Junk": "[Gmail]/Spam"}

    def push(self, imap, todo, files=None):
        kinds = {query: kind for kind, query in dm.push_queries("acct").items()}
        self.told = []
        with mock.patch.object(dm, "pending", lambda q: todo.get(kinds[q], [])), \
                mock.patch.object(dm, "login", lambda a: imap), \
                mock.patch.object(dm, "special", lambda i: self.NAMES), \
                mock.patch.object(dm, "message_files", lambda mid: (files or {}).get(mid, [])), \
                mock.patch.object(dm, "tell", lambda *a: self.told.append(a)), \
                contextlib.redirect_stdout(io.StringIO()) as out:
            ok = dm.push({"name": "acct", "address": "me@example.org"})
        return ok, out.getvalue()

    def state(self):
        f = dm.STATE / "push.json"
        return json.loads(f.read_text()) if f.exists() else {}

    def test_nothing_to_do_makes_no_connection(self):
        with mock.patch.object(dm, "pending", lambda q: []), \
                mock.patch.object(dm, "login", side_effect=AssertionError("no login")):
            self.assertTrue(dm.push({"name": "acct"}))
        self.assertFalse((dm.STATE / "push.json").exists())

    def test_counts_only_what_was_found(self):
        imap = FakeImap({"a@example.org": [11], "d@example.org": [12, 13]})
        ok, out = self.push(imap, {"archive": ["a@example.org", "gone@example.org"],
                                   "delete": ["d@example.org"]})
        self.assertTrue(ok)  # one not found doesn't hold the account up
        self.assertIn("1 archived, 1 moved to the Bin in Gmail", out)
        self.assertIn("archive gone@example.org: not found in Gmail, try 1 of", out)
        self.assertEqual(imap.commands, [
            ('"[Gmail]/All Mail"', "STORE", "11", "-X-GM-LABELS", r"(\Inbox)"),
            ('"[Gmail]/All Mail"', "MOVE", "12,13", '"[Gmail]/Bin"')])
        self.assertEqual(self.state(), {"acct": {"archive gone@example.org": 1}})

    def test_unspam_is_looked_for_in_spam(self):
        imap = FakeImap({"u@example.org": [3]})
        ok, out = self.push(imap, {"unspam": ["u@example.org"]})
        self.assertTrue(ok)
        self.assertEqual(imap.commands, [('"[Gmail]/Spam"', "MOVE", "3", "INBOX")])
        self.assertEqual(imap.searches, ['"<u@example.org>"'])
        self.assertIn("1 back from Spam in Gmail", out)

    def test_gives_up_and_says_so_once(self):
        todo, told = {"archive": ["gone@example.org"]}, []
        for _ in range(dm.PUSH_TRIES):
            ok, _ = self.push(FakeImap(), todo)
            self.assertTrue(ok)
            told += self.told
        self.assertEqual(len(told), 1)
        imap = FakeImap()
        ok, out = self.push(imap, todo)  # given up: not looked for, not told again
        self.assertTrue(ok)
        self.assertEqual((imap.searches, imap.commands, self.told, out), ([], [], [], ""))
        self.assertEqual(self.state(), {"acct": {"archive gone@example.org": dm.PUSH_TRIES}})
        self.push(imap, {})  # no longer waiting (undone on deck, or gone): forgotten
        self.assertEqual(self.state(), {})

    def test_refusal_holds_the_account_until_given_up(self):
        todo, told = {"spam": ["s@example.org"], "archive": ["a@example.org"]}, []
        for i in range(dm.PUSH_TRIES):
            imap = FakeImap({"s@example.org": [5], "a@example.org": [6]}, refuse={"5"})
            ok, out = self.push(imap, todo)
            self.assertEqual(ok, i == dm.PUSH_TRIES - 1)
            self.assertIn("1 archived in Gmail", out)  # the others still go
            self.assertNotIn("reported as spam", out)
            told += self.told
        self.assertEqual(len(told), 1)

    def test_connection_failure_holds_the_account(self):
        class Down(FakeImap):
            def select(self, box):
                raise imaplib.IMAP4.abort("gone")
        ok, out = self.push(Down({"a@example.org": [1]}), {"archive": ["a@example.org"]})
        self.assertFalse(ok)
        self.assertIn("failed", out)
        self.assertEqual(self.state(), {})  # not counted against the message

    def folder(self, box, lines, validity=7):
        d = dm.MAIL / "acct" / box
        (d / "cur").mkdir(parents=True)
        (d / ".mbsyncstate").write_text(
            f"FarUidValidity {validity}\nNearUidValidity 99\nMaxPulledUid 500\n\n" + lines)
        return d

    def test_uid_from_mbsync_record(self):
        d = self.folder("All", "401 3 S\n402 4 FS\n0 5 \n")
        files = {"notmuch-sha1-abc": [str(d / "cur/1.2_3.host,U=3:2,S"),
                                      str(dm.MAIL / "acct/INBOX/cur/9.9_9.host,U=4:2,S")]}
        imap = FakeImap()
        ok, out = self.push(imap, {"archive": ["notmuch-sha1-abc"]}, files)
        self.assertTrue(ok)
        self.assertIn("1 archived", out)
        self.assertEqual(imap.searches, [])  # no Message-ID needed
        self.assertEqual(imap.commands[0][1:3], ("STORE", "401"))

    def test_record_of_another_uidvalidity_is_not_used(self):
        d = self.folder("All", "401 3 S\n", validity=6)
        files = {"a@example.org": [str(d / "cur/1.2_3.host,U=3:2,S")]}
        imap = FakeImap({"a@example.org": [77]})
        self.push(imap, {"archive": ["a@example.org"]}, files)
        self.assertEqual(imap.commands[0][1:3], ("STORE", "77"))

    def test_far_uids(self):
        d = self.folder("Spam", "10 1 S\n11 2 \n0 3 \n12 0 \nbad line\n")
        self.assertEqual(dm.far_uids(d), (7, {1: 10, 2: 11}))
        paths = [d / "cur/a,U=1:2,", d / "new/b,U=2", d / "cur/c,U=3:2,S", d / "cur/no-uid",
                 dm.MAIL / "acct/All/cur/x,U=1:2,S"]
        self.assertEqual(dm.file_uids(map(str, paths), d, dm.far_uids(d)[1]), [10, 11])
        (d / ".mbsyncstate.journal").write_text("")
        self.assertEqual(dm.far_uids(d), (None, {}))
        self.assertEqual(dm.far_uids(dm.MAIL / "acct/None"), (None, {}))


class Response:
    def __init__(self, status, body=b""):
        self.status, self.headers, self.body = status, {}, body

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def read(self):
        return self.body


class FakeDav:
    """A WebDAV server behind urllib's urlopen: `files` by URL path, `head` a status that HEAD
    answers instead, `race` bytes that appear at a path just before the first PUT to it, `put`
    a status that PUT answers instead of storing."""

    def __init__(self, files=None, head=None, race=None, put=None):
        self.files, self.head, self.race, self.put = dict(files or {}), head, race, put
        self.log = []

    def __call__(self, req, timeout=None, context=None):
        method = req.get_method()
        path = urllib.parse.unquote(req.full_url.split("/", 3)[3])
        self.log.append((method, path))
        status = 404
        if method == "MKCOL":
            status = 201
        elif method == "HEAD":
            status = self.head or (200 if path in self.files else 404)
        elif method == "GET" and path in self.files:
            return Response(200, self.files[path])
        elif method == "PUT":
            assert req.get_header("If-none-match") == "*", "a PUT that could replace a file"
            if self.race is not None:
                self.files[path], self.race = self.race, None
            if path in self.files:
                status = 412
            elif self.put:
                status = self.put
            else:
                self.files[path] = req.data
                status = 201
        if status >= 400:
            raise urllib.error.HTTPError(req.full_url, status, "no", {}, io.BytesIO())
        return Response(status)


class Put(Case):
    CONF = {"url": "https://dav.example.org", "user": "u", "server": "webdav", "style": "spaces"}

    def put(self, server, rel="2026/11/a.pdf", data=b"%PDF new", **conf):
        with mock.patch.object(urllib.request, "urlopen", server), \
                warnings.catch_warnings(action="ignore", category=ResourceWarning), \
                mock.patch.object(dm.time, "sleep"):
            return dm.webdav({**self.CONF, **conf}, "secret")[1](rel, data)

    def puts(self, server):
        return [p for m, p in server.log if m == "PUT"]

    def test_new_file(self):
        server = FakeDav()
        self.assertEqual(self.put(server), "2026/11/a.pdf")
        self.assertEqual(server.files, {"2026/11/a.pdf": b"%PDF new"})
        self.assertEqual([p for m, p in server.log if m == "MKCOL"], ["2026", "2026/11"])

    def test_name_taken_gets_the_next(self):
        server = FakeDav({"2026/11/a.pdf": b"%PDF other", "2026/11/a (2).pdf": b"%PDF third"})
        self.assertEqual(self.put(server), "2026/11/a (3).pdf")
        self.assertEqual(server.files["2026/11/a.pdf"], b"%PDF other")
        self.assertEqual(self.put(FakeDav({"2026/11/a.pdf": b"x"}), style="hyphens"),
                         "2026/11/a-2.pdf")

    def test_same_bytes_already_there(self):
        server = FakeDav({"2026/11/a.pdf": b"%PDF new"})
        self.assertEqual(self.put(server), "2026/11/a.pdf")
        self.assertEqual(self.puts(server), [])

    def test_head_other_than_404_is_an_error_not_absence(self):
        for status in (401, 403, 429, 500, 503):
            server = FakeDav({"2026/11/a.pdf": b"%PDF kept"}, head=status)
            with self.assertRaises(OSError, msg=status):
                self.put(server)
            self.assertEqual(self.puts(server), [], status)
            self.assertEqual(server.files, {"2026/11/a.pdf": b"%PDF kept"})

    def test_file_appearing_before_the_put_is_kept(self):
        server = FakeDav(race=b"%PDF someone else's")
        self.assertEqual(self.put(server), "2026/11/a (2).pdf")
        self.assertEqual(server.files, {"2026/11/a.pdf": b"%PDF someone else's",
                                        "2026/11/a (2).pdf": b"%PDF new"})

    def test_same_file_appearing_before_the_put(self):
        server = FakeDav(race=b"%PDF new")
        self.assertEqual(self.put(server), "2026/11/a.pdf")
        self.assertEqual(len(server.files), 1)

    def test_refused_put_fails_at_once_and_server_errors_are_retried(self):
        server = FakeDav(put=403)
        with self.assertRaises(OSError):
            self.put(server)
        self.assertEqual(len(self.puts(server)), 1)
        server = FakeDav(put=502)
        with self.assertRaises(OSError):
            self.put(server)
        self.assertEqual(len(self.puts(server)), 4)


class Writes(Case):
    def test_write_is_whole_and_leaves_no_temporary(self):
        target = self.tmp / "state/archive.json"
        with contextlib.redirect_stdout(io.StringIO()) as out:
            dm.write(target, "{}", quiet=True)
            dm.write(target, '{"a": 1}', quiet=True)
        self.assertEqual(out.getvalue(), "")
        self.assertEqual(json.loads(target.read_text()), {"a": 1})
        self.assertEqual([p.name for p in target.parent.iterdir()], ["archive.json"])
        self.assertEqual(target.stat().st_mode & 0o777, 0o600)


if __name__ == "__main__":
    unittest.main()
