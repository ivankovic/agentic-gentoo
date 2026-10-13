# deck — ThinkPad T14s Gen 4

First real-hardware install for agentic-gentoo, done from the install runbook in
[runbook.md](runbook.md) (Phases 0–9). This directory is deck's configuration.

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
| Swap     | zram, ram/2, zstd; swappiness 180, page-cluster 0 ([sysctl.d/70-zram.conf](etc/sysctl.d/70-zram.conf)) |
| Boot     | systemd-boot + UKI (installkernel → dracut), Plymouth splash and passphrase prompt |
| Kernel   | `sys-kernel/gentoo-kernel` from the binhost ([state/kernel](state/kernel)) |
| Packages | Official binhost, x86-64-v3; `-march=native` for source builds |
| Network  | NetworkManager with iwd |
| Desktop  | niri, quickshell (clock overlay + Mod+D omni panel), foot, mako, swaylock, greetd + tuigreet; GURU overlay enabled |
| Fonts    | Iosevka everywhere (Iosevka Term for monospace, Iosevka Etoile for the shell's serif, Iosevka Aile for GTK UI); Symbols Nerd Font for icons; Noto + Noto Color Emoji as fallback |
| Laptop   | power-profiles-daemon (thermal control is the firmware's: thermald refuses this model), fwupd, battery charge 75–80 % (Mod+D "Full charge" lifts it to 100 % until reboot) |

## Files

Paths mirror the `~/system` layout proposed in research overview §2.5.

| Here | Deployed to |
|------|-------------|
| `portage/` | `/etc/portage/` |
| `usr-local/` | `/usr/local/` |
| `etc/` | `/etc/` |
| `home/` | `~` (your user's home) |
| `state/world` | `/var/lib/portage/world` (observed, not deployed) |
| `recovery/` | the recovery system on `nvme0n1p3`, built and installed by its own scripts (not by deploy.sh); see [recovery/README.md](recovery/README.md) |

`etc/kernel/cmdline`, `etc/dracut.conf.d/10-luks.conf` and `etc/fstab` contain this machine's
LUKS and filesystem UUIDs. Replace them on any other machine.

Deploy with `./deploy.sh deck` (diff) and `./deploy.sh deck apply` from the repository root. Only
tracked files under `home/`, `etc/`, `portage/` and `usr-local/` are installed; nothing on the system is deleted.
Secrets (SSH keys, `gh` token, Wi-Fi passwords) stay on the machine; see [Secrets](../../README.md#secrets).

## Configuration notes

- **Keyboard.** Console and LUKS prompt use `KEYMAP=us`. niri uses `us,ch,hr`, US by
  default; Mod+Space / Mod+Shift+Space cycle layouts.
- **Display scale.** 1.25 (1536×960 logical), set in niri's `output "eDP-1"` block.
- **greetd user** is `greetd`, the Gentoo package's account.
- **One binhost.** The stage3 shipped `/etc/portage/binrepos.conf/gentoo.conf` (generic `x86-64`,
  same priority as `gentoobinhost.conf`), so Portage mixed generic and v3 packages. It was moved
  to `/etc/portage/binrepos-gentoo-generic.conf.disabled` on 2026-10-09; only
  [portage/binrepos.conf/gentoobinhost.conf](portage/binrepos.conf/gentoobinhost.conf) (v3) is left.
- **Fonts.** Added `media-fonts/symbols-nerd-font`, `noto` and `noto-emoji` for icons and fallback.
  fontconfig maps sans-serif, serif and monospace to Iosevka. `media-fonts/iosevka` has USE
  `iosevka-etoile` for the serif used by the clock, panel, mako and swaylock, and `iosevka-aile`
  (proportional) for GTK menus and dialogs and Zathura.
- **Extra keywords:** `gui-wm/niri`, `gui-apps/quickshell` and `app-misc/brightnessctl` (GURU) need
  `~amd64`. quickshell is built with `-crash-handler` (avoids keywording `dev-cpp/cpptrace`) and
  without the Hyprland, i3 and X11 modules.
  `app-editors/neovim` (and `dev-lua/luv`) are `~amd64` for 0.12, which nvim-treesitter requires.
- **Editor:** `EDITOR` and `VISUAL` are `/usr/bin/nvim` ([env.d/99editor](etc/env.d/99editor),
  from `eselect editor`/`eselect visual`); nano is not installed. Edit root's files with `sudoedit`,
  which runs nvim as you with your config.
- **Build tools:** `--with-bdeps=y` ([portage/make.conf](portage/make.conf)) keeps the build
  dependencies of binary packages installed and updated, so any package can be patched and rebuilt.
- **Python** is built with profile-guided and link-time optimisation
  ([package.use/python](portage/package.use/python), [env/lto.conf](portage/env/lto.conf)),
  upstream's release build: ~10 min per Python update. Measured 2026-10-13 against the previous
  build (from the pre-emerge snapshot, alternating runs, on battery): plain Python code 36% faster,
  `emerge -pvuDN @world` 9% (18.0 to 16.4 s).
- **Sustained load is limited by cooling**, not settings: at full fan the chassis sheds about 26 W
  at 96–100 °C, below the firmware's 30 W limit in performance. A higher limit, the `scx_lavd`
  scheduler and always-on huge pages were measured and left nothing faster:
  [bench/2026-10-13-power-and-scheduler.md](bench/2026-10-13-power-and-scheduler.md).
- **Extra packages** in world: `www-client/firefox-bin`, `sys-fs/udisks`,
  `sys-boot/efibootmgr`, `dev-util/github-cli`; `app-crypt/pinentry -qt6`. For looks:
  `sys-boot/plymouth` (`-gtk -pango`; 22.02, since 24.004 is package-masked as breaking some
  systems) with the overlay's `x11-themes/plymouth-theme-academia`, `x11-themes/papirus-icon-theme`,
  `x11-themes/bibata-xcursors` (GURU, `~amd64`), `app-text/zathura` with `zathura-pdf-mupdf`.
- **Bambu Studio** (`media-gfx/bambustudio-bin`, GURU, `~amd64`) for the Bambu Lab X1C: Bambu's
  AppImage unpacked to `/opt`. On first login it downloads Bambu's proprietary network plugin into
  `~/.config/BambuStudio`. Its bundled wxWidgets has no EGL, so the GL canvas needs X11: it runs
  through `gui-apps/xwayland-satellite` (GURU, `~amd64`; niri starts it on demand) with
  `GDK_BACKEND=x11`, set by [a desktop entry override](home/.local/share/applications/bambu-studio.desktop).
  Without that it exits with status 255 right after startup.

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

- **Shell.** Quickshell ([home/.config/quickshell](home/.config/quickshell/))
  runs as a systemd user service that restarts on failure
  ([quickshell.service](home/.config/systemd/user/quickshell.service); enable once with
  `systemctl --user enable --now quickshell.service`). It draws:
  - a click-through clock in the top-right on the top layer, so fullscreen windows cover it. It
    shows the keyboard layout when it isn't US, a coffee cup while keep-awake is on, and the battery
    level in oxblood below 15%. Notifications go out at 15% and 5%.
  - an OSD near the bottom for volume, brightness and layout changes. The brightness keys go through
    `qs ipc call brightness up|down` so it sees them, falling back to `brightnessctl`. On resume,
    swayidle's `after-resume` runs `qs ipc call brightness auto`, which sets the brightness for the
    time of day (`schedule` in Brightness.qml: 45% from 7, 70% from 9, 45% from 18, 25% from 21).
    There is no ambient light sensor.
  - the Mod+D omni panel: clock, battery, Wi-Fi, Bluetooth, power profile and keep-awake toggles,
    Ethernet with its link speed (only while connected), volume and brightness, now playing. The › on the Wi-Fi and Bluetooth tiles opens a network
    picker (with a password field for new networks) and a device list (search, pair, connect).
    Typing searches open windows (from `niri msg -j event-stream`), apps (ranked by how often they
    were launched here), actions, and evaluates arithmetic. Apps start in their own systemd scope
    (`app-quickshell-<id>-<n>.scope` under app.slice, `Session.launch`), not in quickshell.service:
    otherwise their memory counted as the shell's (Firefox made it look like 4 GB) and restarting
    the shell killed them. The panel blurs what's behind it
    (ext-background-effect, supported by niri 26.04).
- **GTK.** `gtk-3.0` and `gtk-4.0` prefer dark and carry `gtk.css` overrides; libadwaita takes the
  palette fully, GTK3 Adwaita only on its main surfaces. Icons are Papirus-Academia: Papirus-Dark with
  pale brown folders, built in `~/.local/share/icons` by
  [papirus-academia.sh](home/.local/share/icons/papirus-academia.sh) (symlinks only, the system theme
  is untouched; rerun after Papirus updates). The cursor is Bibata Modern Amber (niri's `cursor`
  block and settings.ini). Also set once, outside the repo:
  `gsettings set org.gnome.desktop.interface color-scheme prefer-dark` (and `font-name 'Iosevka Aile 11'`,
  `document-font-name 'Iosevka Etoile 11'`, `monospace-font-name 'Iosevka Term 11'`,
  `icon-theme 'Papirus-Academia'`, `cursor-theme 'Bibata-Modern-Amber'`), which the portal passes to
  Firefox and libadwaita.
- **Portals.** [xdg-desktop-portal/niri-portals.conf](home/.config/xdg-desktop-portal/niri-portals.conf)
  takes the file chooser from the gtk backend: the gnome one hands it to Nautilus, which isn't
  installed, so file dialogs (Firefox's Import, Upload, Save as) silently never opened.
- **niri.** Brass focus ring, shadows, 10 px rounded corners, `prefer-no-csd`. New windows open
  maximized (`open-maximized`, as Mod+F); floating ones are unaffected. Unfocused windows are at 93 %
  opacity. Windows open like ink soaking into paper (a custom shader: a ragged, brass-tinted front
  spreads from the centre) and close the same way in reverse; movement and resize springs are a
  little softer than the defaults.
- **Wallpaper.** Walnut leather with a vignette, a faint brass astrolabe and a double-rule frame,
  generated by [academia.py](home/.local/share/wallpapers/academia.py) (Pillow only, fixed seeds;
  `python3 academia.py academia.png 1920x1200`, and `... academia-night.png 1920x1200 night` for the
  candlelit variant: dimmer, with a warm glow from the lower left). Quickshell draws it
  ([Wallpaper.qml](home/.config/quickshell/Wallpaper.qml)) and fades to the night variant as the sun
  goes from 3° above the horizon to 6° below, at the coordinates wlsunset uses. A niri layer rule
  places it in the overview backdrop, and workspaces are transparent so it shows in the gaps too.
- **Console.** `vt.default_red/grn/blu` on the kernel command line ([etc/kernel/cmdline](etc/kernel/cmdline)
  and dracut's `kernel_cmdline`) give every VT the foot palette from boot on: the LUKS prompt, the
  text consoles and tuigreet, which uses ochre ("yellow") for borders and prompts.
- **Boot splash.** Plymouth (`splash` on the command line, dracut's `plymouth` module) with the
  overlay's [academia theme](overlay/x11-themes/plymouth-theme-academia/files/academia/): the
  wallpaper's astrolabe on espresso, its rete turning about once a minute, and "deck" with a dot per
  typed character for the LUKS passphrase. The images are pre-rendered by `make-images.py` there.
  [etc/plymouth/plymouthd.conf](etc/plymouth/plymouthd.conf) selects it; after changing either,
  rebuild the UKI (`emerge --config sys-kernel/gentoo-kernel`). If the prompt ever fails to show,
  press e in systemd-boot and add `plymouth.enable=0` (Secure Boot is off, so the UKI accepts it).
  Between the unlock and tuigreet the firmware's Lenovo logo shows for a moment: fbcon takes the
  console over lazily, so the framebuffer falls back to what the firmware drew. Accepted; it goes
  with the firmware logo below. Don't use `plymouth quit --retain-splash` (a plymouth-quit drop-in)
  against it: that leaves the console in graphics mode, tuigreet never becomes visible, and the
  machine looks hung after the unlock (2026-10-11, repaired from a live USB).
  For anything Plymouth breaks after the unlock, the escape is `plymouth.enable=0` at the
  systemd-boot menu (e). The "-noplymouth" UKI only avoids Plymouth in the initramfs; the root
  filesystem still starts it.
- **Prompt.** Pure bash in [.bashrc](home/.bashrc), two lines. The first has the directory in linen,
  the git branch in sage (with dirty, staged, untracked and stash marks, rebase/merge state, and
  ↑/↓ against upstream), and, only when they apply, the duration of a command over 5 s, the exit
  status and signal, background jobs, the venv and user@host over SSH. The second is a brass ❯,
  oxblood after a failure. On the text console it drops the glyphs and the escape codes. For foot it
  emits OSC 7 (Ctrl+Shift+N opens in the same directory) and OSC 133 (Ctrl+Shift+Z/X jump between
  prompts, Ctrl+Shift+G copies the last command's output, bound in [foot.ini](home/.config/foot/foot.ini)),
  and a command that ran 30 s or more raises a notification when the window isn't focused (except
  editors, pagers, ssh and the like).
- **PDFs.** Zathura ([zathurarc](home/.config/zathura/zathurarc)) recolours pages to parchment on
  walnut, keeping figure hues; Ctrl+R shows the original colours. It is the default for
  `application/pdf`.

## Stability

On 2026-10-09 deck reset by itself with nothing in the journal, and the next boot came up without
Wi-Fi. What is set because of it:

- **Wi-Fi.** iwd deletes and re-creates the kernel's `wlan0` at startup; once that raced
  iwlwifi's firmware reload (`NEW_INTERFACE failed`) and NetworkManager kept `wlan0` unavailable
  until the next reboot. [etc/iwd/main.conf](etc/iwd/main.conf) sets `DefaultInterface=*` so iwd
  keeps the kernel's interface. Consequence: `systemctl restart iwd` leaves no Wi-Fi, because the
  stopping iwd deletes `wlan0` and the new one will not create it. Reload the driver instead
  (`modprobe -r iwlmvm iwlwifi && modprobe iwlwifi`) or reboot.
  iwd also looks for that interface only once, at startup, so
  [etc/systemd/system/iwd.service.d/wait-wlan0.conf](etc/systemd/system/iwd.service.d/wait-wlan0.conf)
  starts it after udev has finished with `wlan0` (it once looked 0.3 s early and found nothing).
- **Crash capture.** `efi_pstore.pstore_disable=0` on the kernel command line (the dist kernel
  disables it by default), so a panic leaves the kernel log in EFI variables and `systemd-pstore`
  moves it to `/var/lib/systemd/pstore/` on the next boot.
  [etc/sysctl.d/90-crash.conf](etc/sysctl.d/90-crash.conf) turns soft and hard lockups into panics
  and reboots 10 s after one. journald syncs every 15 s instead of every 5 min
  ([etc/systemd/journald.conf.d/sync.conf](etc/systemd/journald.conf.d/sync.conf)).

- **Vitals.** [deck-vitals.service](etc/systemd/system/deck-vitals.service) runs
  [vitals.py](etc/deck-vitals/vitals.py) (one Python process, no subprocesses) and logs a line of
  hardware state every 2 s on AC, 10 s on battery: temperatures, fan, RAPL power (whole platform,
  package, cores, uncore), power limits, CPU and GPU frequencies, load, memory, pressure, throttle
  and PCIe error counts, battery and platform profile. It goes to its own journal namespace,
  synced every 2 s and kept four days ([journald@vitals.conf](etc/systemd/journald@vitals.conf)).
  About 4 ms of CPU per 10 s; the EC sensors and battery (about 1 ms each, through ACPI) are read
  every 10 s, the SSD temperature (which wakes the drive) every 30 s.
  Both unexplained resets (2026-10-09 during light work, 2026-10-11 under a full-core build) left
  nothing: no kernel message, empty pstore, no firmware error record. So the kernel never saw them
  coming, which points below it (power, firmware, a PCIe device, RAM).

After an unexplained reset: `ls /var/lib/systemd/pstore/`, `journalctl -b -1 -k -p warning`, and
`journalctl --namespace=vitals -b -1 -n 50` for the last 100 s of temperatures and power. An
empty pstore after a reset with no panic screen points at firmware or hardware (power, thermal,
EC), not the kernel.

## Security

Hardening (2026-10-09):

- **One network manager.** `systemd-networkd` and its sockets and `-wait-online` are disabled
  (`systemctl disable`, not a file here); NetworkManager with iwd manages every interface.
- **Signed binary packages only.** `binpkg-request-signature` in `FEATURES`
  ([portage/make.conf](portage/make.conf)): Portage refuses binhost packages without a valid
  Gentoo signature (keyring in `/etc/portage/gnupg`, from `getuto`).
- **No LLMNR or mDNS.** [etc/systemd/resolved.conf.d/no-multicast.conf](etc/systemd/resolved.conf.d/no-multicast.conf);
  resolved no longer listens on 5353/5355.
- **Firewall.** [etc/nftables/rules/main.nft](etc/nftables/rules/main.nft), loaded by
  `nftables.service` (enabled): inbound is dropped unless it is a reply, loopback, ICMP or a
  DHCPv6 reply; forwarding is dropped. A server started here (a dev server on `0.0.0.0`) is not
  reachable from the network; allow its port in `input` to change that.
- **sysctl.** [etc/sysctl.d/80-hardening.conf](etc/sysctl.d/80-hardening.conf): no ICMP redirects
  in or out, `kptr_restrict=1`, `kexec_load_disabled=1`, `ldisc_autoload=0`, `suid_dumpable=0`.
  `rp_filter` is already 2 on every interface (Gentoo's `50-default.conf`).
- **Kept as is, on purpose.** `ptrace_scope=1` (gdb and `strace -p` on own processes), user
  namespaces (Firefox's sandbox, systemd), io_uring.
- **Not done yet: firmware logo.** The firmware still shows the Lenovo logo at power-on. The
  replacement is ready: `LOGO.GIF` (440×440, 50 KB, the astrolabe on black) from the Plymouth
  theme's `make-images.py`. It can only be written during a BIOS flash, and 1.24 (N3PET33W) is
  current and can't be reflashed over itself. So with the next release (1.25), update from Lenovo's
  Bootable CD ISO instead of fwupd: `geteltorito -o bios.img <iso>`, write it to a USB stick, copy
  `LOGO.GIF` into its `FLASH` folder (check the limits in `FLASH/readme.txt`; 768×480 and 60 KB
  on the Gen 3), boot it on AC power and answer yes to the custom logo. It survives later
  updates; answering no on a later flash restores Lenovo's.
- **Shelved.** Secure Boot is disabled. Signing the UKIs with own keys (`sbctl`) was considered
  and set aside (2026-10-12): a missed signature or a BIOS update resetting the keys would leave
  every entry, the recovery included, unbootable.

## Maintenance

Set up on 2026-10-11. Anything that needs attention shows as a tile at the top of the Mod+D
dashboard ([quickshell/Maintenance.qml](home/.config/quickshell/Maintenance.qml)): security
advisories, firmware updates, failed system or user units, a recovery system over 6 weeks old, an
unclean shutdown, and what [deck-attention](usr-local/bin/deck-attention) finds (below). Clicking
it opens a terminal with the details and the failed units' logs.

- **Snapshots.** btrbk snapshots `@` and `@home` hourly into `@snapshots`, visible as
  `/.snapshots/@.<date>` and `/.snapshots/@home.<date>` ([etc/btrbk/btrbk.conf](etc/btrbk/btrbk.conf),
  [hourly timer](etc/systemd/system/btrbk.timer.d/hourly.conf)). Kept: everything for 2 days, then
  48 hourly, 14 daily, 6 weekly and 3 monthly. `@var_log` and `@var_tmp` are not snapshotted.
  btrbk works on the btrfs top level, automounted at `/mnt/btr_pool` ([etc/fstab](etc/fstab)).
  Snapshots are on the same disk, so they are not a backup. Rebuildable folders stay out of them:
  cargo's `target/` in each repo and `~/.cache` are nested subvolumes, which snapshots skip
  ([usr-local/bin/deck-nosnap](usr-local/bin/deck-nosnap) converts a folder in place, no root needed;
  the tile lists any that aren't).
- **Updates and rollback.** `sudo emerge` ([usr-local/sbin/emerge](usr-local/sbin/emerge)) first
  takes a read-only snapshot of `@`, `/.snapshots/@.pre-emerge.<time>` (the last 5 kept; btrbk
  ignores them), unless the command changes nothing (`-p`, `--search`, `--sync`, ...). To go back,
  boot the recovery system and run `deck-rollback`: it lists the snapshots of `@`, renames `@` to
  `@.broken.<time>` and makes the chosen snapshot the new `@`, then checks that the kernels on the
  ESP still have their modules in it.
- **Previous kernel kept.** Each kernel that boots far enough for the health check is recorded as
  good, and the last two good ones are in the Portage set `@deck-kernels`
  (`/etc/portage/sets/deck-kernels`, written by [check.py](etc/deck-health/check.py), listed in
  `/var/lib/portage/world_sets`), so `emerge --depclean` leaves the previous kernel and its boot
  menu entry. If a new kernel won't boot: Space at power-on, pick the older `gentoo-*` entry.
- **Sync and security advisories.** [gentoo-sync.timer](etc/systemd/system/gentoo-sync.timer)
  runs `emerge --sync` daily, then writes `glsa-check -l affected` to
  `/var/lib/gentoo-security/affected`.
- **Keeping up with Gentoo.** After the sync and after every `sudo emerge` that changes something,
  [deck-portage.service](etc/systemd/system/deck-portage.service) ([portage.py](etc/deck-health/portage.py))
  records when `emerge --update @world` last finished, preserved libraries and whether
  `emerge -p --depclean` is blocked, in `/var/lib/deck-health/portage.json`. The panel says so
  when @world is more than 14 days behind, a `@preserved-rebuild` is due, depclean is blocked or
  Gentoo news is unread.
- **Firmware.** A user timer refreshes fwupd's metadata daily
  ([systemd/user/fwupd-refresh.timer](home/.config/systemd/user/fwupd-refresh.timer), enabled with
  `systemctl --user enable`); the panel asks `fwupdmgr get-updates` at most hourly. The system
  `fwupd-refresh.timer` stays disabled: its DynamicUser sandbox gets "Failed to connect to daemon:
  The connection is closed" from the system bus.
- **Scrub.** [btrfs-scrub.timer](etc/systemd/system/btrfs-scrub.timer) verifies every checksum on
  `/` monthly; uncorrectable errors leave the unit failed.
- **Disk health** ([deck-health.timer](etc/systemd/system/deck-health.timer), 2 min after boot and
  every 6 h; [check.py](etc/deck-health/check.py)): btrfs's on-disk error counters, disk errors in
  the kernel log, and the SSD's health log (`sys-apps/nvme-cli`): critical warning, spare, wear,
  media errors. The SSD also counts shutdowns it wasn't warned of; when that goes up, the previous
  boot ended in a crash, reset or power loss: the panel says so for a week (until seen) and a
  scrub starts. btrfs needs no repair pass after a crash (it mounts at its last commit); the offline
  `btrfs check --readonly` is `deck-mount check` in the recovery system. Report:
  `/var/lib/deck-health/report`.
- **ESP copy.** The EFI system partition is FAT (no checksums) and holds every kernel and the
  recovery's UKI. The health check mirrors its files to `/var/lib/deck-health/esp` (root only),
  inside `@`, so the hourly snapshots keep its history, with its identity in `esp.blkid`. To
  restore it: [runbook.md](runbook.md#the-esp-is-damaged).
- **Needs a look** ([usr-local/bin/deck-attention](usr-local/bin/deck-attention), run by the panel
  when it opens; `deck-attention --details` explains):
  - work only on deck: a repository in `~/src` with uncommitted changes or commits on no remote,
    for more than a day;
  - config drift: files that differ between deck and this repo (`./deploy.sh deck`);
  - programs that dumped core in the last 3 days (`coredumpctl`; dumps capped at 2 GiB by
    [coredump.conf.d/deck.conf](etc/systemd/coredump.conf.d/deck.conf)), and an unclean shutdown
    in the last week. These are events: opening the details marks them as seen and the tile drops
    them until a newer one (`~/.local/state/deck-attention/seen`);
  - the Portage lines above;
  - `/` or `/efi` over 85% full.
- **Recovery system.** [deck-recovery-update.timer](etc/systemd/system/deck-recovery-update.timer)
  updates the recovery on p3 about monthly, on AC; see
  [recovery/README.md](recovery/README.md#keeping-it-up-to-date).
- **Empty battery.** UPower's critical action (2%) is `HybridSleep`, which deck can't do (no
  hibernation), so UPower falls back to a clean power-off (`GetCriticalAction` says `PowerOff`).
- **Power profile** follows the plug ([quickshell/Battery.qml](home/.config/quickshell/Battery.qml)):
  Performance on AC, Balanced on battery. A choice made in the panel holds until the next plug change.
- **Builds** run at `PORTAGE_NICENESS=10` with idle I/O ([portage/make.conf](portage/make.conf)).
- **Video** decodes on the GPU: Firefox's media process loads Intel's VA-API driver
  (`iHD_drv_video.so`, `media-libs/libva-intel-media-driver`) by itself; no prefs needed. Check:
  `grep -l iHD_drv_video /proc/$(pgrep -f 'RDD Process')/maps`.
- **Memory guard** (2026-10-12), so a runaway program fails on its own instead of freezing the
  machine into a hard reset:
  - systemd-oomd ([oomd.conf.d/deck.conf](etc/systemd/oomd.conf.d/deck.conf)) kills the cgroup
    reclaiming the most when the desktop session is stalled on memory over 50% of the time for
    20 s ([user@.service.d](etc/systemd/system/user@.service.d/oomd.conf)), or the one using the
    most swap when RAM and zram are both over 90% full ([-.slice.d](etc/systemd/system/-.slice.d/oomd.conf)).
    niri, quickshell, PipeWire and WirePlumber ask to be spared
    (`home/.config/systemd/user/*.service.d/oomd.conf`). Kills: `journalctl -u systemd-oomd`.
  - That only works if work is in its own cgroup. niri's plain `spawn "foot"` left everything run in
    a terminal inside niri.service (niri moves foot into a scope only after its shell has forked),
    so Mod+T starts foot through `systemd-run --scope`; quickshell's launcher already does.
  - `sudo emerge` is [usr-local/sbin/emerge](usr-local/sbin/emerge), which runs `/usr/bin/emerge`
    in a scope in [build.slice](etc/systemd/system/build.slice): reclaimed hard above 18 GiB,
    killed (the build, nothing else) above 24 GiB, at most 4 GiB in zram. The recovery update runs
    there too. `/usr/bin/emerge` bypasses it.
  - Rootless podman containers share a 10 GiB high / 14 GiB max cap
    ([systemd/user/user.slice.d](home/.config/systemd/user/user.slice.d/limits.conf)).
- **Night light.** wlsunset (GURU, `~amd64`), started by niri, for the Zurich area (coordinates
  rounded to 0.1°).
- **Clipboard history.** `wl-paste --watch cliphist store` (cliphist from GURU, `~amd64`), started by
  niri. Mod+Alt+V opens it in the omni panel; typing filters, Enter copies. "Clear clipboard
  history" wipes it. cliphist skips entries password managers mark as sensitive.

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
| LaTeX | TeX Live collections for omnidiff's paper (acmart): `texlive-publishers`, `-latexextra`, `-pictures`, `-fontsextra`; latexmk. Not the `app-text/texlive` meta-package. `ibm-plex` fonts for the Typst slides | — |

Skipped: Android SDK/JDK (eInk-markdown-editor, possibly dead), zig (only omnidiff's and
friends-and-family-ca's release workflows use it), Nix.

`~/.cargo/config.toml` links with mold but leaves out `target-cpu=native`:
these repos build binaries that ship. It runs rustc through sccache (`dev-util/sccache`, cache in
`~/.cache/sccache`), so dependencies compiled once are reused by clean builds, worktrees and other
repos; `sccache --show-stats` shows hits.

Go is `dev-lang/go` from Portage, in world. `GOTOOLCHAIN=local`
([home/.config/go/env](home/.config/go/env)) stops `go` from downloading newer toolchains that a
`go.mod` asks for. `GOPATH` stays `~/go`; `~/go/bin` is on `PATH` (in `~/.bashrc`, not tracked).

Source checkouts: `~/src/<repo>` holds only my own repositories; anything authored by others is
cloned to `~/3p/<host>/<owner>/<repo>` (e.g. `~/3p/github.com/gitleaks/gitleaks`, used to
generate the gitleaks ebuild below).

## Local overlay

[overlay/](overlay/) is the source of the `local` repository. It is deployed root-owned with
`sudo rsync -a --delete --chown=root:root overlay/ /var/db/repos/local/`
([portage/repos.conf/local.conf](portage/repos.conf/local.conf)); Portage never reads this checkout.

| Package | Ebuild | Notes |
|---|---|---|
| `dev-util/omnidiff` | `9999` (installed, in world) | Live: clones `main`, crates from Cargo.lock at unpack; `**` keyword. Rebuild with `emerge @live-rebuild` |
| | `0.2.0` | Upstream's release ebuild from omnidiff's `packaging/gentoo`, unchanged (still says `RUST_MIN_VER=1.88.0`; Cargo.toml needs 1.92) |
| `dev-util/gitleaks` | `8.30.1` (in world) | Secret scanner; `.githooks/scan-secrets` runs it when installed. Go modules via `EGO_SUM` from upstream's `go.sum` (deprecated in ::gentoo, but needs no hosted dependency tarball; QA notice at unpack). `~amd64` keyword. [portage/mirrors](portage/mirrors) pins Go downloads to proxy.golang.org: goproxy.io served HTML pages for 76 of the 426 modules, which went into the first Manifest. Regenerating the Manifest: check every distfile against upstream's `go.sum` h1 hashes first |

Built with Portage's Rust (rust-bin), not rustup. Installs the binary, man page and
bash/zsh/fish completions. git uses it as `diff.tool` and `diff.external`, as on core
([home/.gitconfig](home/.gitconfig)).

podman's `containers-common` needs `net-firewall/iptables[nftables]`
([portage/package.use/podman](portage/package.use/podman)).

## SSH and git (not stored here)

Keys and SSH config stay on the machine; this repo is public. Layout, for reference:

- One ed25519 key per destination, generated on deck (`<your username>@deck`): `id_core_ed25519`,
  `id_hetzner_ed25519`, `id_scaleway_ed25519`, `id_github_ed25519`.
- The MikroTik key is shared with core (`id_ed25519_mikrotik`).
- Host keys were taken from core's `known_hosts`, not accepted on first use.
- `gh` authenticated with `--git-protocol ssh --skip-ssh-key`.
- Global git config: [home/.gitconfig](home/.gitconfig). omnidiff (codediff's new name) replaces
  core's codediff difftool/external settings; core's `[delta]` block is left out (no pager uses it).
