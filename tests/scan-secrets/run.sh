#!/usr/bin/env bash
# Tests for .githooks/scan-secrets and .githooks/pre-push: tests/scan-secrets/run.sh
#
# Every case runs in a throwaway repository under $TMPDIR. The strings that must be refused are
# put together here at run time, so that this file holds nothing a scanner would stop on; the
# last case checks exactly that, by scanning the hooks and this file. gitleaks is kept off the
# PATH: what is tested is the script's own rules. Exits 1 if any case fails.
set -uo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
hooks=$(cd "$here/../../.githooks" && pwd)
scan=$hooks/scan-secrets
work=$(mktemp -d "${TMPDIR:-/tmp}/scan-secrets-test.XXXXXX") || exit 1
trap 'rm -rf "$work"' EXIT

# Only the tools the hooks need, and git itself: /usr/bin/git when it is there, since a wrapper
# earlier in the PATH may not want to run in a scratch repository.
mkdir "$work/bin"
for tool in bash env grep sed awk tr head od wc cut sort iconv mktemp rm cat cp mkdir dirname ln; do
  ln -s "$(command -v "$tool")" "$work/bin/$tool" || exit 1
done
if [[ -x /usr/bin/git ]]; then ln -s /usr/bin/git "$work/bin/git"; else ln -s "$(command -v git)" "$work/bin/git"; fi
export PATH=$work/bin HOME=$work TMPDIR=$work
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
unset SECRET_SCAN_BYPASS
export PRE_PUSH_TESTS=0 # pre-push would run every test, this one too

# Made here, never written out whole.
pw="Tr0ub4d""or-x9"
long="q7Zr2m""Kx81Lw""c4Tn9v"
tk="gh""p_$(printf 'a1B2%.0s' 1 2 3 4 5 6 7 8 9)"
aws="AK""IA$(printf 'Q7%.0s' 1 2 3 4 5 6 7 8)"
keyblock="-----BEGIN OPENSSH PRIVATE"" KEY-----"
private="zzyzx-private@example.invalid"
unknown=$(printf '1%.0s' {1..40})

failed=0 count=0 repos=0 repo=
new_repo() {
  repo=$work/repo$((++repos))
  git init -q -b main "$repo"
  printf '# a comment\n%s\n' "$private" >"$repo/.git/info/scan-secrets-deny"
}
commit() { git -C "$repo" commit -q "$@"; }

# t block|pass|refuse DESCRIPTION RULE COMMAND...: run COMMAND in $repo and compare. A block is
# exit 1 naming RULE; a refusal is exit 1 saying that the scan could not be done.
t() {
  local want=$1 desc=$2 rule=$3 rc good=0
  shift 3
  (cd "$repo" && "$@") >"$work/out" 2>&1
  rc=$?
  case $want in
    pass) ((rc == 0)) && good=1 ;;
    block) ((rc == 1)) && grep -qF -e "$rule" "$work/out" && ! grep -qF 'could not be completed' "$work/out" && good=1 ;;
    refuse) ((rc == 1)) && grep -qF 'could not be completed' "$work/out" && good=1 ;;
  esac
  count=$((count + 1))
  if ((!good)); then
    failed=$((failed + 1))
    echo "FAIL ($want expected, exit $rc): $desc"
    sed 's/^/    /' "$work/out"
  fi
}

# stage PATH: make the index hold only PATH, with stdin as its content.
stage() {
  git -C "$repo" read-tree --empty
  mkdir -p "$repo/$(dirname "$1")"
  cat >"$repo/$1"
  git -C "$repo" add -f -- "$1"
}
staged_line() { # block|pass RULE LINE
  printf '%s\n' "$3" | stage notes.txt
  t "$1" "staged line: ${3:0:40}" "$2" "$scan" --staged
}

# --- content: must be refused -------------------------------------------------------------
new_repo
a=credential-assignment
staged_line block "$a" "PASSWORD=$pw"
staged_line block "$a" "API_KEY=$long"
staged_line block "$a" "export GITHUB_TOKEN=$long"
staged_line block "$a" "Password: $pw"
staged_line block "$a" '"password": "'"$pw"'",'
staged_line block "$a" 'db_password = "'"$pw"'"'
staged_line block "$a" "client_secret: $long"
staged_line block "$a" "AWS_SECRET_ACCESS_KEY='$long'"
staged_line block "$a" "  wifi-passphrase:  \"$pw\""
staged_line block "$a" "psk=$pw"
staged_line block "$a" "user=me PASSWORD=$pw TOKEN=$long"
staged_line block url-with-password "url = https://deploy:$pw@example.org/repo.git"
staged_line block authorization-header "authorization: bearer $long"
staged_line block github-token "see $tk"
staged_line block aws-key-id "id $aws"
staged_line block private-key-block "$keyblock"
staged_line block 'private string' "mail ${private^^} about it"

# --- content: must pass -------------------------------------------------------------------
staged_line pass - 'PASSWORD='
staged_line pass - 'PASSWORD=""'
staged_line pass - 'password: changeme'
staged_line pass - 'password=<your password>'
staged_line pass - 'PASSWORD=${DB_PASSWORD}'
staged_line pass - 'export API_TOKEN=$(pass show api/token)'
staged_line pass - 'api_key = "xxxxxxxxxxxxxxxxxxxx"'
staged_line pass - 'password: …'
staged_line pass - 'password = "..."'
staged_line pass - 'password: ********'
staged_line pass - 'db_password = "{{ vault_db_password }}"'
staged_line pass - 'password_file = "/etc/app/password"'
staged_line pass - 'password_prompt = "Enter the password for this host"'
staged_line pass - 'PASSWORD_STORE_GPG_OPTS="--pinentry-mode=error"'
staged_line pass - 'let token = self.next_token();'
staged_line pass - 'password = args.password'
staged_line pass - 'pub password: Option<String>,'
staged_line pass - 'sort_key = "name_then_version_then_slot"'
staged_line pass - 'cache_key: linux-x86_64-cargo-0123456789'
staged_line pass - 'PORTAGE_GPG_KEY="0x0123456789''ABCDEF0123"'
staged_line pass - '# set a password: see the manual'
staged_line pass - 'PasswordAuthentication no'
staged_line pass - 'KEYWORDS="~amd64 ~arm64"'
staged_line pass - 'DIST foo-1.0.tar.gz 1234 BLAKE2B 0123456789abcdef0123456789abcdef SHA512 fedcba9876543210fedcba9876543210'
staged_line pass - 'https://user:password@example.org/'
staged_line pass - 'https://$USER:$PASS@example.org/'
staged_line pass - 'https://example.org:8080/a@b'
staged_line pass - 'ssh://git@example.org:22/repo.git'

# --- binary and UTF-16 files --------------------------------------------------------------
printf 'PASSWORD=%s\n' "$pw" | iconv -f UTF-8 -t UTF-16 | stage notes.txt
t block "UTF-16 text is decoded and scanned" "$a" "$scan" --staged
printf 'nothing to see\n' | iconv -f UTF-8 -t UTF-16 | stage notes.txt
t pass "UTF-16 text without secrets" - "$scan" --staged
printf 'see %s\n' "$tk" | iconv -f UTF-8 -t UTF-16LE | stage notes.txt
t block "UTF-16 without a byte order mark: token" github-token "$scan" --staged
printf '\0\1\2 %s \0\n' "$tk" | stage blob.bin
t block "binary file: token" github-token "$scan" --staged
printf '\0\1\2 %s \0\n' "$private" | stage blob.bin
t block "binary file: private string" 'private string' "$scan" --staged
printf '\0\1\2 nothing \0\n' | stage blob.bin
t pass "binary file without secrets" - "$scan" --staged
printf 'x\n' | stage keys/id_ed25519.bin.key
t block "path rules apply whatever the content" 'denied path' "$scan" --staged

# --- paths --------------------------------------------------------------------------------
for path in .claude/settings.local.json home/.claude/settings.local.json .aws/credentials \
  etc/shadow backup/shadow- authorized_keys conf/rclone.conf .env home/.ssh/config id_ed25519 \
  nvim/shada/main.shada .password-store/mail; do
  printf 'x\n' | stage "$path"
  t block "path $path" 'denied path' "$scan" --staged
done
printf 'x\n' | stage "notes/${private^^}.txt"
t block "private string in a file name" 'private string' "$scan" --staged
for path in id_ed25519.pub README.md src/shadow.rs env.example docs/keys.md; do
  printf 'x\n' | stage "$path"
  t pass "path $path" - "$scan" --staged
done

# --- commit messages ----------------------------------------------------------------------
printf 'fix login\n\nPASSWORD=%s\n' "$pw" >"$work/msg"
t block "commit message with an assignment" "$a" "$scan" --message "$work/msg"
printf 'fix login\n\nreported by %s\n' "$private" >"$work/msg"
t block "commit message with a private string" 'private string' "$scan" --message "$work/msg"
printf 'fix login\n\nthe password: prompt was shown twice\n' >"$work/msg"
t pass "plain commit message" - "$scan" --message "$work/msg"

# --- ranges -------------------------------------------------------------------------------
new_repo
printf 'one\n' | stage a.txt
commit -m 'first'
base=$(git -C "$repo" rev-parse HEAD)
printf 'one\ntwo\n' | stage a.txt
commit -m 'second'
t pass "clean repository: range" - "$scan" --range "$base..HEAD"
t pass "clean repository: every commit" - "$scan" --range HEAD
t pass "clean repository: staged" - "$scan" --staged
t pass "empty range" - "$scan" --range HEAD..HEAD
t refuse "range git cannot resolve" - "$scan" --range "$unknown..HEAD"
t refuse "range git cannot resolve, with the bypass" - env SECRET_SCAN_BYPASS=1 "$scan" --range "$unknown..HEAD"

# Lines whose own text begins with +: ++ and +++ in the diff.
new_repo
printf 'one\n' | stage a.txt
commit -m 'first'
printf 'one\n+PASSWORD=%s\n' "$pw" | stage a.txt
commit -m 'second'
t block "added line beginning with +" "$a" "$scan" --range HEAD~1..HEAD
printf 'one\n++ PASSWORD=%s\n' "$pw" | stage a.txt
commit -m 'third'
t block "added line beginning with '++ '" "$a" "$scan" --range HEAD~1..HEAD
printf 'one\n' | stage a.txt
commit -m 'fourth: the line is gone again'
t pass "removed lines are not scanned" - "$scan" --range HEAD~1..HEAD
t block "the whole history still has it" "$a" "$scan" --range HEAD

# A merge that brings in a secret neither parent has.
new_repo
printf 'one\n' | stage a.txt
commit -m 'first'
base=$(git -C "$repo" rev-parse HEAD)
git -C "$repo" checkout -q -b side
printf 'side\n' >"$repo/b.txt"
git -C "$repo" add b.txt
commit -m 'side'
git -C "$repo" checkout -q main
printf 'main\n' >"$repo/c.txt"
git -C "$repo" add c.txt
commit -m 'main'
git -C "$repo" merge -q --no-ff --no-commit side >/dev/null 2>&1
printf 'API_KEY=%s\n' "$long" >"$repo/d.txt"
git -C "$repo" add d.txt
commit -m 'merge side'
t block "secret introduced by a merge commit" "$a" "$scan" --range "$base..HEAD"
t block "the hit names the file and line" "d.txt:1: $a" "$scan" --range "$base..HEAD"

# Binary file, denied path and message in commits.
new_repo
printf 'one\n' | stage a.txt
commit -m 'first'
printf '\0\1 %s \0\n' "$tk" | stage blob.bin
commit -m 'second'
t block "binary file in a commit: token" github-token "$scan" --range HEAD~1..HEAD
printf 'x\n' | stage .claude/settings.local.json
commit -m 'third'
t block "denied path in a commit" 'denied path' "$scan" --range HEAD~1..HEAD
printf 'x\n' | stage a.txt
commit -m "fourth: PASSWORD=$pw"
t block "secret in a commit message" "$a" "$scan" --range HEAD~1..HEAD

# --- pre-push -----------------------------------------------------------------------------
# The remote's commit is not in this clone: every commit not on a remote-tracking branch is
# scanned instead.
new_repo
cp -r "$hooks" "$repo/.githooks"
printf 'one\n' | stage a.txt
commit -m 'first'
head=$(git -C "$repo" rev-parse HEAD)
zero=${unknown//1/0}
push() { printf '%s\n' "$1" | .githooks/pre-push origin https://example.invalid/repo.git; }
t pass "pre-push: unknown remote commit, clean history" - push "refs/heads/main $head refs/heads/main $unknown"
t pass "pre-push: new ref, clean history" - push "refs/heads/main $head refs/heads/main $zero"
t pass "pre-push: deleting a ref" - push "(delete) $zero refs/heads/main $head"
printf 'TOKEN=%s\n' "$long" | stage a.txt
commit -m 'second'
new=$(git -C "$repo" rev-parse HEAD)
t block "pre-push: unknown remote commit, secret in history" "$a" push "refs/heads/main $new refs/heads/main $unknown"
t block "pre-push: new ref, secret in history" "$a" push "refs/heads/main $new refs/heads/main $zero"
t block "pre-push: known remote commit" "$a" push "refs/heads/main $new refs/heads/main $head"
git -C "$repo" update-ref refs/remotes/origin/main "$new"
t pass "pre-push: commits already on a remote are not scanned again" - push "refs/heads/main $new refs/heads/main $unknown"

# --- private details: people, networks, pictures, authors ---------------------------------
new_repo
at="@"
staged_line block mail-address "write to jane.doe${at}mailbox.org today"
staged_line pass - "write to jane${at}example.org or run journald${at}vitals.conf"
staged_line pass - "the committer is test${at}example.invalid"
staged_line block private-network-address "server 10"".20.30.40 port 22"
staged_line block private-network-address "gateway 172"".16.0.1"
staged_line block private-network-address "peer 100"".64.0.7"
staged_line block private-network-address "address fd""12:3456:789a::1"
staged_line pass - 'listen 127.0.0.1:8080 and 8.8.8.8, version 10.2.1'
staged_line block hardware-address "ether 3c:""52:82:aa:bb:cc"
staged_line block ssh-public-key "ssh-ed25519 AAAA""C3NzaC1lZDI1NTE5AAAAIPk65CwT9a7hxA9dPLMbogaSAdEv me"
staged_line block bank-account "pay to CH93"" 0076 2011 6238 5295 7"
staged_line block phone-number "call +41"" 44 668 18 00"
staged_line block coordinates "at 47.37""689, 8.54""169"
staged_line pass - 'wlsunset -l 47.4 -L 8.5, sha 1234 5678, +2 lines, version 2.18'
staged_line block json-web-token "eyJ""hbGciOiJIUzI1NiJ9.eyJ""zdWIiOiIxMjM0NTY3ODkwIn0.abcdefghij"
staged_line block app-password "nextcloud: Ab3dE""-fG5hJ-kL7mN-pQ9rS-tU2vW"
staged_line block otp-secret "otpauth""://totp/x?sec""ret=JBSWY3DPEHPK3PXP"
for path in hosts/core/etc/fstab state/usage.db notes/session.jsonl build.log .claude/todo.md \
  doc/invoice.pdf src/main.rs.orig mail/a.eml; do
  printf 'x\n' | stage "$path"
  t block "path: $path" 'denied path' "$scan" --staged
done
printf 'x\n' | stage hosts/deck/etc/fstab
t pass "path: hosts/deck/etc/fstab" - "$scan" --staged

# A link out of the repository, and one inside it.
git -C "$repo" read-tree --empty
ln -s /home/someone/notes "$repo/out" && git -C "$repo" add -f out
t block "a link to an absolute path" link-out "$scan" --staged
git -C "$repo" read-tree --empty
ln -s notes.txt "$repo/in" && git -C "$repo" add -f in
t pass "a link inside the repository" - "$scan" --staged

# A file too large to be source.
head -c $((6 * 1024 * 1024)) /dev/zero | tr '\0' 'a' | stage big.txt
t block "a 6 MiB file" large-file "$scan" --staged

# Pictures: plain ones pass, one with a text field or a position does not. Needs Pillow.
if python=$(PATH=/usr/bin:/bin:/usr/local/bin command -v python3) && "$python" -c 'import PIL' 2>/dev/null; then
  ln -sf "$python" "$work/bin/python3"
  "$python" - "$work" <<'PY'
import sys
from PIL import Image, PngImagePlugin
d = sys.argv[1]
Image.new("RGB", (4, 4)).save(d + "/plain.png")
info = PngImagePlugin.PngInfo(); info.add_text("Author", "someone")
Image.new("RGB", (4, 4)).save(d + "/text.png", pnginfo=info)
im = Image.new("RGB", (4, 4)); exif = im.getexif(); exif[0x8825] = {1: "N", 2: (47.0, 22.0, 0.0)}
im.save(d + "/gps.jpg", exif=exif)
PY
  stage plain.png <"$work/plain.png"
  t pass "a picture with nothing but the picture" - "$scan" --staged
  stage text.png <"$work/text.png"
  t block "a picture with a text field" picture-metadata "$scan" --staged
  stage gps.jpg <"$work/gps.jpg"
  t block "a picture with a position" 'GPS position' "$scan" --staged
  rm "$work/bin/python3"
  stage plain.png <"$work/plain.png"
  t block "a picture that cannot be inspected" 'could not be read' "$scan" --staged
else
  echo "scan-secrets tests: no python3 with Pillow here: the picture cases are skipped"
fi

# An author address no remote has seen.
new_repo
printf 'one\n' | stage a.txt
commit -m 'first'
git -C "$repo" update-ref refs/remotes/origin/main HEAD
printf 'two\n' | stage a.txt
t pass "the author the remote knows" - "$scan" --staged
other() { GIT_AUTHOR_EMAIL="other${at}example.invalid" GIT_COMMITTER_EMAIL="other${at}example.invalid" "$@"; }
t block "an author no remote knows, at commit" new-author-address other "$scan" --staged
other git -C "$repo" commit -q -m 'second'
t block "an author no remote knows, at push" new-author-address "$scan" --range origin/main..HEAD
t block "a private string in the author's address" 'private string' env GIT_AUTHOR_EMAIL="$private" \
  bash -c 'printf "three\n" >a.txt; git add a.txt; git commit -q -m third; "$0" --range HEAD~1..HEAD' "$scan"

# --- the hooks and this file hold nothing the scanner stops on ----------------------------
new_repo
mkdir -p "$repo/.githooks" "$repo/tests/scan-secrets"
cp "$hooks"/* "$repo/.githooks/"
cp "$here"/* "$repo/tests/scan-secrets/"
rm "$repo/.git/info/scan-secrets-deny"
git -C "$repo" add .githooks tests
t pass "the hooks and their tests pass the scan themselves" - "$scan" --staged

echo "scan-secrets tests: $((count - failed)) of $count passed"
((failed == 0))
