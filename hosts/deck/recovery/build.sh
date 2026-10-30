#!/usr/bin/env bash
# build.sh [dir]: build this machine's recovery root on the running main system, in dir (default
# /var/tmp/deck-recovery/root), as host.conf describes it (RECOVERY_CONF, default
# /etc/deck-recovery/host.conf). Nothing here touches the disk layout or the ESP: install.sh copies
# the finished root onto its partition later. Safe to rerun; it picks up where it left off.
#
# 1. the latest amd64 systemd stage3, checked against Gentoo's release key
# 2. update.sh: root/ from this directory on top (portage config, dracut, iwd, autologin, helpers),
#    the deck router and what host.conf sets (name, fstab, network, SSH), then the recovery's
#    packages, installed inside it with systemd-nspawn
set -euo pipefail
[[ $EUID == 0 ]] || { echo "build.sh: run as root" >&2; exit 1; }
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=${1:-/var/tmp/deck-recovery/root}
export RECOVERY_CONF=${RECOVERY_CONF:-/etc/deck-recovery/host.conf}
[[ -s $RECOVERY_CONF ]] || { echo "build.sh: no $RECOVERY_CONF (deploy.sh installs it)" >&2; exit 1; }
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

# Console root login without a password (autologin on tty1, and the emergency shell must work);
# the LUKS passphrase is the gate. With SSH=yes in host.conf, sshd takes keys only.
chroot "$ROOT" passwd -d root >/dev/null

RECOVERY_FILES=$here/root RECOVERY_DOC=$here/README.md RECOVERY_ROUTER=$here/../usr-local/bin/deck \
  "$here/../etc/deck-recovery/update.sh" "$ROOT"
echo "build.sh: recovery root ready in $ROOT ($(du -sh "$ROOT" | cut -f1))"
