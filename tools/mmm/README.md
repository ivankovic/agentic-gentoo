# mmm

Marko's Mail Machine: reading mail in the terminal, over notmuch. It replaces aerc as the
reader on deck; mbsync, notmuch, msmtp and `deck-mail` stay as they are
([hosts/deck/email.md](../../hosts/deck/email.md)).

```sh
mmm                     # the searches from ~/.config/mmm/searches, the first one open
mmm from:hep date:1y..  # one notmuch query
```

`?` explains what is selected and the keys that work on it; `?` again lists every key.

## What it does

- **Searches** along the top, `1`–`9`, from `~/.config/mmm/searches` (`Name = notmuch query`, a
  line each), with their unread counts. `/` types any notmuch query.
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
- New mail and tag changes from elsewhere show up within two seconds (notmuch's revision is
  checked between keys).

mmm runs `notmuch` for everything and never writes mail files.

## Not yet

Writing: compose, reply, forward, with the review screen, language detection and signature,
y/n and a one-minute undo. Then address completion, drafts, the PDF archive review in the
message view, images.

Installed through Portage from the local overlay's live ebuild `mail-client/mmm`, built from
this clone (`portage/env/deck-local-source.conf`), like `deck`.
