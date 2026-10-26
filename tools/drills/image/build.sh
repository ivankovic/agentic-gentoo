#!/usr/bin/env bash
# build.sh <out dir>: the drill machine's disk image, built with Docker (no root, no privileges).
#
# Writes to <out dir>:
#   base.raw              ext4 root file system, the disk every drill starts from (copy-on-write)
#   vmlinuz, initramfs    the kernel and initramfs, booted directly by QEMU
#   key, key.pub          the SSH key the runner logs in with as root
#
# The machine is a Gentoo systemd stage3 with a dist kernel, sshd, the Gentoo repository and
# a local binary package cache for the packages the drills install (packages.txt next to this
# script), so emerge works with no network: the drills run offline, as the recovery does.
set -euo pipefail

out=$(realpath -m -- "${1:?usage: build.sh <out dir>}")
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
mkdir -p "$out"
[[ -f $out/key ]] || ssh-keygen -q -t ed25519 -N '' -C drill-runner -f "$out/key"

name=drill-build-$$
trap 'docker rm -f "$name" "$name-tree" >/dev/null 2>&1 || true' EXIT

docker pull -q gentoo/stage3:systemd >/dev/null
docker pull -q gentoo/portage:latest >/dev/null
docker create --name "$name-tree" gentoo/portage:latest true >/dev/null

# Everything that installs or configures runs inside the stage3 container; its file system,
# exported, becomes the disk.
docker run --name "$name" --volumes-from "$name-tree" \
  -v "$here:/build:ro" -v "$out/key.pub:/build-key.pub:ro" \
  gentoo/stage3:systemd bash /build/inside.sh

echo "build: exporting the file system"
rm -rf "$out/root.tar"
docker export "$name" -o "$out/root.tar"

# mkfs.ext4 -d fills the file system from a directory, so no loop mount (and no root) is needed;
# unpacking as root inside a throwaway container keeps owners and modes.
echo "build: making the ext4 image"
docker run --rm -v "$out:/out" gentoo/stage3:systemd bash -euc '
  mkdir /r && tar -xpf /out/root.tar -C /r --numeric-owner
  rm -f /r/.dockerenv
  rm -rf /r/var/db/repos/gentoo && mv /r/var/db/repos/gentoo.copy /r/var/db/repos/gentoo
  # Docker bind-mounts these; the exported copies are empty or the build host'"'"'s.
  echo drill > /r/etc/hostname
  printf "127.0.0.1 localhost drill\n::1 localhost\n" > /r/etc/hosts
  ln -sf ../run/systemd/resolve/stub-resolv.conf /r/etc/resolv.conf
  cp /r/boot/vmlinuz /out/vmlinuz && cp /r/boot/initramfs.img /out/initramfs
  rm -f /out/base.raw
  truncate -s 16G /out/base.raw
  mkfs.ext4 -q -L drill -d /r /out/base.raw
  chown '"$(id -u):$(id -g)"' /out/base.raw /out/vmlinuz /out/initramfs
'
rm -f "$out/root.tar"
echo "build: done: $out/base.raw"
