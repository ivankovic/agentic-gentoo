#!/usr/bin/env bash
# install.sh [root]: put the recovery system built by build.sh (default /var/tmp/deck-recovery/root)
# on a new partition 3, online, from the running main system. Run as root, with a backup made.
#
#   Before:  p1 ESP 1 GiB | p2 LUKS2 → btrfs (the rest)
#   After:   p1 ESP 1 GiB | p2 LUKS2 → btrfs (32 GiB less) | p3 LUKS2 → ext4 "deck-recovery"
#
# Shrinking goes outside in, each layer smaller than the one around it, and every step is checked
# before the next:
#   1. btrfs shrinks to 1 GiB below its new room (only unallocated space goes; 400 GiB is free)
#   2. the dm-crypt mapping shrinks to exactly the new partition minus the LUKS header
#   3. the partition table: p2 ends earlier (same start, type, GUID and name), p3 takes the rest
#   4. the mapping, then btrfs, grow back to fill the new p2
# Then p3: LUKS2 (you type the passphrase: use the main one), ext4, the built root, the kernel
# command line with p3's UUID, the Wi-Fi networks, and its UKI on the ESP as deck-recovery.efi.
# LUKS header backups go both ways: p2's into the recovery, p3's into the main system's /root.
#
# If the kernel won't take the shrunk p2 while it's in use (step 3), the script stops after
# writing the table and asks for a reboot; run it again afterwards and it continues from step 4.
set -euo pipefail
[[ $EUID == 0 ]] || { echo "install.sh: run as root" >&2; exit 1; }
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=${1:-/var/tmp/deck-recovery/root}

DISK=/dev/nvme0n1
P2=${DISK}p2
P3=${DISK}p3
MAPPER=root
RECOVERY_SECTORS=$((32 * 1024 * 1024 * 2)) # 32 GiB in 512-byte sectors
ALIGN=2048                                   # 1 MiB
MARGIN=$((1024 * 1024 * 1024))               # btrfs stays 1 GiB below the mapping while shrinking

die() { echo "install.sh: $*" >&2; exit 1; }
sectors() { cat "/sys/class/block/$(basename "$1")/size"; }
start() { cat "/sys/class/block/$(basename "$1")/start"; }

[[ $(cat /sys/block/nvme0n1/queue/logical_block_size) == 512 ]] || die "expected 512-byte sectors"
[[ -e $ROOT/etc/deck-recovery-updated ]] || die "no built root in $ROOT; run build.sh first"
dm=$(basename "$(readlink -f /dev/mapper/$MAPPER)")
[[ $(ls /sys/class/block/$dm/slaves) == "$(basename "$P2")" ]] || die "/dev/mapper/$MAPPER isn't on $P2"

p2_start=$(start "$P2")
offset=$(cryptsetup status "$MAPPER" | awk '/offset:/ {print $2}')  # LUKS2 header, in sectors
[[ $offset =~ ^[0-9]+$ ]] || die "couldn't read the LUKS data offset"
type2=$(sgdisk -i 2 "$DISK" | awk -F': ' '/Partition GUID code/ {print $2}' | cut -d' ' -f1)
guid2=$(sgdisk -i 2 "$DISK" | awk -F': ' '/Partition unique GUID/ {print $2}')
name2=$(sgdisk -i 2 "$DISK" | awk -F"'" '/Partition name/ {print $2}')
last=$(sgdisk -p "$DISK" | sed -n 's/.*last usable sector is \([0-9]*\).*/\1/p')  # (-E is the largest gap's end)
[[ $last =~ ^[0-9]+$ ]] || die "couldn't read the last usable sector"

if [[ ! -e $P3 ]] && ! sgdisk -i 3 "$DISK" | grep -q 'Partition GUID code'; then
  # The new p2: shrunk by the recovery size, rounded down to 1 MiB, so p3 starts aligned.
  old2=$(sectors "$P2")
  new2=$(( (old2 - RECOVERY_SECTORS) / ALIGN * ALIGN ))
  p3_start=$((p2_start + new2))
  map_sectors=$((new2 - offset))
  fs_bytes=$((map_sectors * 512 - MARGIN))
  used=$(btrfs filesystem usage -b / | awk '/Device allocated:/ {print $3}')

  cat <<TXT
Plan:
  p2   $P2: $old2 → $new2 sectors ($((new2 / 2097152)) GiB), start $p2_start unchanged
       LUKS header $offset sectors; mapping → $map_sectors sectors
       btrfs → $fs_bytes bytes first (allocated now: $used), then grown back to fill
  p3   $P3: sectors $p3_start–$last ($(((last - p3_start + 1) / 2097152)) GiB)
TXT
  (( fs_bytes > used + 20 * 1024 * 1024 * 1024 )) || die "btrfs would end up with under 20 GiB headroom"
  (( p3_start % ALIGN == 0 )) || die "p3 start not aligned"
  (( last - p3_start + 1 >= RECOVERY_SECTORS - ALIGN )) || die "p3 would be $((last - p3_start + 1)) sectors, too small"
  read -r -p "Type SHRINK to go ahead: " answer
  [[ $answer == SHRINK ]] || die "stopped; nothing changed"

  backup=/root/luks-header-deck-$(date +%Y%m%d).img
  [[ -e $backup ]] || cryptsetup luksHeaderBackup "$P2" --header-backup-file "$backup"
  echo "LUKS header of p2 backed up to $backup"

  echo "1. btrfs → $fs_bytes"
  btrfs filesystem resize "$fs_bytes" /
  fs_now=$(btrfs filesystem show --raw / | awk '/devid/ {print $4}')
  (( fs_now <= fs_bytes )) || die "btrfs is $fs_now bytes, not shrunk"

  echo "2. dm-crypt mapping → $map_sectors sectors"
  cryptsetup resize "$MAPPER" --size "$map_sectors"
  (( $(sectors "/dev/$dm") == map_sectors )) || die "mapping is $(sectors "/dev/$dm") sectors"
  (( fs_now <= map_sectors * 512 )) || die "btrfs larger than the mapping"

  echo "3. partition table"
  sgdisk --backup="/root/gpt-deck-$(date +%Y%m%d%H%M).bin" "$DISK" >/dev/null
  sgdisk -d 2 -n "2:$p2_start:$((p2_start + new2 - 1))" -t "2:$type2" -u "2:$guid2" -c "2:$name2" \
    -n "3:$p3_start:$last" -t 3:8309 -c 3:deck-recovery "$DISK"
  partx -u -n 2 "$DISK" || true
  partx -a -n 3 "$DISK" || true  # may already be there; what counts is the kernel's view below
  udevadm settle
  if [[ -e $P3 ]] && (( $(sectors "$P2") == new2 )); then
    echo "   the kernel took the new table"
  else
    echo
    echo "The table is written, but the kernel still has the old p2 in use. Reboot (the main system"
    echo "comes up as before: the mapping is sized from the new p2), then run install.sh again."
    exit 0
  fi
fi

echo "4. dm-crypt mapping, then btrfs → fill p2"
cryptsetup resize "$MAPPER"  # no size: the partition minus the header
(( $(sectors "/dev/$dm") == $(sectors "$P2") - offset )) || die "mapping is $(sectors "/dev/$dm") sectors, not p2 - header"
btrfs filesystem resize max /

echo "5. p3: LUKS2 (use the main passphrase), ext4"
if ! cryptsetup isLuks "$P3"; then
  cryptsetup luksFormat --type luks2 --pbkdf argon2id --label deck-recovery "$P3"
fi
uuid3=$(cryptsetup luksUUID "$P3")
[[ -e /dev/mapper/recovery ]] || cryptsetup open "$P3" recovery
blkid /dev/mapper/recovery >/dev/null || mkfs.ext4 -q -L deck-recovery /dev/mapper/recovery
mnt=$(mktemp -d)
mount /dev/mapper/recovery "$mnt"
trap 'umount "$mnt" 2>/dev/null; cryptsetup close recovery 2>/dev/null; rmdir "$mnt"' EXIT

echo "6. the root, its command line, Wi-Fi networks, header backups"
rsync -aHAX --delete --info=progress2 "$ROOT/" "$mnt/"
cat > "$mnt/etc/dracut.conf.d/90-cmdline.conf" <<CONF
# Written by install.sh: p3's LUKS UUID.
kernel_cmdline="rd.luks.name=$uuid3=recovery root=/dev/mapper/recovery rw"
CONF
mkdir -p "$mnt/var/lib/iwd"
cp -a /var/lib/iwd/*.psk "$mnt/var/lib/iwd/" 2>/dev/null || true
chmod 600 "$mnt"/var/lib/iwd/*.psk 2>/dev/null || true
cp -a /root/luks-header-deck-*.img "$mnt/root/"
cryptsetup luksHeaderBackup "$P3" --header-backup-file "/root/luks-header-recovery-$(date +%Y%m%d).img" 2>/dev/null || true

echo "7. UKI"
"$here/update.sh" "$mnt" /efi
ls -la /efi/EFI/Linux/
echo
echo "Done. In the systemd-boot menu: \"Deck recovery\". The main system stays the default."
