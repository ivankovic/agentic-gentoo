#!/usr/bin/env bash
# build.sh [dir]: build deck's recovery root on the running main system, in dir (default
# /var/tmp/deck-recovery/root). Nothing here touches the disk layout or the ESP: install.sh copies
# the finished root onto partition 3 later. Safe to rerun; it picks up where it left off.
#
# 1. the latest amd64 systemd stage3, checked against Gentoo's release key
# 2. root/ from this directory on top (portage config, dracut, fstab, iwd, autologin, helpers)
# 3. update.sh: the recovery's packages, installed inside it with systemd-nspawn
set -euo pipefail
[[ $EUID == 0 ]] || { echo "build.sh: run as root" >&2; exit 1; }
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=${1:-/var/tmp/deck-recovery/root}
work=$(dirname "$ROOT")
mirror=https://distfiles.gentoo.org/releases/amd64/autobuilds

if [[ ! -e $ROOT/etc/gentoo-release ]]; then
  mkdir -p "$work/download"
  cd "$work/download"
  path=$(curl -fsSL "$mirror/latest-stage3-amd64-systemd.txt" | awk '/^[0-9]+T[0-9]+Z\/stage3-/ {print $1; exit}')
  file=${path##*/}
  [[ -s $file ]] || curl -fSLO "$mirror/$path"
  curl -fSsLO "$mirror/$path.asc"
  gnupg=$(mktemp -d)
  gpg --homedir "$gnupg" --quiet --import /usr/share/openpgp-keys/gentoo-release.asc
  gpg --homedir "$gnupg" --verify "$file.asc" "$file"
  rm -rf "$gnupg"
  mkdir -p "$ROOT"
  tar xpf "$file" --xattrs-include='*.*' --numeric-owner -C "$ROOT"
  echo "build.sh: extracted $file"
fi

# Our files on top. os-release is a symlink into /usr/lib in the stage3; replace it, don't write
# through it.
rm -f "$ROOT/etc/os-release"
cp -rT --preserve=mode,timestamps "$here/root" "$ROOT"
chown -R root:root "$ROOT/etc" "$ROOT/usr/local"
install -m 644 "$here/README.md" "$ROOT/usr/local/share/RECOVERY.md"

# Console root login without a password (autologin on tty1, and the emergency shell must work);
# the LUKS passphrase is the gate. No sshd.
chroot "$ROOT" passwd -d root >/dev/null

"$here/update.sh" "$ROOT"
echo "build.sh: recovery root ready in $ROOT ($(du -sh "$ROOT" | cut -f1))"
