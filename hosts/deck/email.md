# Email on deck

Status (2026-10-14): installed and configured on deck, waiting for the app passwords; sync is
one way (Gmail to deck) until the first full sync has been checked. Accounts: a personal Gmail
account (28,060 messages) and a Google Workspace account on the user's own domain (43,221
messages); more may follow. Addresses stay out of the repo. Disk: 323 GB free on `/home`.

## Decided

- **Offline**: a full local copy of every account, readable and searchable without a connection;
  mail written offline is sent when back online.
- **Everything syncs**: all history, promotions included. Only Spam stays on the server.
- **One view over all accounts**, plus each account on its own.
- **No Google Cloud OAuth client**: IMAP and SMTP with an **app password** per account.
- **No Gmail labels to sync**: only Gmail's built-ins are in use. Inbox, Starred, read/unread,
  Sent, Drafts and Trash map onto IMAP; the inbox tabs (Primary, Social, Promotions, Updates,
  Forums) exist only in Gmail's own apps. So mbsync loses nothing that matters, and the
  label-aware sync tool once considered isn't needed (see the end).
- Tools through Portage; building and patching are acceptable.

## Setup

```
gmail ────┐ mbsync ┌→ ~/Mail/gmail/      ┐
          │ (every │                     ├→ notmuch → aerc: "All" + one tab per account
workspace ┘ 5 min) └→ ~/Mail/ivankovic/  ┘
                                                 aerc → msmtpq → the right account's SMTP
```

| Layer | Tool | Package |
|---|---|---|
| Sync | mbsync, a user timer every 5 min | `net-mail/isync` (stable) |
| Index, tags, search | notmuch, one database over `~/Mail` | `net-mail/notmuch` (0.39 stable) |
| Reader | aerc: "All" tab (combined inbox, unread, flagged, sent) + one tab per account | `mail-client/aerc` (`~amd64`) |
| Send | msmtp: picks the account by the From address (`--read-envelope-from`); `msmtpq` queues mail written offline | `mail-mta/msmtp` (`~amd64` for current) |
| Secrets | app passwords in `pass` (GPG; gpg-agent caches the passphrase) | `app-admin/pass` (stable) |
| HTML, images | `w3m` renders HTML as text; `chafa` shows image attachments as sixel in aerc's viewer; `:open` sends anything to Firefox or `imv` | `www-client/w3m`, `media-gfx/chafa`, `media-gfx/imv` (stable) |

### Files

| Where | What |
|---|---|
| `usr-local/bin/deck-mail` | `config` writes each account's config; `sync` (the timer) fetches, indexes, sends the queue; `unlock` asks for the GPG passphrase; `send` is aerc's outgoing command (msmtpq) |
| `~/.config/deck-mail/accounts` | on deck only, not in the repo: one line per account, `name address Your Name`; the first is the default sender. After a change: `deck-mail config` |
| generated from it | `~/.config/isyncrc`, `~/.config/msmtp/config`, `~/.config/notmuch/default/config`, `~/.config/aerc/accounts.conf` |
| `home/.config/aerc/` | `aerc.conf`; `all.map`, the "All" tab's searches |
| `home/.config/notmuch/default/hooks/post-new` | tags from Gmail folders: `inbox`, `sent`, `draft`, `deleted` |
| `home/.config/systemd/user/deck-mail.{service,timer}` | `deck-mail sync` 1 min after login, then 5 min after each sync ends |
| `home/.gnupg/gpg-agent.conf` | the passphrase stays cached while used, at most 12 h |
| `portage/package.{use,accept_keywords}/mail` | aerc with notmuch (`~amd64`); pass and imv without X |
| `pass`: `mail/<name>` | each account's app password |

Mail is in `~/Mail/<name>/{INBOX,All,Sent,Drafts,Trash}`; the notmuch index in `~/Mail/.notmuch`,
its own subvolume (out of the snapshots: `notmuch new` rebuilds it). msmtpq's queue and log, and
deck-mail's own state, in `~/.local/state/deck-mail/`. msmtp is the stable 1.8.32; nothing
newer is needed.

**The passphrase**: the timer never asks for it. When gpg-agent doesn't have it (after login,
after 12 h), the sync notifies once ("Mail sync is waiting") and skips; `deck-mail unlock`, or a
`deck-mail sync` in a terminal, asks for it. Sending from aerc asks for it itself if needed;
mail that can't go out waits in the queue for the next sync.

### Gmail over IMAP

Per account, mbsync syncs `INBOX`, `[Gmail]/All Mail`, `[Gmail]/Sent Mail`, `[Gmail]/Drafts` and
`[Gmail]/Trash`. A message in several folders is one message to notmuch.

| Gmail | notmuch |
|---|---|
| Inbox | `inbox` tag (both ways) |
| Starred | `flagged` tag (both ways, maildir flag) |
| read / unread | `unread` tag (both ways, maildir flag) |
| Sent, Drafts | `sent`, `draft`, from their folders |
| Trash | `deleted` |
| inbox tabs | not visible over IMAP; local rules instead (below) |

Now (one way, mbsync `Sync Pull`, `Expunge Near`): Gmail's state comes down; the `post-new`
hook sets `inbox`, `draft` and `deleted` from the folders, so archiving in Gmail archives here.
Changes made on deck stay on deck.

Both ways, next (not built yet):

- **Archive** = remove `inbox`: before mbsync, the INBOX copy is deleted, mbsync expunges it,
  Gmail removes the Inbox label (the All Mail copy stays). Gmail setting: auto-expunge on.
- **Delete** = tag `deleted`: every copy but Trash is deleted; expunged from All Mail, Gmail
  moves it to Trash with the setting "When a message is marked as deleted and expunged from the
  last visible IMAP folder: **Move the message to the Trash**" (not "archive", as first planned).
  Gmail keeps Trash 30 days.
- **Read/unread and starred**: notmuch `synchronize_flags` keeps maildir flags and tags in step;
  mbsync carries the flags.

### Promotions and other bulk mail

All of it comes to disk. To keep it out of the way:

- **Gmail filter** per account: `category:promotions` → *Skip the Inbox* (and "also apply to
  matching conversations" for the existing ones). They stay in All Mail, so they're synced and
  searchable, but out of the inbox everywhere, on the phone too.
- **Local rules** in the notmuch `post-new` hook: tag bulk mail by its headers
  (`List-Unsubscribe`, `Precedence: bulk`, `List-Id`) as `promotions` or `lists`; aerc's views
  leave those out of the inbox. Rules live in the repo and can grow.

### First sync

Gmail allows about 2.5 GB of IMAP download per account per day: a large account takes more than
a day the first time. mbsync resumes where it stopped, so the timer just keeps going. Check each
account's size first (Google storage page). Maildir is many small files: fine on btrfs with zstd;
the notmuch index adds a fraction on top (to measure). Mail stays in the hourly snapshots: it
churns little.

### Adding an account

One block each in the mbsync, msmtp and aerc configs and one password in `pass`; notmuch picks up
the new folder by itself. A non-Google account is the same, minus the Gmail folder names.

## Steps

The user, per account, before setup (about 15 minutes):

1. 2-Step Verification on; an app password (myaccount.google.com → Security).
2. The Workspace account: in the Workspace admin console, IMAP enabled for users and app passwords
   allowed.
3. Gmail settings → Forwarding and POP/IMAP: IMAP on, auto-expunge on, expunged messages moved
   to the Trash. Gmail in another language may name its folders `[Google Mail]/…`: then
   `FOLDERS` in deck-mail needs that.
4. The promotions filter above, applied to existing conversations.
5. Note the account's size.

Then on deck:

1. ~~Keywords and packages~~; a GPG key, `pass init`, the two app passwords
   (`pass insert mail/<name>`). (Done 2026-10-14 except the passwords.)
2. ~~Configs: deck-mail, aerc, notmuch hook, the sync timer~~ (2026-10-14).
3. A first sync **pulling only**: `deck-mail sync` in a terminal (it shows progress and can take
   more than a day: Gmail's daily IMAP limit), then `systemctl --user start deck-mail.timer`;
   checked together; then both ways.
4. A maintenance-tile line when sync has been failing for an hour (`~/.local/state/deck-mail/synced`
   is touched after each good sync).

## Other options considered

| Instead of | Considered | Why not now |
|---|---|---|
| mbsync | lieer (Gmail API, labels as tags) | needs a Google Cloud OAuth client |
| mbsync | OfflineIMAP (`synclabels`) | slow, old, not in Portage; labels not needed |
| notmuch | mu | folder model, Emacs-centred |
| aerc | neomutt | most mature, but much more configuration; weaker images |
| aerc | alot, notmuch-vim, Emacs (notmuch.el, mu4e) | smaller; vim has no images; Emacs is Emacs |
| app passwords | OAuth2 (`cyrus-sasl-xoauth2`, a token helper) | needs a Google Cloud OAuth client |

**Gmail-label-aware IMAP sync** (designed 2026-10-14, shelved): Gmail's IMAP has `X-GM-EXT-1`,
with permanent message IDs (`X-GM-MSGID`), thread IDs, labels readable and writable
(`X-GM-LABELS`), Gmail search (`X-GM-RAW`) and CONDSTORE for "what changed". A Python tool could
sync only All Mail and map labels to tags both ways (three-way merge against the last-synced
state, never deleting permanently, read-only phase first). Not needed while only Gmail's built-in
labels are in use; worth revisiting if custom labels or the inbox tabs as tags become wanted
(`X-GM-RAW "category:promotions"` would give the tabs).
