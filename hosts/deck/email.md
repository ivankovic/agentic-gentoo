# Email on deck: plan

Status (2026-10-14): setup decided, nothing installed yet. Accounts: a personal Gmail
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

Config templates go in the repo (no secrets); passwords stay in `pass` on deck.

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

- **Archive** = remove `inbox`: a notmuch hook deletes the INBOX copy, mbsync expunges it, Gmail
  removes the Inbox label (the All Mail copy stays). Gmail setting: auto-expunge on, "archive the
  message" for expunged messages.
- **Delete** = tag `deleted`: the hook moves the file to the Trash folder; Gmail keeps Trash 30 days.
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
3. Gmail settings → Forwarding and POP/IMAP: IMAP on, auto-expunge on, expunged messages archived.
4. The promotions filter above, applied to existing conversations.
5. Note the account's size.

Then on deck:

1. Keywords and packages; `pass` set up with the two app passwords.
2. Templates in the repo: mbsync, msmtp, aerc, notmuch config and hooks; the sync timer.
3. A first sync **pulling only** (mbsync `Sync Pull`), checked together; then both ways.
4. A maintenance-tile line when sync has been failing for an hour.

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
