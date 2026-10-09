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
# Everything else (state/, README.md, ...) is a record, not deployed.
#
# Only files tracked by git are deployed, in their working-tree version. Deploying only ever goes
# repo -> system, and never deletes anything on the system: a file removed from the repo stays on
# the machine until you remove it by hand.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cd "$root"

host=$(hostname)
mode=diff
for arg in "$@"; do
  case $arg in
    diff | apply) mode=$arg ;;
    -h | --help) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) host=$arg ;;
  esac
done

dir=hosts/$host
[[ -d $dir ]] || { echo "deploy: no $dir; hosts: $(ls hosts | tr '\n' ' ')" >&2; exit 2; }

# Prints "<repo path>\t<target path>\t<sudo|user>" for every tracked, deployable file.
targets() {
  git ls-files -z -- "$dir/home" "$dir/etc" "$dir/portage" | while IFS= read -r -d '' src; do
    rel=${src#"$dir"/}
    case $rel in
      home/*) printf '%s\t%s\tuser\n' "$src" "$HOME/${rel#home/}" ;;
      etc/*) printf '%s\t%s\tsudo\n' "$src" "/etc/${rel#etc/}" ;;
      portage/*) printf '%s\t%s\tsudo\n' "$src" "/etc/portage/${rel#portage/}" ;;
    esac
  done
}

readable() { [[ -r $1 ]]; }

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
