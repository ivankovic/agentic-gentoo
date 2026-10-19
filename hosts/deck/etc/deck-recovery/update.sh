#!/usr/bin/env bash
# update.sh <root> [esp]: bring the recovery root up to date from the main system, with
# systemd-nspawn: its packages from the binhost (the main system's Gentoo repository, bound
# read-only, so both are on the same snapshot) and, given the ESP's mount point, its UKI at
# <esp>/EFI/Linux/deck-recovery.efi. The main system's own UKIs are never touched.
#
# Our files go on top first, every time, so the recovery follows the repository at its next update:
# recovery/root/ and its README (deploy.sh installs them in this directory) and the deck router
# (/usr/local/bin/deck). build.sh points RECOVERY_FILES, RECOVERY_DOC and RECOVERY_ROUTER at the
# repository instead. What was removed from the repository stays on the recovery, as with deploy.sh.
set -euo pipefail
[[ $EUID == 0 ]] || { echo "update.sh: run as root" >&2; exit 1; }
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=${1:?usage: update.sh <root> [esp]}
ESP=${2:-}
files=${RECOVERY_FILES:-$here/root}
doc=${RECOVERY_DOC:-$here/RECOVERY.md}
router=${RECOVERY_ROUTER:-/usr/local/bin/deck}

# The stage3's os-release is a symlink into /usr/lib: replace it, don't write through it.
[[ -L $ROOT/etc/os-release ]] && rm "$ROOT/etc/os-release"
# Copied as root, so new files are root's; --preserve keeps the executable bits.
cp -rT --preserve=mode,timestamps "$files" "$ROOT"
install -D -m 644 "$doc" "$ROOT/usr/local/share/RECOVERY.md"
install -m 755 "$router" "$ROOT/usr/local/bin/deck"

packages=(
  sys-kernel/gentoo-kernel-bin sys-kernel/dracut sys-kernel/linux-firmware sys-firmware/intel-microcode
  sys-fs/cryptsetup sys-fs/btrfs-progs sys-fs/dosfstools sys-apps/gptfdisk sys-block/parted
  sys-apps/nvme-cli sys-apps/smartmontools sys-boot/efibootmgr sys-apps/pciutils sys-apps/usbutils
  net-wireless/iwd dev-vcs/git app-misc/tmux app-editors/vim sys-process/lsof dev-debug/strace
  sys-process/htop
)

nspawn=(systemd-nspawn --quiet --directory="$ROOT" --resolv-conf=copy-host --slice=build.slice
  --bind-ro=/var/db/repos/gentoo --bind=/var/cache/distfiles)

"${nspawn[@]}" emerge --quiet-build --update --deep --newuse --noreplace "${packages[@]}" @world
"${nspawn[@]}" emerge --quiet --depclean
systemctl --root="$ROOT" enable iwd.service systemd-resolved.service systemd-timesyncd.service >/dev/null
ln -sf ../usr/lib/systemd/resolv.conf "$ROOT/etc/resolv.conf"
date -u +%FT%TZ > "$ROOT/etc/deck-recovery-updated"

if [[ -n $ESP ]]; then
  [[ -s $ROOT/etc/dracut.conf.d/90-cmdline.conf ]] ||
    { echo "update.sh: no 90-cmdline.conf yet (install.sh writes it); not building the UKI" >&2; exit 1; }
  kver=$(ls "$ROOT/lib/modules" | sort -V | tail -1)
  "${nspawn[@]}" --bind="$ESP:/efi" dracut --force --uefi --kver "$kver" /efi/EFI/Linux/deck-recovery.efi
  echo "update.sh: $ESP/EFI/Linux/deck-recovery.efi for $kver"
fi
