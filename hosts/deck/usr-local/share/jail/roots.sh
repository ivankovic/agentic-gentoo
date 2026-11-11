# Work roots, for jail and the host git guard (/usr/local/bin/jail, /usr/local/bin/git), which both
# source this file so that they can't disagree: ~/src, and the directories ~/.config/jail/roots
# lists (one per line, relative to the home or absolute, inside the home), treated the same:
# writable in the jail, their repositories protected, host git guarded there. The jail never sees
# ~/.config, so it can't add one.
#
# Sets the array `roots` and defines in_roots. Bash, sourced; needs HOME.
roots=("$HOME/src")
if [[ -r ${XDG_CONFIG_HOME:-$HOME/.config}/jail/roots ]]; then
	while read -r r; do
		[[ -z $r || $r == '#'* ]] && continue
		[[ $r == /* ]] || r=$HOME/$r
		r=$(realpath -m -- "$r")
		[[ -d $r && $r/ == "$HOME"/?* && $r != "$HOME" ]] && roots+=("$r")
	done <"${XDG_CONFIG_HOME:-$HOME/.config}/jail/roots"
fi
in_roots() { # PATH: inside one of the roots? Prints which.
	local r
	for r in "${roots[@]}"; do [[ $1/ == "$r"/* ]] && { printf '%s' "$r"; return 0; }; done
	return 1
}
