# deck — ThinkPad T14s Gen 4

First real-hardware install for agentic-gentoo, done by hand from the runbook
"Gentoo install runbook — ThinkPad T14s Gen 4" (Phases 0–9), summarised in [runbook.md](runbook.md).
This directory records what was actually configured, so the installer can be checked against it.

Recorded 2026-10-09.

## Hardware

| Part     | Value |
|----------|-------|
| Model    | ThinkPad T14s Gen 4, BIOS N3PET33W (1.24) |
| CPU      | Intel Core i7-1370P (20 threads), Iris Xe graphics |
| RAM      | 32 GB |
| Disk     | KIOXIA KXG8AZNV512G, 512 GB NVMe |
| Wi-Fi    | Intel Raptor Lake CNVi (iwlwifi) |
| Audio    | Intel Raptor Lake cAVS (SOF) |

## System

| Area     | Choice |
|----------|--------|
| Profile  | `default/linux/amd64/23.0/desktop/systemd` ([state/profile](state/profile)) |
| Disk     | GPT: 1 GiB EFI on `/efi` + LUKS2 (argon2id) → btrfs, `noatime,compress=zstd:1` |
| Subvols  | `@` `/`, `@home`, `@snapshots` `/.snapshots`, `@var_log`, `@var_tmp` |
| Swap     | zram, ram/2, zstd |
| Boot     | systemd-boot + UKI (installkernel → dracut) |
| Kernel   | `sys-kernel/gentoo-kernel` from the binhost ([state/kernel](state/kernel)) |
| Packages | Official binhost, x86-64-v3; `-march=native` for source builds |
| Network  | NetworkManager with iwd |
| Desktop  | niri, waybar, foot, fuzzel, mako, greetd + tuigreet; GURU overlay enabled |
| Fonts    | Iosevka everywhere (Iosevka Term for monospace); Symbols Nerd Font for icons; Noto + Noto Color Emoji as fallback |
| Laptop   | power-profiles-daemon, thermald, fwupd, battery charge 75–80 % |

## Files

Paths mirror the `~/system` layout proposed in research overview §2.5.

| Here | Deployed to |
|------|-------------|
| `portage/` | `/etc/portage/` |
| `etc/` | `/etc/` |
| `home/` | `~` (user `m`) |
| `state/world` | `/var/lib/portage/world` (observed, not deployed) |

`etc/kernel/cmdline`, `etc/dracut.conf.d/10-luks.conf` and `etc/fstab` contain this machine's
LUKS and filesystem UUIDs. Replace them on any other machine.

## Where deck differs from the runbook

- **Keyboard.** Console and LUKS prompt use `KEYMAP=us`, not `sg`. niri uses `us,ch,hr`, US by
  default; Mod+Space / Mod+Shift+Space cycle layouts.
- **Display scale.** No `output "eDP-1"` scale set yet; 1.25 is deferred.
- **greetd user** is `greetd` (the Gentoo package's account), not `greeter` as in the runbook.
- **User** is `m`, not `marko`.
- **Fonts.** Added `media-fonts/symbols-nerd-font`, `noto` and `noto-emoji`; without an icon font the
  default waybar config shows squares. fontconfig maps sans-serif, serif and monospace to Iosevka.
- **Extra keywords:** `gui-wm/niri` and `gui-apps/fuzzel` need `~amd64`.
- **Extra packages** in world beyond the runbook: `www-client/firefox-bin`, `sys-fs/udisks`,
  `sys-boot/efibootmgr`, `dev-util/github-cli`; `app-crypt/pinentry -qt6`.

## Status

- Phases 0–7: done.
- Phase 8: done except `app-misc/brightnessctl` (GURU, needs `~amd64`), so brightness keys don't
  work yet. `fwupdmgr` reports no firmware updates.
- Phase 9: in progress. Done: SSH keys, git, `gh`, repos cloned to `~/src/<repo>` (same flat
  layout as domaci). Next: rustup, mold, cmake, ninja, tio, cargo config.

## SSH and git (not stored here)

Keys and SSH config stay on the machine; this repo is public. Layout, for reference:

- One ed25519 key per destination, generated on deck (`m@deck`): `id_core_ed25519`,
  `id_hetzner_ed25519`, `id_scaleway_ed25519`, `id_github_ed25519`.
- The MikroTik key is shared with core (`id_ed25519_mikrotik`).
- Host keys were taken from core's `known_hosts`, not accepted on first use.
- `gh` authenticated with `--git-protocol ssh --skip-ssh-key`.
- Global git config: [home/.gitconfig](home/.gitconfig). Core's codediff/delta settings are left
  out until codediff is installed.
