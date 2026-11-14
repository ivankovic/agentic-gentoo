# agentic-gentoo

Tooling for installing and running Gentoo with an agent in the loop.

## Hosts

Records of real installs, kept as reference for the installer:

- [hosts/deck](hosts/deck/) — ThinkPad T14s Gen 4: LUKS2 + btrfs, systemd-boot UKI, binhost, niri.

## Tools

- [tools/deck](tools/deck/) — a TUI listing installed packages by origin (binhost, compiled here,
  prebuilt, noarch), the local patches each carries, and each patch's way upstream, timestamped in
  the host's `upstream.toml`.
- [tools/mmm](tools/mmm/) — a terminal mail reader over notmuch: threads, a screener for new
  senders, composing with a minute to take a mail back.

## Deploying

`./deploy.sh [host]` shows how a host's tracked files differ from this machine; `./deploy.sh
[host] apply` shows the same and installs them after asking (sudo for `/etc`). The host defaults
to `$(hostname)`. It only copies repo → system and never deletes anything on the system.

`apply` runs as you, not under sudo. It marks files whose repo version isn't committed, refuses a
new `/etc/fstab` that fails `findmnt --verify` or a `/etc/pam.d` file naming a module that isn't
installed, and keeps what it overwrites in `/var/backups/deploy/<time>/` (the home's files in
`~/.local/state/deck/deploy-backups/<time>/`). `./deploy.sh [host] untracked` goes the other way:
files in `/etc/portage` and `/etc/systemd/system` that the repo doesn't cover. `./deploy.sh [host]
list` prints every deployed file and its place on the system, for scripts. Tests:
`python3 -m unittest discover -s tests/system`.

## Secrets

This repository is public. Nothing secret goes in: SSH and GPG keys, tokens, Wi-Fi passwords,
NetworkManager connections, `gh` credentials, browser profiles, shell history. Nor do private details: mail addresses, the home network's
addresses, other machines' configuration. Four layers keep them out:

- **[.gitignore](.gitignore)** ignores the usual secret paths.
- **Git hooks** in [.githooks](.githooks/). `pre-commit` scans the staged files and `pre-push`
  every commit being pushed, merges included. Both refuse secret paths (keys, `known_hosts`,
  `*.nmconnection`, `*.psk`, `gh/hosts.yml`, `.netrc`, browser profiles, `*_history`, …: everything
  secret that .gitignore lists) and content that looks like a secret: private key blocks, GitHub,
  GitLab, AWS, Slack, OpenAI, Anthropic and Google tokens, `Authorization: Bearer`, password
  hashes, URLs with a password in them, and assignments to names such as `password`, `psk`,
  `*_KEY`, `*_TOKEN` and `*_SECRET` in any case (`NAME=…`, `name: …`, `"name": "…"`), unless the
  value is a placeholder (`changeme`, `<…>`, `${VAR}`, `xxx`), a file name or code. UTF-16 text is
  decoded first; other binary files are checked for tokens and private strings. They also run
  [gitleaks](https://github.com/gitleaks/gitleaks) when it is installed. `commit-msg` checks the
  message the same way, and `pre-push` the messages being pushed. Private strings such as
  personal email addresses go one per line in `.git/info/scan-secrets-deny` (never committed, so
  the list stays private; matched literally, ignoring case, in content, messages and file
  names): commits and pushes containing them are refused. A push whose commits git cannot list
  is refused too. [tests/scan-secrets/run.sh](tests/scan-secrets/run.sh) tests the scanner.

  The same hooks refuse private details: a mail address, a private network address (10/8,
  172.16/12, 192.168/16, 100.64/10, fc00::/7), a hardware address, an ssh public key, a bank
  account or phone number, coordinates to four decimals; pictures that carry a position or other
  metadata; files over 5 MiB; links to absolute paths; data and leftovers by name (databases,
  logs, `*.jsonl`, mail, captures, agent sessions, documents, `*.orig`, `*.bak`); another host's
  directory under `hosts/`; and an author or committer address that no commit on a remote
  carries yet (a wrong `user.email`). What is public on purpose is allowed by rule in
  [.githooks/scan-allow](.githooks/scan-allow); addresses that already published commits here
  are allowed as mail addresses. `pre-push` then runs every test ([tests/run.sh](tests/run.sh));
  `PRE_PUSH_TESTS=0 git push` skips them.
  Enable the hooks once per clone:

  ```sh
  git config core.hooksPath .githooks
  ```

  For a hit you have checked is a false positive: `SECRET_SCAN_BYPASS=1 git commit ...`.
  `git commit --no-verify` skips pre-commit, but pre-push still scans the commit.
- **CI** ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs the same scanner and gitleaks
  over the whole history at every push, so a clone without the hooks, or a push with
  `--no-verify`, still shows. It has no secrets of its own and a read-only token. The private
  list is not there: only the rules are.
- **GitHub** secret scanning and push protection are on for the repository.

## License

Copyright 2026 Marko Ivankovic. Licensed under
[Creative Commons Attribution-NonCommercial-NoDerivatives 4.0 International](LICENSE)
(CC BY-NC-ND 4.0), except:

- **Ebuilds** keep the license in their header: [gitleaks](hosts/deck/overlay/dev-util/gitleaks/)
  GPL-2, as Gentoo ebuilds are; [omnidiff](hosts/deck/overlay/dev-util/omnidiff/) AGPL-3.0-or-later,
  as upstream ships them.
- **[niri's config.kdl](hosts/deck/home/.config/niri/config.kdl)** is based on niri's default
  configuration and stays under niri's license, GPL-3.0-or-later.
