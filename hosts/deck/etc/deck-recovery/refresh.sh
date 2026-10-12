#!/usr/bin/env bash
# refresh.sh [--enroll | --force]: update the recovery system on p3 in place, from the main system.
#
# Run weekly by deck-recovery-update.timer, on AC only. It does nothing while the last update is
# under 25 days old, so on a laptop that's often unplugged an update still lands about monthly.
# p3 is opened with a key file kept on the main system's encrypted root, enrolled once with
#   sudo bash /etc/deck-recovery/refresh.sh --enroll     (asks for the recovery passphrase)
# Then update.sh (in this directory) updates its packages and rebuilds deck-recovery.efi, and
# /var/lib/deck-recovery/updated records when, for the omni panel, which warns after 6 weeks.
# --force updates now regardless of age.
set -euo pipefail
[[ $EUID == 0 ]] || { echo "refresh.sh: run as root" >&2; exit 1; }
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
P3=/dev/disk/by-partlabel/deck-recovery
KEY=/etc/deck-recovery/p3.key
STAMP=/var/lib/deck-recovery/updated
MAPPER=recovery-update
MIN_AGE_DAYS=25

[[ -e $P3 ]] || { echo "refresh.sh: no $P3" >&2; exit 1; }
mkdir -p "${STAMP%/*}"

if [[ ${1:-} == --enroll ]]; then
  [[ -s $KEY ]] || (umask 077 && head -c 64 /dev/urandom > "$KEY")
  if cryptsetup open --test-passphrase --key-file "$KEY" "$P3" 2>/dev/null; then
    echo "refresh.sh: $KEY already opens p3"
  else
    echo "Adding $KEY to p3's key slots; type the recovery passphrase:"
    cryptsetup luksAddKey "$P3" "$KEY"
  fi
fi

if [[ ${1:-} == "" ]] && [[ -s $STAMP ]]; then
  age=$(( ($(date +%s) - $(date -d "$(cat "$STAMP")" +%s)) / 86400 ))
  if (( age < MIN_AGE_DAYS )); then
    echo "refresh.sh: updated $age days ago; next update after $MIN_AGE_DAYS"
    exit 0
  fi
fi

[[ -s $KEY ]] || { echo "refresh.sh: no $KEY; run refresh.sh --enroll once" >&2; exit 1; }
mnt=$(mktemp -d /run/deck-recovery.XXXXXX)
cleanup() { umount "$mnt" 2>/dev/null || true; cryptsetup close "$MAPPER" 2>/dev/null || true; rmdir "$mnt"; }
trap cleanup EXIT
cryptsetup open --key-file "$KEY" "$P3" "$MAPPER"
mount "/dev/mapper/$MAPPER" "$mnt"

if [[ ${1:-} != --enroll ]]; then
  "$here/update.sh" "$mnt" /efi
fi
install -m 644 "$mnt/etc/deck-recovery-updated" "$STAMP"
echo "refresh.sh: recovery last updated $(cat "$STAMP")"
