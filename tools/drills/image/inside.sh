#!/usr/bin/env bash
# inside.sh: run by build.sh inside the stage3 container; turns it into the drill machine.
set -euo pipefail
export FEATURES="-ipc-sandbox -network-sandbox -pid-sandbox"   # no namespaces in the container
# Binary packages where the binhost has them; the rest compile (small ones, in the container).
emerge_bin() { emerge --quiet-build --usepkg --binpkg-respect-use=y "$@"; }

echo "inside: Gentoo repository"
mkdir -p /var/db/repos/gentoo.copy
cp -a /var/db/repos/gentoo/. /var/db/repos/gentoo.copy/   # the mount isn't exported; the copy is
getuto
cat >> /etc/portage/make.conf <<'EOF'
FEATURES="getbinpkg binpkg-request-signature"
EOF

echo "inside: kernel and tools"
mkdir -p /etc/portage/package.use /etc/portage/package.license
echo 'sys-kernel/linux-firmware linux-fw-redistributable' > /etc/portage/package.license/firmware
# The kernel without its own initramfs: dracut below makes a generic one with the virtio drivers.
echo 'sys-kernel/gentoo-kernel-bin -initramfs' > /etc/portage/package.use/kernel
# gentoo-kernel-bin is itself a prebuilt kernel, not on the binhost: an ordinary emerge, no compiling.
emerge --quiet-build --usepkg --binpkg-respect-use=y sys-kernel/gentoo-kernel-bin
emerge_bin app-admin/sudo sys-process/lsof dev-debug/strace \
  sys-process/htop app-portage/gentoolkit app-portage/portage-utils sys-kernel/dracut

kv=$(ls /lib/modules | sort -V | tail -1)
for img in /boot/vmlinuz-"$kv" /usr/src/linux-"$kv"/arch/x86/boot/bzImage; do
  [[ -f $img ]] && break
done
cp "$img" /boot/vmlinuz
dracut --force --no-hostonly --kver "$kv" \
  --add-drivers "virtio_blk virtio_pci virtio_net virtio_console ext4" /boot/initramfs.img

echo "inside: binary packages the drills install, cached"
# The binary package, or the sources where there is none: offline, emerge installs or compiles.
grep -v '^#' /build/packages.txt | xargs -r emerge --fetchonly --usepkg --binpkg-respect-use=y

# Portage keeps what it downloads from the binhost in its own cache, read only while the binhost is
# on; copied into PKGDIR, with a fresh index, emerge --usepkg finds the packages offline.
cp -a /var/cache/binhost/gentoo/. /var/cache/binpkgs/
rm -f /var/cache/binpkgs/Packages
emaint binhost --fix
rm -rf /var/cache/binhost

echo "inside: system configuration"
# Offline from here on: emerge uses the local cache only.
sed -i '/^FEATURES="getbinpkg/d' /etc/portage/make.conf
cat >> /etc/portage/make.conf <<'EOF'
# The drill machine has no network: binary packages come from the local cache only.
FEATURES="-getbinpkg"
EMERGE_DEFAULT_OPTS="--usepkg --binpkg-respect-use=y"
EOF

echo 'LABEL=drill / ext4 defaults,noatime 0 1' > /etc/fstab
mkdir -p /var/log/journal /root/.ssh
install -m 600 /build-key.pub /root/.ssh/authorized_keys
cat > /etc/systemd/network/50-wired.network <<'EOF'
[Match]
Name=en* eth*

[Network]
DHCP=yes
EOF
systemctl enable sshd systemd-networkd systemd-resolved
echo 3bb5e9a0c4d14f2a9c0f6f2b1e7d8a11 > /etc/machine-id
rm -rf /var/tmp/portage/*

echo "inside: offline install test"
for a in $(grep -v '^#' /build/packages.txt); do
  emerge --pretend --fetchonly "$a" 2>&1 | grep -q 'Downloading' && { echo "inside: $a needs a download" >&2; exit 1; }
done
echo "inside: done"
