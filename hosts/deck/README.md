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
| Desktop  | niri, quickshell (clock overlay + Mod+D omni panel), foot, mako, swaylock, greetd + tuigreet; GURU overlay enabled |
| Fonts    | Iosevka everywhere (Iosevka Term for monospace, Iosevka Etoile for the shell's serif); Symbols Nerd Font for icons; Noto + Noto Color Emoji as fallback |
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
- **Fonts.** Added `media-fonts/symbols-nerd-font`, `noto` and `noto-emoji` for icons and fallback.
  fontconfig maps sans-serif, serif and monospace to Iosevka. `media-fonts/iosevka` has USE
  `iosevka-etoile` for the serif used by the clock, panel, mako and swaylock.
- **Extra keywords:** `gui-wm/niri`, `gui-apps/quickshell` and `app-misc/brightnessctl` (GURU) need
  `~amd64`. quickshell is built with `-crash-handler` (avoids keywording `dev-cpp/cpptrace`) and
  without the Hyprland, i3 and X11 modules.
- **Extra packages** in world beyond the runbook: `www-client/firefox-bin`, `sys-fs/udisks`,
  `sys-boot/efibootmgr`, `dev-util/github-cli`; `app-crypt/pinentry -qt6`.

## Theme

Dark academia: walnut and leather surfaces, parchment text, brass accent. One palette, repeated by hand
in each config; [quickshell/Theme.qml](home/.config/quickshell/Theme.qml) is the reference.

| Name | Hex | Use |
|------|-----|-----|
| espresso | `#1c1612` | niri background, headerbars, deepest surface |
| walnut | `#241d18` | terminal and window background |
| leather | `#2f2620` | raised surfaces: notifications, entries, menus |
| umber / bark | `#3d3129` / `#54453a` | borders, selection, inactive focus ring |
| taupe / linen | `#8a7866` / `#c9b89f` | muted and secondary text |
| parchment | `#ebdfca` | text |
| brass | `#c9a46a` | accent: focus ring, highlights, active toggles |
| antique | `#a67c45` | deeper brass for the floating clock, legible on light and dark windows |
| oxblood · sage · ochre · slate | `#b0645a` `#9ea67e` `#d6ac63` `#8b9eab` | red, green, yellow, blue |

- **Shell.** waybar and fuzzel are gone. Quickshell ([home/.config/quickshell](home/.config/quickshell/))
  draws a click-through clock in the top-right on the top layer, so fullscreen windows cover it, and
  the Mod+D omni panel: clock, battery, Wi-Fi, Bluetooth, power profile and keep-awake toggles,
  volume and brightness, now playing; typing searches apps, actions (lock, suspend, power, toggles,
  profiles, screenshot) and evaluates arithmetic. The panel blurs what's behind it
  (ext-background-effect, supported by niri 26.04).
- **GTK.** `gtk-3.0` and `gtk-4.0` prefer dark and carry `gtk.css` overrides; libadwaita takes the
  palette fully, GTK3 Adwaita only on its main surfaces. Also set once, outside the repo:
  `gsettings set org.gnome.desktop.interface color-scheme prefer-dark` (and `font-name 'Iosevka 11'`,
  `monospace-font-name 'Iosevka Term 11'`), which the portal passes to Firefox and libadwaita.
- **niri.** Brass focus ring, shadows, 10 px rounded corners, `prefer-no-csd`, espresso background.
- **Console.** `vt.default_red/grn/blu` on the kernel command line ([etc/kernel/cmdline](etc/kernel/cmdline)
  and dracut's `kernel_cmdline`) give every VT the foot palette from boot on: the LUKS prompt, the
  text consoles and tuigreet, which uses ochre ("yellow") for borders and prompts.

## Status

- Phases 0–7: done.
- Phase 8: done. Volume, brightness and media keys use niri's default binds (`wpctl`,
  `brightnessctl`, `playerctl`). `fwupdmgr` reports no firmware updates.
- Phase 9: done. SSH keys, git, `gh`, repos cloned to `~/src/<repo>` (same flat layout as
  domaci), developer tools (below). omnidiff builds and all tests pass:
  [bench/2026-10-09-omnidiff.md](bench/2026-10-09-omnidiff.md).

## Developer tools: system vs user

Rule: tools go through Portage (patchable via `/etc/portage/patches`, upgraded with @world, and
own projects can later be deployed from git via a local overlay); only what projects pin lives
in the home directory.

| | System (Portage) | User (`~`) |
|---|---|---|
| Rust | `rust-bin` (Portage builds Rust ebuilds with it), mold, cargo-nextest, cargo-audit | rustup stable + clippy, rustfmt, rust-analyzer, llvm-tools, rust-src, musl target; cargo-llvm-cov, cargo-deb, cargo-about (`cargo install --locked`) |
| JS | nodejs 24 LTS (masked `>=25`; 22 isn't on the binhost) | bun (official installer, `~/.bun`), `node_modules` |
| Python | uv | Python versions and venvs, managed by uv |
| Other | hugo, typst, stylua, tio, go, podman (rootless) + podman-compose | — |

Skipped: Android SDK/JDK (eInk-markdown-editor, possibly dead), zig (only omnidiff's and
friends-and-family-ca's release workflows use it), Nix.

`~/.cargo/config.toml` links with mold but leaves out `target-cpu=native` (the runbook had it):
these repos build binaries that ship.

podman's `containers-common` needs `net-firewall/iptables[nftables]`
([portage/package.use/podman](portage/package.use/podman)).

## SSH and git (not stored here)

Keys and SSH config stay on the machine; this repo is public. Layout, for reference:

- One ed25519 key per destination, generated on deck (`m@deck`): `id_core_ed25519`,
  `id_hetzner_ed25519`, `id_scaleway_ed25519`, `id_github_ed25519`.
- The MikroTik key is shared with core (`id_ed25519_mikrotik`).
- Host keys were taken from core's `known_hosts`, not accepted on first use.
- `gh` authenticated with `--git-protocol ssh --skip-ssh-key`.
- Global git config: [home/.gitconfig](home/.gitconfig). Core's codediff/delta settings are left
  out until codediff is installed.
