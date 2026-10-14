# Email on deck: plan (tabled)

Status: discussed 2026-10-13/14, nothing installed yet. Two accounts for now (personal Gmail and a
Google Workspace account on the user's domain), maybe more later.

## Decided so far

- **Offline mail**: a local copy on deck, readable and searchable without a connection; mail
  written offline is sent when back online.
- **No Google Cloud OAuth client.** No registering with Google for OAuth keys, so no Gmail API
  and no lieer. That leaves IMAP and SMTP with an **app password** per account.
- **Gmail labels should sync both ways** with local tags, so a label set on the phone shows on
  deck and the reverse. mbsync can't do that, hence a sync tool of our own (below).
- Building and patching software is acceptable; tools go through Portage (local overlay).

## Layers

```
 Google ⇄ [sync] ⇄ ~/Mail/<account>/ (maildir) → [notmuch: index + tags] → [aerc]
                                                                              │
                                                      [msmtp / msmtpq] ← send ┘
```

| Layer | Choice | Alternatives considered |
|---|---|---|
| Sync | Own Gmail-label-aware IMAP sync (below) | mbsync (stable, any provider, but labels become duplicate folders and tags stay local); lieer (labels as tags over the Gmail API, needs an OAuth client: ruled out); OfflineIMAP `synclabels` (labels in a header inside each file, slow, not in Portage) |
| Index + tags | notmuch (`net-mail/notmuch`, `USE=python` for the bindings) | mu (folder model, Emacs-centred) |
| Reader | aerc (`mail-client/aerc`, `~amd64`) on notmuch | neomutt (most mature, most configuration, weaker images); alot; notmuch-vim; Emacs notmuch.el / mu4e (best inline HTML and images, but Emacs) |
| Send | msmtp, with `msmtpq` queueing mail written offline (`mail-mta/msmtp`) | aerc's own SMTP (no offline queue) |
| Secrets | app passwords in `pass` (`app-admin/pass`, GPG; gpg-agent caches the passphrase) | |
| New-mail push | IDLE in the sync tool | goimapnotify (GURU), for mbsync accounts |
| Auto-tagging | notmuch `post-new` hook with a rules file | afew (not in Portage) |

A future non-Google account would use mbsync into the same `~/Mail` and the same notmuch
database; aerc shows each account as a tab, notmuch searches all of them (`folder:<account>/**`).

## Images

None of the readers shows images inline in HTML mail: HTML goes through `w3m` as text, remote
images are never fetched (good for privacy). Attached images: aerc's built-in terminal passes
sixel through, so `image/*=chafa -f sixel …` (`media-gfx/chafa`) can show them in the viewer
(to confirm with aerc 0.21 on deck); neomutt via mailcap, full screen. Anything image-heavy: one
key opens it in Firefox or `imv` (`media-gfx/imv`).

Experiment for later: `w3m -sixel` (with `media-libs/libsixel`) inside aerc's terminal might show
images that are attached to the mail itself in rendered HTML.

## The sync tool: Gmail-label-aware IMAP

Gmail's IMAP advertises `X-GM-EXT-1`:

| Extension | Use |
|---|---|
| `X-GM-MSGID` | permanent 64-bit message ID, same in every folder: no duplicates, no confusion |
| `X-GM-THRID` | Gmail's thread ID |
| `X-GM-LABELS` | read labels with FETCH; add/remove with `STORE +X-GM-LABELS` / `-X-GM-LABELS`, system labels included (`\Inbox`, `\Important`, `\Sent`, `\Trash`) |
| `X-GM-RAW` | Gmail search syntax on the server (`older_than:2y`) |
| CONDSTORE | changes since the last sync, via `CHANGEDSINCE` (to confirm early: that label changes count as changes) |
| IDLE | instant notice of new mail |

Design (as lieer does over the API, here over IMAP):

- **Only `[Gmail]/All Mail`** (everything but Spam and Trash), plus Spam/Trash if wanted.
- **Pull:** `UID FETCH … (X-GM-MSGID X-GM-LABELS FLAGS) (CHANGEDSINCE <modseq>)`; new messages
  downloaded into the maildir and added to notmuch; deletions found by comparing UID lists.
- **Push:** notmuch's revision counter (`lastmod:`) gives every local tag change since the last
  sync in one query; each becomes a label or flag STORE.
- **Merge:** a small state database keeps each message's labels at the last sync, so both sides'
  changes merge label by label. Only "added on one side, removed on the other" conflicts; the
  server wins.
- **Mapping:**

  | Gmail | notmuch |
  |---|---|
  | `\Inbox` | `inbox` |
  | no `\Seen` flag | `unread` |
  | `\Flagged` (starred) | `flagged` |
  | `\Answered` | `replied` |
  | `\Important`, `\Sent` | `important`, `sent` |
  | user labels | same-name tags |

  notmuch's automatic tags (`attachment`, `signed`, `encrypted`) stay local.
- **Delete** = add `\Trash`. Never EXPUNGE or delete permanently; Gmail keeps Trash 30 days.
- **UIDVALIDITY change:** re-match by `X-GM-MSGID`, no re-download.
- **Language:** Python: `imaplib` sends the X-GM commands, and has IDLE since Python 3.14
  (deck runs 3.14); notmuch's official bindings. In the local overlay with an ebuild and tests.
  Rough size 1000–1500 lines with tests.
- Sent mail appears in Sent by itself on the next pull. Drafts stay local at first.

### Phases and safety

1. **App passwords first**, before any code: 2-Step Verification on both accounts; on Workspace
   the admin console must allow app passwords and IMAP. Confirm both log in over IMAP.
2. **Read-only** (`EXAMINE`, which can't change the server) for about a week: already a full
   searchable offline copy with labels as tags.
3. **Writing**, behind a dry-run mode and a log of every server change.
4. **Guards:** no permanent deletion ever; stop if one run would change more than N messages;
   the local copy is in the hourly btrfs snapshots.

Gmail caps IMAP downloads at about 2.5 GB per day per account: a large mailbox takes days for the
first sync, so start with recent years and fetch older mail in the background.

## Open decisions

1. How much history: everything, or the last N years first.
2. Sync Spam and Trash, or skip them.
3. A name for the tool (working name `gmsync`).
4. Try the inline-image experiment once the basics work?

## Packages (as of 2026-10-14)

| Package | Version, keyword |
|---|---|
| `net-mail/notmuch` | 0.39 stable, 0.40 `~amd64` |
| `mail-client/aerc` | 0.21.0 `~amd64` |
| `mail-mta/msmtp` | 1.8.34 `~amd64` (older stable) |
| `net-mail/isync` | 1.5.1 stable (for non-Google accounts) |
| `app-admin/pass` | 1.7.4 stable |
| `media-gfx/chafa`, `media-gfx/imv`, `www-client/w3m`, `media-libs/libsixel` | stable |
