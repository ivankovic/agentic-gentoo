#!/usr/bin/env bash
# update.sh <root> [esp]: bring the recovery root up to date from the main system, with
# systemd-nspawn: its packages from the binhost (the main system's Gentoo repository, bound
# read-only, so both are on the same snapshot) and, given the ESP's mount point, its UKI at
# <esp>/EFI/Linux/<RECOVERY_LABEL>.efi. The main system's own UKIs are never touched.
#
# Our files go on top first, every time, so the recovery follows the repository at its next update:
# recovery/root/ and its README (deploy.sh installs them in this directory) and the deck router
# (/usr/local/bin/deck). build.sh points RECOVERY_FILES, RECOVERY_DOC and RECOVERY_ROUTER at the
# repository instead. What was removed from the repository stays on the recovery, as with deploy.sh.
# What differs from host to host comes from host.conf (RECOVERY_CONF), written into the root here:
# its name, hostname, fstab, network, SSH and the extra initramfs settings.
set -euo pipefail
[[ $EUID == 0 ]] || { echo "update.sh: run as root" >&2; exit 1; }
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=${1:?usage: update.sh <root> [esp]}
ESP=${2:-}
files=${RECOVERY_FILES:-$here/root}
doc=${RECOVERY_DOC:-$here/RECOVERY.md}
router=${RECOVERY_ROUTER:-/usr/local/bin/deck}
conf=${RECOVERY_CONF:-$here/host.conf}
[[ -s $conf ]] || { echo "update.sh: no $conf (deploy.sh installs it)" >&2; exit 1; }
# shellcheck source=host.conf
. "$conf"

# The stage3's os-release is a symlink into /usr/lib: replace it, don't write through it.
[[ -L $ROOT/etc/os-release ]] && rm "$ROOT/etc/os-release"
# Copied as root, so new files are root's; --preserve keeps the executable bits.
cp -rT --preserve=mode,timestamps "$files" "$ROOT"
install -D -m 644 "$doc" "$ROOT/usr/local/share/RECOVERY.md"
install -m 755 "$router" "$ROOT/usr/local/bin/deck"

# This host's: the config itself (for deck mount and the login help), identity, fstab.
install -m 644 "$conf" "$ROOT/etc/deck-recovery.conf"
echo "$RECOVERY_LABEL" > "$ROOT/etc/hostname"
# PRETTY_NAME is the systemd-boot menu title of the UKI. The rest is Gentoo's (systemd's build reads
# BUG_REPORT_URL from here for its boot stub's SBAT data).
cat > "$ROOT/etc/os-release" <<EOF
NAME="$RECOVERY_NAME"
PRETTY_NAME="$RECOVERY_NAME"
ID=gentoo
VERSION_ID=2.18
ANSI_COLOR="1;33"
HOME_URL="https://github.com/ivankovic/agentic-gentoo/tree/main/hosts/deck/recovery"
SUPPORT_URL="https://www.gentoo.org/support/"
BUG_REPORT_URL="https://bugs.gentoo.org/"
EOF
cat > "$ROOT/etc/fstab" <<EOF
# $RECOVERY_NAME. / is the ext4 inside its LUKS partition (opened by the initramfs as "recovery");
# the ESP is shared with the main system. The main system itself is mounted only on request
# (deck mount).
/dev/mapper/recovery	/	ext4	noatime,errors=remount-ro	0 1
UUID=$ESP_UUID		/efi	vfat	umask=0077,noauto,x-systemd.automount	0 2
EOF

packages=(
  sys-kernel/gentoo-kernel-bin sys-kernel/dracut sys-kernel/linux-firmware sys-firmware/intel-microcode
  sys-fs/cryptsetup sys-fs/btrfs-progs sys-fs/dosfstools sys-apps/gptfdisk sys-block/parted
  sys-apps/nvme-cli sys-apps/smartmontools sys-boot/efibootmgr sys-apps/pciutils sys-apps/usbutils
  dev-vcs/git app-misc/tmux app-editors/vim sys-process/lsof dev-debug/strace sys-process/htop
)
services=(systemd-resolved.service systemd-timesyncd.service)
if [[ $NETWORK == wifi ]]; then
  packages+=(net-wireless/iwd)
  services+=(iwd.service)
else
  mkdir -p "$ROOT/etc/systemd/network"
  cat > "$ROOT/etc/systemd/network/50-wired.network" <<'EOF'
# Written by update.sh (NETWORK=wired): DHCP on every wired interface.
[Match]
Name=en*

[Network]
DHCP=yes
EOF
  services+=(systemd-networkd.service)
fi

# The initramfs settings beyond 10-recovery.conf.
{
  echo "# Written by update.sh from host.conf."
  [[ -n $INITRD_DRIVERS ]] && echo "drivers+=\" $INITRD_DRIVERS \""
  if [[ $SSH == yes ]]; then
    echo 'add_dracutmodules+=" crypt-ssh "'
    echo 'dropbear_port="2222"'
    echo 'dropbear_acl="/etc/dropbear/initramfs.authorized_keys"'
    echo 'dropbear_keytypes="ed25519"'
    echo 'dropbear_ed25519_key="/etc/dropbear/initramfs_ed25519_key"'
  else
    echo 'omit_dracutmodules+=" network network-manager "'
  fi
} > "$ROOT/etc/dracut.conf.d/20-host.conf"

if [[ $SSH == yes ]]; then
  # The main system's initramfs key and allowed keys, for both: whoever unlocks the main system
  # over SSH can reach the recovery the same way, and the host key stays the same.
  for f in initramfs.authorized_keys initramfs_ed25519_key; do
    [[ -s /etc/dropbear/$f ]] || { echo "update.sh: SSH=yes but no /etc/dropbear/$f" >&2; exit 1; }
  done
  packages+=(sys-kernel/dracut-crypt-ssh net-misc/openssh)
  install -D -m 600 /etc/dropbear/initramfs.authorized_keys "$ROOT/etc/dropbear/initramfs.authorized_keys"
  install -D -m 600 /etc/dropbear/initramfs_ed25519_key "$ROOT/etc/dropbear/initramfs_ed25519_key"
  install -D -m 600 /etc/dropbear/initramfs.authorized_keys "$ROOT/root/.ssh/authorized_keys"
  services+=(sshd.service)
fi

nspawn=(systemd-nspawn --quiet --directory="$ROOT" --resolv-conf=copy-host --slice=build.slice
  --bind-ro=/var/db/repos/gentoo --bind=/var/cache/distfiles)

"${nspawn[@]}" emerge --quiet-build --update --deep --newuse --noreplace "${packages[@]}" @world
"${nspawn[@]}" emerge --quiet --depclean

if [[ $SSH == yes ]]; then
  # sshd with the same host key as dropbear (converted to OpenSSH's format), keys only.
  "${nspawn[@]}" dropbearconvert dropbear openssh /etc/dropbear/initramfs_ed25519_key \
    /etc/ssh/ssh_host_ed25519_key >/dev/null
  "${nspawn[@]}" sh -c 'ssh-keygen -y -f /etc/ssh/ssh_host_ed25519_key > /etc/ssh/ssh_host_ed25519_key.pub'
  mkdir -p "$ROOT/etc/ssh/sshd_config.d"
  cat > "$ROOT/etc/ssh/sshd_config.d/10-recovery.conf" <<'EOF'
# Written by update.sh (SSH=yes): the port and host key of the main system's initramfs unlock.
Port 2222
HostKey /etc/ssh/ssh_host_ed25519_key
PermitRootLogin prohibit-password
PasswordAuthentication no
KbdInteractiveAuthentication no
EOF
fi
systemctl --root="$ROOT" enable "${services[@]}" >/dev/null
ln -sf ../usr/lib/systemd/resolv.conf "$ROOT/etc/resolv.conf"
date -u +%FT%TZ > "$ROOT/etc/deck-recovery-updated"

if [[ -n $ESP ]]; then
  [[ -s $ROOT/etc/dracut.conf.d/90-cmdline.conf ]] ||
    { echo "update.sh: no 90-cmdline.conf yet (install.sh writes it); not building the UKI" >&2; exit 1; }
  kver=$(ls "$ROOT/lib/modules" | sort -V | tail -1)
  "${nspawn[@]}" --bind="$ESP:/efi" dracut --force --uefi --kver "$kver" "/efi/EFI/Linux/$RECOVERY_LABEL.efi"
  echo "update.sh: $ESP/EFI/Linux/$RECOVERY_LABEL.efi for $kver"
fi
