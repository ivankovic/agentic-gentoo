# mmm

Marko's Mail Machine: reading and writing mail in the terminal, over notmuch. It replaces aerc as the
reader on deck; mbsync, notmuch, msmtp and `deck-mail` stay as they are
([hosts/deck/email.md](../../hosts/deck/email.md)).

```sh
mmm                     # the searches from ~/.config/mmm/searches, the first one open
mmm from:hep date:1y..  # one notmuch query
```

`?` explains what is selected and the keys that work on it; `?` again lists every key.

## What it does

- **Searches** along the top, `1`–`9`, from `~/.config/mmm/searches` (`Name = notmuch query`, a
  line each), with how many need you (unread, or a PDF waiting). `/` types any notmuch query.
- **The list**: threads, newest first, loaded 200 at a time as the cursor nears the end.
- **A thread**: every message, HTML as text (`H` for the plain text, where there is one),
  quotes and signatures dimmed. Reading starts at the first unread message.
- **Links and attachments** are what `Tab` / `Shift+Tab` move between. The footer shows where a
  link goes; behind a tracking or safe-links redirect, where it leads on to. A link whose
  text names one site and whose target is another is marked ⚠. `Enter` opens (links in the
  browser through `xdg-open`, attachments with their program from `$XDG_RUNTIME_DIR/mmm`), `y`
  copies, `w` saves an attachment to `~/Downloads`.
- **Tags**: `a` archive (`-inbox`), `d` delete (`+deleted`; again, back out), `s` star, `m`
  read/unread. `u` undoes the last, on exactly the messages it changed. Opening a thread marks
  it read. `deck-mail sync` carries all of it to Gmail, as before.
- **PDFs to the family archive in Nextcloud**: under each PDF, what became of it: in Nextcloud
  (and where), going at the next sync, left out, or waiting. Nothing goes until it's chosen:
  `t` files it (a form with the category and name, filled in from deck-mail's rules when they
  know the sender, so a known bill is `t`, Enter), `x` leaves it out, `u` undoes. Both act on
  the focused PDF, or on the thread's only waiting one. Mail with a PDF waiting is in the
  Waiting tab and marked ☁ in the list; a tab's count is its mail that needs you, unread or
  waiting. `deck-mail sync` does the sending ([email.md](../../hosts/deck/email.md)).
- **Writing**: `c` a new mail, `r` reply, `R` reply to all, `f` forward (the message being read;
  in the list, the thread's newest). The draft opens in `$VISUAL` / `$EDITOR` (nvim): headers
  (From, To, Cc, Bcc, Subject, and `Attach:` lines, one file each), then your text, then the
  quoted or forwarded mail. Who a reply goes to, from which of your addresses, `Re:` and the
  threading headers are `notmuch reply`'s; a forward carries the mail's attachments along.
- **Address completion**, in the editor (nvim, vim): `Tab` on a To, Cc or Bcc line offers the
  people whose name or address has a word starting with what you typed; `Tab` again for the
  next, `Enter` takes it. People you wrote to come first (a mail sent counts five times one
  received); no-reply addresses and your own are left out. It's `mmm addresses PREFIX`, over a
  list kept in `~/.cache/mmm/addresses` and made again (a second) when the mail has changed.
- **Drafts**: every time you come back from the editor, the draft is kept, as a message in notmuch
  (folder `~/Mail/mmm-drafts`, tag `draft`) and as JSON in `~/.local/state/mmm/drafts`. It shows
  in the Drafts tab, ✎ in the list, and a reply's draft inside its conversation. `q` on the
  review screen leaves it there; `e` resumes it (in the list, the thread's draft; in a thread,
  the message read); `D` discards it. Sending clears it away. Gmail never sees these: mbsync
  only syncs the accounts' own folders, and Gmail's drafts (from the web or phone) show as
  "a draft from Gmail", to finish there.
- **Review before it goes**: From, To, Cc, Bcc, attachments, the language and why, the text as it
  will go, and what's wrong: no recipient, an address that isn't one, a file that isn't there
  (these stop it), no subject, or "attached" / "u prilogu" / "anbei" with nothing attached.
  `e` edits again, `l` steps the language, `f` the From address, `q` keeps it as a draft, `D`
  discards it (y/n).
- **Language and signature**: the signature is `~/.config/mmm/signatures/hr`, `en` or `de`, used as
  written (start it with a `-- ` line for the usual separator); it goes after your text, above
  the quote. The language is the one you wrote in (whatlang, among the three); when the text is
  too short to tell, the one you last wrote to the first recipient in
  (`~/.local/state/mmm/languages`), then the one of the mail you answer, then English. A reply's
  "On … wrote:" line follows the language.
- **Sending**: `y`, and the mail goes a minute later, from a systemd user timer that runs
  `deck-mail send` (msmtpq: queued when offline, sent by the next sync). The footer counts down;
  `u` takes it back to the review screen until then, even after leaving mmm (the outbox file is
  the lock: whichever of `u` and the timer moves it first has it). Then the mail answered is
  tagged `replied`, or `passed` for a forward, and the draft is gone.
- New mail and tag changes from elsewhere show up within two seconds (notmuch's revision is
  checked between keys).

mmm runs `notmuch` for everything and never writes mail files; Gmail keeps the sent copy.

## Not yet

Images. HTML mail: not planned; mail goes as plain text.

Installed through Portage from the local overlay's live ebuild `mail-client/mmm`, built from
this clone (`portage/env/deck-local-source.conf`), like `deck`.
