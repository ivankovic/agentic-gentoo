# agentic-gentoo

Tooling for installing and running Gentoo with an agent in the loop.

## Hosts

Records of real installs, kept as reference for the installer:

- [hosts/deck](hosts/deck/) — ThinkPad T14s Gen 4: LUKS2 + btrfs, systemd-boot UKI, binhost, niri.

## Deploying

`./deploy.sh [host]` shows how a host's tracked files differ from this machine; `./deploy.sh
[host] apply` installs them after asking (sudo for `/etc`). The host defaults to `$(hostname)`.
It only copies repo → system and never deletes anything on the system.

## Secrets

This repository is public. Nothing secret goes in: SSH and GPG keys, tokens, Wi-Fi passwords,
NetworkManager connections, `gh` credentials, browser profiles, shell history. Three layers keep
them out:

- **[.gitignore](.gitignore)** ignores the usual secret paths.
- **Git hooks** in [.githooks](.githooks/). `pre-commit` scans the staged files and `pre-push`
  every commit being pushed. Both refuse secret paths (keys, `known_hosts`, `*.nmconnection`,
  `*.psk`, `gh/hosts.yml`, `.netrc`, browser profiles, `*_history`, …) and content that looks like a
  secret (private key blocks, GitHub, GitLab, AWS, Slack, OpenAI, Anthropic and Google tokens,
  `psk=`, `password=`, `Authorization: Bearer`, password hashes). They also run
  [gitleaks](https://github.com/gitleaks/gitleaks) when it is installed. Enable them once per clone:

  ```sh
  git config core.hooksPath .githooks
  ```

  For a hit you have checked is a false positive: `SECRET_SCAN_BYPASS=1 git commit ...`.
  `git commit --no-verify` skips pre-commit, but pre-push still scans the commit.
- **GitHub** secret scanning and push protection are on for the repository.
