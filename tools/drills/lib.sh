# Shared by the drills' scripts: the runner puts it into each one after its #! line.
# break.sh runs under bash -e: a command there that may fail says so with `|| true`.

# web_unit FILE DESC DIR PORT [PYTHON [EXTRA]]: a unit serving DIR with python3's http.server.
web_unit() {
	mkdir -p "${1%/*}"
	cat > "$1" <<X
[Unit]
Description=$2

[Service]
WorkingDirectory=$3
ExecStart=${5:-/usr/bin/python3} -m http.server $4
${6:-}

[Install]
WantedBy=multi-user.target
X
}
# web_root DIR TEXT: the page a web service serves.
web_root() { mkdir -p "$1" && echo "$2" > "$1/index.html"; }
# serves PORT TEXT: the page on PORT reads TEXT within 10 seconds.
serves() {
	local i
	for i in $(seq 10); do [[ $(curl -sf "http://127.0.0.1:$1/") == "$2" ]] && return 0; sleep 1; done
	return 1
}
installed() { qlist -I "$1" | grep -q .; }
in_world() { grep -qx "$1" /var/lib/portage/world; }
# remember NAME FILE...: record the files' contents; unchanged NAME: they still match.
# The runner takes /var/lib/.drill-* out of the machine after break.sh and puts its own copy back
# for check.sh, so a repair neither sees which files are compared nor can change the record.
remember() { local n=$1; shift; sha256sum "$@" > "/var/lib/.drill-$n"; }
unchanged() { sha256sum -c --quiet "/var/lib/.drill-$1" >/dev/null 2>&1; }
# answer_has REGEX...: the final answer matches each (case-insensitive extended regex).
answer_has() { local r; for r; do grep -qiE "$r" /run/drill/answer || return 1; done; }
