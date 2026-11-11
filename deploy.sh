#!/usr/bin/env bash
# deploy.sh: compare or install a host's tracked config files on this machine.
#
#   ./deploy.sh [host] [diff]     show what differs from the system (default)
#   ./deploy.sh [host] apply      show it, then copy the differing files to the system, after asking
#   ./deploy.sh [host] list       every deployed file as "<repo path>\t<system path>", for scripts
#   ./deploy.sh [host] untracked  files in /etc/portage and /etc/systemd/system that no deployed
#                                 file covers: what exists only on the system
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
#
# apply runs as you, not as root (it calls sudo itself; under sudo, home/ would land in /root). It
# marks files whose repo version isn't committed, checks a new /etc/fstab (findmnt --verify) and
# that every module a new /etc/pam.d file names is installed, and installs nothing when one of
# those fails. What it overwrites is first copied to /var/backups/deploy/<time>/ (root's files)
# and ~/.local/state/deck/deploy-backups/<time>/ (the home's). A tracked file missing from the
# working tree is reported and skipped.
#
# untracked leaves out /etc/portage/gnupg and savedconfig, *.disabled, .keep*, sets a program wrote
# (their first line says "# Written by"), symlinks (units enabled or masked), and what the optional
# /etc/deck/host.conf lists: DEPLOY_UNTRACKED_IGNORE="<glob> <glob> ..." (system paths).
#
# DEPLOY_ROOT=<dir> stands in for / (the home stays $HOME) and nothing runs through sudo: for the
# tests in tests/system.
set -euo pipefail

# Resolved before the cd below, so a relative directory means one relative to where you are.
ext=${DECK_HOST_DIR:+$(realpath -- "$DECK_HOST_DIR")}
sysroot=${DEPLOY_ROOT:+$(realpath -- "$DEPLOY_ROOT")}
sysroot=${sysroot%/}
root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cd "$root"

host=$(hostname)
mode=diff
for arg in "$@"; do
  case $arg in
    diff | apply | list | untracked) mode=$arg ;;
    -h | --help) sed -n '2,/^set /{/^#/s/^# \{0,1\}//p}' "$0"; exit 0 ;;
    *) host=$arg ;;
  esac
done

dir=${ext:-hosts/$host}
[[ -d $dir ]] || { echo "deploy: no $dir; hosts: $(ls hosts | tr '\n' ' ')" >&2; exit 2; }
if [[ $mode == apply && $EUID == 0 ]]; then
  echo "deploy: apply runs as the user, not as root or under sudo: home/ would go to root's home." >&2
  echo "        It asks for sudo itself for the files outside the home." >&2
  exit 2
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Runs a command as root; with DEPLOY_ROOT, as it is.
as_root() { if [[ -n $sysroot ]]; then "$@"; else sudo "$@"; fi; }

# A value from the optional /etc/deck/host.conf (lines KEY=value, the value may be quoted), or $2.
conf() {
  local v
  v=$(sed -n "s/^[[:space:]]*$1=//p" "$sysroot/etc/deck/host.conf" 2>/dev/null | tail -n 1)
  [[ -n $v ]] || { printf '%s\n' "${2-}"; return 0; }
  v=${v%\"}; v=${v#\"}; v=${v%\'}; v=${v#\'}
  printf '%s\n' "$v"
}

# Prints "<file>\t<target path>\t<sudo|user>" for every tracked, deployable file in host directory
# $1, limited to the paths after it (relative to it). Fails when git can't list them.
host_files() {
  local hd=$1 rel src; shift
  if ! git -C "$hd" ls-files -z -- "$@" >"$tmp/ls" 2>"$tmp/err"; then
    echo "deploy: can't list the tracked files of $hd (it has to be in a git checkout):" >&2
    sed 's/^/  /' "$tmp/err" >&2
    return 1
  fi
  while IFS= read -r -d '' rel; do
    src=${hd%/}/$rel
    src=${src#./}
    case $rel in
      home/*) printf '%s\t%s\tuser\n' "$src" "$HOME/${rel#home/}" ;;
      etc/*) printf '%s\t%s\tsudo\n' "$src" "$sysroot/etc/${rel#etc/}" ;;
      portage/*) printf '%s\t%s\tsudo\n' "$src" "$sysroot/etc/portage/${rel#portage/}" ;;
      usr-local/*) printf '%s\t%s\tsudo\n' "$src" "$sysroot/usr/local/${rel#usr-local/}" ;;
      recovery/root/*) printf '%s\t%s\tsudo\n' "$src" "$sysroot/etc/deck-recovery/root/${rel#recovery/root/}" ;;
      recovery/README.md) printf '%s\t%s\tsudo\n' "$src" "$sysroot/etc/deck-recovery/RECOVERY.md" ;;
    esac
  done <"$tmp/ls"
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

# The host's own files, then what it inherits, into $tmp/targets; the first file for a target
# wins. Fails when git can't list a directory, an inherited path has no tracked file, or there is
# nothing to deploy at all: none of those is "up to date".
targets() {
  local e
  host_files "$dir" home etc portage usr-local recovery/root recovery/README.md >"$tmp/all" || return 1
  for e in "${inherited[@]}"; do
    host_files "${e%%$'\t'*}" "${e#*$'\t'}" >"$tmp/part" || return 1
    if [[ ! -s $tmp/part ]]; then
      echo "deploy: $dir/inherit: ${e%%$'\t'*}/${e#*$'\t'} has no tracked files" >&2
      return 1
    fi
    cat "$tmp/part" >>"$tmp/all"
  done
  awk -F'\t' '!seen[$2]++' "$tmp/all" >"$tmp/targets"
  if [[ ! -s $tmp/targets ]]; then
    echo "deploy: $dir has no tracked files to deploy (home/, etc/, portage/, usr-local/, recovery/)" >&2
    return 1
  fi
}
targets || exit 2

if [[ $mode == list ]]; then
  cut -f1,2 "$tmp/targets"
  exit 0
fi

if [[ $mode == untracked ]]; then
  cut -f2 "$tmp/targets" | LC_ALL=C sort -u >"$tmp/known"
  find "$sysroot/etc/portage" "$sysroot/etc/systemd/system" \
    \( -path "$sysroot/etc/portage/gnupg" -o -path "$sysroot/etc/portage/savedconfig" \) -prune \
    -o -type f ! -name '*.disabled' ! -name '.keep*' -print 2>/dev/null | LC_ALL=C sort >"$tmp/found" || true
  read -r -a ignore <<<"$(conf DEPLOY_UNTRACKED_IGNORE)"
  n=0
  while IFS= read -r f; do
    for g in "${ignore[@]}"; do
      # shellcheck disable=SC2053  # the configured globs are meant to match as patterns
      [[ ${f#"$sysroot"} == $g ]] && continue 2
    done
    if [[ $f == "$sysroot"/etc/portage/sets/* ]]; then
      IFS= read -r first <"$f" || true
      [[ ${first-} == '# Written by'* ]] && continue
    fi
    echo "$f"
    n=$((n + 1))
  done < <(LC_ALL=C comm -23 "$tmp/found" "$tmp/known")
  echo "deploy: $n file(s) on the system that $host's files don't cover." >&2
  exit 0
fi

# Files whose working-tree version differs from the last commit, by "<file>" as host_files prints it.
declare -A dirty=()
uncommitted() {
  local hd rel s
  for hd in "$dir" "${inherited[@]%%$'\t'*}"; do
    [[ -n ${dirty[//$hd]-} ]] && continue
    dirty[//$hd]=1
    if ! git -C "$hd" diff --name-only -z --relative HEAD -- . >"$tmp/dirty" 2>/dev/null; then
      echo "deploy: can't tell what is uncommitted in $hd (no commit yet?)" >&2
      continue
    fi
    while IFS= read -r -d '' rel; do
      s=${hd%/}/$rel
      dirty[${s#./}]=1
    done <"$tmp/dirty"
  done
}
uncommitted

exe() { [[ -x $1 ]] && echo yes || echo no; }
# A file this user can't see may still be there: when a directory above it can't be entered.
hidden() {
  local d=${1%/*}
  while [[ -n $d && ! -e $d ]]; do d=${d%/*}; done
  [[ -n $d && ! -x $d ]]
}

changed=()   # "src\tdst\tsudo|user"
unreadable=()
missing=()
while IFS=$'\t' read -r src dst who; do
  if [[ ! -e $src ]]; then
    # Tracked, but deleted in the working tree: there is nothing to install.
    missing+=("$src")
    continue
  fi
  if [[ ! -r $dst ]] && { [[ -e $dst ]] || hidden "$dst"; }; then
    if [[ $mode != apply ]]; then
      unreadable+=("$dst")
      changed+=("$src"$'\t'"$dst"$'\t'"$who")
    elif ! as_root test -e "$dst"; then
      echo "new:      $dst"
      changed+=("$src"$'\t'"$dst"$'\t'"$who")
    elif ! as_root cmp -s "$src" "$dst"; then
      # apply reads it as root, so that what is about to be replaced is shown.
      echo "changed:  $dst"
      as_root cat "$dst" | diff -u --label "$dst (system)" --label "$src (repo)" - "$src" || true
      changed+=("$src"$'\t'"$dst"$'\t'"$who")
    elif [[ $(exe "$src") != $(as_root test -x "$dst" && echo yes || echo no) ]]; then
      echo "mode:     $dst (executable in the repo: $(exe "$src"))"
      changed+=("$src"$'\t'"$dst"$'\t'"$who")
    fi
  elif [[ ! -e $dst ]]; then
    echo "new:      $dst"
    changed+=("$src"$'\t'"$dst"$'\t'"$who")
  elif ! cmp -s "$src" "$dst"; then
    echo "changed:  $dst"
    diff -u --label "$dst (system)" --label "$src (repo)" "$dst" "$src" || true
    changed+=("$src"$'\t'"$dst"$'\t'"$who")
  elif [[ $(exe "$src") != $(exe "$dst") ]]; then
    # Same content, but git's executable bit differs: install keeps the repo's mode.
    echo "mode:     $dst (executable in the repo: $(exe "$src"))"
    changed+=("$src"$'\t'"$dst"$'\t'"$who")
  fi
done <"$tmp/targets"

for f in "${unreadable[@]}"; do echo "unreadable: $f (not compared; apply compares it as root)"; done
for f in "${missing[@]}"; do echo "missing:  $f (tracked, but deleted in the working tree; skipped)"; done

if ((${#changed[@]} == 0)); then
  echo "deploy: $host is up to date."
  exit 0
fi

ndirty=0
for entry in "${changed[@]}"; do
  IFS=$'\t' read -r src dst who <<<"$entry"
  [[ -n ${dirty[$src]-} ]] && ndirty=$((ndirty + 1))
done

if [[ $mode == diff ]]; then
  echo
  echo "deploy: ${#changed[@]} file(s) differ. Run './deploy.sh $host apply' to install them."
  ((ndirty)) && echo "deploy: $ndirty of them not committed in the repo (apply marks them)."
  exit 0
fi

# The modules a pam.d file ($1) names that aren't installed, one per line. Lines that include
# another file name no module, and a type starting with "-" may name a missing one.
pam_missing() {
  sed -e ':a' -e '/\\$/{N;s/\\\n/ /;ba' -e '}' "$1" | awk '
    /^[[:space:]]*(#|$)/ || $1 == "@include" || $1 ~ /^-/ { next }
    {
      i = 2
      if ($i ~ /^\[/) while (i < NF && $i !~ /\]$/) i++
      else if ($i == "include" || $i == "substack") next
      if (i < NF) print $(i + 1)
    }' | sort -u | while read -r m; do
    if [[ $m == /* ]]; then
      [[ -e $sysroot$m ]] || echo "$m"
    else
      compgen -G "$sysroot/lib*/security/$m" >/dev/null ||
        compgen -G "$sysroot/usr/lib*/security/$m" >/dev/null || echo "$m"
    fi
  done
}

# The files a mistake in locks you out with are checked before anything is installed.
invalid=0
for entry in "${changed[@]}"; do
  IFS=$'\t' read -r src dst who <<<"$entry"
  case ${dst#"$sysroot"} in
    /etc/fstab)
      if ! findmnt --verify --tab-file "$src" >"$tmp/verify" 2>&1; then
        echo "deploy: $src fails findmnt --verify:" >&2
        sed 's/^/  /' "$tmp/verify" >&2
        invalid=1
      fi
      ;;
    /etc/pam.d/*)
      lacking=$(pam_missing "$src" | tr '\n' ' ')
      if [[ -n $lacking ]]; then
        echo "deploy: $src names PAM modules that aren't installed: $lacking" >&2
        invalid=1
      fi
      ;;
  esac
done
((invalid)) && { echo "deploy: nothing installed." >&2; exit 1; }

echo
echo "deploy: will install ${#changed[@]} file(s) from $dir:"
need_sudo=0
for entry in "${changed[@]}"; do
  IFS=$'\t' read -r src dst who <<<"$entry"
  printf '  %-5s %s%s\n' "$who" "$dst" "${dirty[$src]:+   <- NOT COMMITTED: $src}"
  [[ $who == sudo ]] && need_sudo=1
done
((need_sudo)) && [[ -z $sysroot ]] && echo "Files marked sudo are installed as root:root with sudo."
((ndirty)) && echo "$ndirty file(s) marked NOT COMMITTED: the diff above is what goes in, not a reviewed commit."
# Files git doesn't track yet are not installed. One that a file above needs (a script's new
# library) would be missing on the system: say so before anything goes in.
new=$(git -C "$dir" ls-files --others --exclude-standard -- home etc portage usr-local 2>/dev/null || true)
if [[ -n $new ]]; then
  echo "Not tracked by git, so NOT installed (git add them first if the files above need them):"
  sed 's/^/  /' <<<"$new"
fi
read -r -p "Proceed? [y/N] " answer
[[ $answer == [yY] ]] || { echo "deploy: nothing installed."; exit 1; }

# Copies what is on the system now under the backup directory $1, as $3 (the path below it).
stamp=$(date +%Y%m%dT%H%M%S)
sysbak=$sysroot/var/backups/deploy/$stamp
userbak=${XDG_STATE_HOME:-$HOME/.local/state}/deck/deploy-backups/$stamp
kept=()
backup() {
  local dst=$1 who=$2 to
  if [[ $who == sudo ]]; then
    as_root test -e "$dst" || return 0
    to=$sysbak${dst#"$sysroot"}
    [[ " ${kept[*]-} " == *" $sysbak "* ]] || { as_root install -d -m 700 "$sysbak" && kept+=("$sysbak"); } || return 1
    as_root mkdir -p "${to%/*}" && as_root cp -a "$dst" "$to"
  else
    [[ -e $dst ]] || return 0
    to=$userbak/${dst#"$HOME"/}
    [[ " ${kept[*]-} " == *" $userbak "* ]] || { install -d -m 700 "$userbak" && kept+=("$userbak"); } || return 1
    mkdir -p "${to%/*}" && cp -a "$dst" "$to"
  fi
}

failed=0
for entry in "${changed[@]}"; do
  IFS=$'\t' read -r src dst who <<<"$entry"
  if ! backup "$dst" "$who"; then
    echo "deploy: no backup of $dst; left as it is" >&2
    failed=$((failed + 1))
    continue
  fi
  # Keep the repo's mode (git tracks the executable bit).
  perm=$(stat -c %a "$src")
  if [[ $who == user || -n $sysroot ]]; then
    install -D -m "$perm" "$src" "$dst" || { failed=$((failed + 1)); continue; }
  else
    sudo install -D -o root -g root -m "$perm" "$src" "$dst" || { failed=$((failed + 1)); continue; }
  fi
  echo "installed $dst"
done
for d in "${kept[@]}"; do echo "deploy: what was there before is in $d"; done
((failed)) && { echo "deploy: $failed file(s) not installed." >&2; exit 1; }
exit 0
