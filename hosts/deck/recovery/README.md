# Deck recovery

A small console Gentoo on its own encrypted partition (`nvme0n1p3`, 32 GiB, LUKS2 with the same
passphrase as the main system, ext4 inside), for when the main system won't boot or boots broken.
It has its own kernel and initramfs, so nothing in the main system's root, kernel or dracut setup
can take it down with it. It doesn't survive a dead SSD: that's what GitHub and the live USB are for.

## Using it

In the systemd-boot menu pick **Deck recovery** (the main system stays the default), type the
passphrase, and you're in a root shell on tty1.

| Command | What it does |
|---|---|
| `iwctl station wlan0 connect <SSID>` | Wi-Fi; the main system's saved networks were copied at install and connect by themselves |
| `deck-mount` | opens the main LUKS partition **read-only** and mounts `@`, `@home`, `@var_log`, `@var_tmp`, `@snapshots` and the ESP under `/mnt/deck` |
| `deck-mount rw` | the same, writable (after `deck-mount off` if it was open read-only) |
| `deck-chroot` | a shell inside the main system (systemd-nspawn), for `emerge`, `dracut`, `systemctl` |
| `deck-mount off` | unmount and close |
| `install-claude` | installs Claude Code (it isn't kept here: it would go stale and needs a login anyway) |

Where to look first:

    journalctl --root=/mnt/deck --list-boots
    journalctl --root=/mnt/deck -b -1 -p warning
    journalctl --root=/mnt/deck --namespace=vitals -b -1 -n 50   # last 100 s of temperatures and power
    ls /mnt/deck/var/lib/systemd/pstore                          # kernel log of a panic, if any
    btrfs subvolume list /mnt/deck/.snapshots                    # hourly btrbk snapshots

Backups of the main partition's LUKS header are in `/root/luks-header-deck-*.img`; the main
system keeps one of this partition's in its own `/root`.

## What's in it

The amd64 systemd stage3 plus `gentoo-kernel-bin`, `linux-firmware`, `intel-microcode`, cryptsetup,
btrfs-progs, dosfstools, gptfdisk, parted, nvme-cli, smartmontools, efibootmgr, pciutils, usbutils,
iwd (doing its own DHCP), git, tmux, vim, lsof, strace, htop. No desktop, no sshd. root has no
password and is logged in automatically on tty1: the LUKS passphrase is the gate.

The initramfs is generic, built inside systemd-nspawn, with only dm-crypt and the keyboard drivers
(`etc/dracut.conf.d/10-recovery.conf`); the console stays on the firmware's framebuffer.

## Files here

- [build.sh](build.sh): builds the root on the main system (default `/var/tmp/deck-recovery/root`):
  the latest stage3, checked against Gentoo's release key, then [root/](root/) on top, then update.sh.
- [update.sh](update.sh) `<root> [esp]`: updates its packages with systemd-nspawn (binhost, the main
  system's Gentoo repository bound read-only) and, given the ESP, builds its UKI as
  `EFI/Linux/deck-recovery.efi`. The main system's `gentoo-*.efi` are never touched.
- [install.sh](install.sh): shrinks the main partition by 32 GiB online (btrfs, then the dm-crypt
  mapping, then the partition, each checked) and puts the built root on the new p3.
- [root/](root/): the recovery's own files: portage config, dracut, fstab, os-release, iwd,
  autologin, and the `deck-mount`, `deck-chroot` and `install-claude` helpers.
