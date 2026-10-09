# Runbook summary — ThinkPad T14s Gen 4

Condensed from the doc "Gentoo install runbook — ThinkPad T14s Gen 4"
as written before the install. What deck actually runs, and where it differs, is in [README.md](README.md).

## Decisions

| Area     | Choice |
|----------|--------|
| Init     | systemd |
| Disk     | GPT: 1 GiB EFI (`/efi`) + LUKS2 (argon2id) → btrfs, zstd:1 |
| Subvols  | `@` `/`, `@home`, `@snapshots` `/.snapshots`, `@var_log`, `@var_tmp` |
| Swap     | zram (ram/2, zstd), no hibernation |
| Boot     | systemd-boot + UKI built by dracut via installkernel |
| Kernel   | `sys-kernel/gentoo-kernel` (dist-kernel), from binhost |
| Packages | Official binhost x86-64-v3 + source; profile `default/linux/amd64/23.0/desktop/systemd` |
| Desktop  | niri (Wayland), waybar, foot, fuzzel, mako, greetd + tuigreet |

## Phases

- **0 Hardware checks** — battery health, BIOS (Secure Boot off, AMT off, VT-x/VT-d on), webcam, suspend, FTDI, Wi-Fi.
- **1 Live USB** — LiveGUI ISO, nmcli Wi-Fi, clock, sshd for remote install.
- **2 Disk** — sgdisk, `cryptsetup luksFormat --type luks2 --pbkdf argon2id`, `open --allow-discards --persistent`, btrfs + 5 subvols, mount with `noatime,compress=zstd:1`.
- **3 Stage 3** — desktop-systemd stage3, gpg verify, extract with xattrs, `arch-chroot`.
- **4 Portage** — `-march=native -O2`, `MAKEOPTS="-j20 -l18"`, `FEATURES="getbinpkg"`, binhost x86-64-v3 + `getuto`, cpuid2cpuflags, firmware licences, locales (en_US, de_CH, hr_HR), `emerge -avuDN @world`.
- **5 Kernel/boot** — linux-firmware, sof-firmware, intel-microcode; `/etc/dracut.conf.d/10-luks.conf` with `kernel_cmdline="rd.luks.name=<LUKS-UUID>=root root=/dev/mapper/root rootflags=subvol=@ rw quiet"`; `/etc/kernel/cmdline`; `bootctl install`; `emerge --config sys-kernel/gentoo-kernel` rebuilds the UKI.
- **6 System config** — fstab with btrfs/EFI UUIDs, Europe/Zurich, hostname t14s, `KEYMAP=sg` (console + LUKS prompt), NetworkManager + iwd, zram-generator, sudo for wheel, user in wheel/video/audio/input/dialout/plugdev.
- **7 First boot** — verify cmdline, btrfs mounts, zram, discards, `journalctl -b -p err`, `bootctl status`.
- **8 Laptop + desktop** — power-profiles-daemon, thermald, fwupd, bluez, pipewire/wireplumber, brightnessctl, intel media driver; battery thresholds 75/80 via `/etc/tmpfiles.d/battery.conf`; GURU overlay; niri + companions; greetd/tuigreet.
- **9 Dev tools** — rustup, mold, cmake, ninja, pkgconf, tio; `~/.cargo/config.toml` uses mold + target-cpu=native; cargo-nextest; OmniDiff build benchmark vs domaci.

## Recovery (from live USB)

```
cryptsetup open /dev/nvme0n1p2 root
OPTS=noatime,compress=zstd:1
mount -o $OPTS,subvol=@        /dev/mapper/root /mnt/gentoo
mount -o $OPTS,subvol=@home    /dev/mapper/root /mnt/gentoo/home
mount -o $OPTS,subvol=@var_log /dev/mapper/root /mnt/gentoo/var/log
mount -o $OPTS,subvol=@var_tmp /dev/mapper/root /mnt/gentoo/var/tmp
mount /dev/nvme0n1p1 /mnt/gentoo/efi
arch-chroot /mnt/gentoo
# fix, then: emerge --config sys-kernel/gentoo-kernel
```

Common failures: no boot entry → UKI not written; dracut "no root device" → wrong LUKS UUID in cmdline;
passphrase rejected → keymap mismatch; missing /home → fstab typo; black screen → missing firmware in UKI.
