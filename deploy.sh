#!/usr/bin/env bash
# deploy.sh: compare or install a host's tracked config files on this machine.
#
#   ./deploy.sh [host] [diff]   show what differs from the system (default)
#   ./deploy.sh [host] apply    copy the differing files to the system, after asking
#
# host defaults to $(hostname). Mapping, as in hosts/<host>/README.md:
#   hosts/<host>/home/     -> ~
#   hosts/<host>/etc/      -> /etc       (sudo)
#   hosts/<host>/portage/  -> /etc/portage (sudo)
#   hosts/<host>/usr-local/ -> /usr/local  (sudo)
#   hosts/<host>/recovery/root/, README.md -> /etc/deck-recovery/root/, RECOVERY.md (sudo): the
#     recovery system's files, which its update (etc/deck-recovery/update.sh) puts on it
# Everything else (state/, README.md, ...) is a record, not deployed.
#
# A host kept elsewhere: DECK_HOST_DIR=<dir> ./deploy.sh <host> takes the host's files from <dir>
# (a git checkout) instead of hosts/<host>. A host can share another's files: hosts/<host>/inherit
# (or <dir>/inherit) names repository paths, one per line, e.g. hosts/deck/home/.config/niri or
# hosts/deck/recovery/root, deployed as if they were its own. Its own files win over inherited ones
# with the same target.
#
# Only files tracked by git are deployed, in their working-tree version. Deploying only ever goes
# repo -> system, and never deletes anything on the system: a file removed from the repo stays on
# the machine until you remove it by hand.
set -euo pipefail

# Resolved before the cd below, so a relative directory means one relative to where you are.
ext=${DECK_HOST_DIR:+$(realpath -- "$DECK_HOST_DIR")}
root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cd "$root"

host=$(hostname)
mode=diff
for arg in "$@"; do
  case $arg in
    diff | apply) mode=$arg ;;
    -h | --help) sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) host=$arg ;;
  esac
done

dir=${ext:-hosts/$host}
[[ -d $dir ]] || { echo "deploy: no $dir; hosts: $(ls hosts | tr '\n' ' ')" >&2; exit 2; }

# Prints "<file>\t<target path>\t<sudo|user>" for every tracked, deployable file in host directory
# $1, limited to the paths after it (relative to it).
host_files() {
  local hd=$1; shift
  git -C "$hd" ls-files -z -- "$@" | while IFS= read -r -d '' rel; do
    src=${hd%/}/$rel
    src=${src#./}
    case $rel in
      home/*) printf '%s\t%s\tuser\n' "$src" "$HOME/${rel#home/}" ;;
      etc/*) printf '%s\t%s\tsudo\n' "$src" "/etc/${rel#etc/}" ;;
      portage/*) printf '%s\t%s\tsudo\n' "$src" "/etc/portage/${rel#portage/}" ;;
      usr-local/*) printf '%s\t%s\tsudo\n' "$src" "/usr/local/${rel#usr-local/}" ;;
      recovery/root/*) printf '%s\t%s\tsudo\n' "$src" "/etc/deck-recovery/root/${rel#recovery/root/}" ;;
      recovery/README.md) printf '%s\t%s\tsudo\n' "$src" /etc/deck-recovery/RECOVERY.md ;;
    esac
  done
}

# What the host inherits: "<host directory>\t<path in it>" per line of its inherit file.
inherited=()
if [[ -f $dir/inherit ]]; then
  while read -r p; do
    [[ -z $p || $p == '#'* ]] && continue
    [[ $p =~ ^(hosts/[^/]+)/(((home|etc|portage|usr-local|recovery/root)(/.*)?)|recovery/README\.md)$ && -e $p ]] \
      || { echo "deploy: $dir/inherit: $p is not a deployable path under hosts/<host>/" >&2; exit 2; }
    inherited+=("${BASH_REMATCH[1]}"$'\t'"${BASH_REMATCH[2]}")
  done <"$dir/inherit"
fi

# The host's own files, then what it inherits; the first file for a target wins.
targets() {
  {
    host_files "$dir" home etc portage usr-local recovery/root recovery/README.md
    local e
    for e in "${inherited[@]}"; do host_files "${e%%$'\t'*}" "${e#*$'\t'}"; done
  } | awk -F'\t' '!seen[$2]++'
}

readable() { [[ -r $1 ]]; }
exe() { [[ -x $1 ]] && echo yes || echo no; }

changed=()   # "src\tdst\tsudo|user"
unreadable=()
while IFS=$'\t' read -r src dst who; do
  if [[ -e $dst ]] && ! readable "$dst"; then
    unreadable+=("$dst")
    changed+=("$src"$'\t'"$dst"$'\t'"$who")
  elif [[ ! -e $dst ]]; then
    echo "new:      $dst"
    changed+=("$src"$'\t'"$dst"$'\t'"$who")
  elif ! cmp -s "$src" "$dst"; then
    echo "changed:  $dst"
    [[ $mode == diff ]] && { diff -u --label "$dst (system)" --label "$src (repo)" "$dst" "$src" || true; }
    changed+=("$src"$'\t'"$dst"$'\t'"$who")
  elif [[ $(exe "$src") != $(exe "$dst") ]]; then
    # Same content, but git's executable bit differs: install keeps the repo's mode.
    echo "mode:     $dst (executable in the repo: $(exe "$src"))"
    changed+=("$src"$'\t'"$dst"$'\t'"$who")
  fi
done < <(targets)

for f in "${unreadable[@]}"; do echo "unreadable: $f (not compared; apply will overwrite it)"; done

if ((${#changed[@]} == 0)); then
  echo "deploy: $host is up to date."
  exit 0
fi

if [[ $mode == diff ]]; then
  echo
  echo "deploy: ${#changed[@]} file(s) differ. Run './deploy.sh $host apply' to install them."
  exit 0
fi

echo
echo "deploy: will install ${#changed[@]} file(s) from $dir:"
need_sudo=0
for entry in "${changed[@]}"; do
  IFS=$'\t' read -r src dst who <<<"$entry"
  printf '  %-5s %s\n' "$who" "$dst"
  [[ $who == sudo ]] && need_sudo=1
done
((need_sudo)) && echo "Files marked sudo are installed as root:root with sudo."
read -r -p "Proceed? [y/N] " answer
[[ $answer == [yY] ]] || { echo "deploy: nothing installed."; exit 1; }

for entry in "${changed[@]}"; do
  IFS=$'\t' read -r src dst who <<<"$entry"
  # Keep the repo's mode (git tracks the executable bit).
  perm=$(stat -c %a "$src")
  if [[ $who == sudo ]]; then
    sudo install -D -o root -g root -m "$perm" "$src" "$dst"
  else
    install -D -m "$perm" "$src" "$dst"
  fi
  echo "installed $dst"
done
