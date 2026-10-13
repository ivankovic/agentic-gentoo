# /etc/skel/.bashrc
#
# This file is sourced by all *interactive* bash shells on startup,
# including some apparently interactive shells such as scp and rcp
# that can't tolerate any output.  So make sure this doesn't display
# anything or bad things will happen !


# Test for an interactive shell.  There is no need to set anything
# past this point for scp and rcp, and it's important to refrain from
# outputting anything in those cases.
if [[ $- != *i* ]] ; then
	# Shell is non-interactive.  Be done now!
	return
fi


# Put your fun stuff here.
export PATH="$HOME/.local/bin:$HOME/go/bin:$PATH"
. "$HOME/.cargo/env"

# Prompt in the foot palette (ANSI colours, so the text consoles match too). Two lines:
#
#   ~/src/agentic-gentoo   main *+% ↑2 ↓1  took 4m12s  ✗ 130 INT
#   ❯
#
# Line one: the directory in linen; the git branch in sage, with * unstaged, + staged,
# % untracked, $ stashed, |REBASE and the like mid-operation, ↑/↓ commits ahead of and behind
# upstream; then, only when they apply, the duration of a command that ran 5 s or more, the exit
# status (with the signal for 129-159), background jobs, the venv, and user@host over SSH.
# Line two: ❯ in brass, oxblood after a failure, # as root; plain ASCII on the text console.
#
# For foot: OSC 7 reports the directory (Ctrl+Shift+N opens a terminal there), OSC 133 marks
# prompts and command output (Ctrl+Shift+Z/X jump between prompts, Ctrl+Shift+G copies the last
# output; see foot.ini), and a command that ran 30 s or more raises a notification when the
# window isn't focused. Everything that ends up in PS1 is read from variables, so a branch or
# directory name is never expanded as prompt code.
if [[ -r /usr/share/git/git-prompt.sh ]]; then
	. /usr/share/git/git-prompt.sh
	GIT_PS1_SHOWDIRTYSTATE=1
	GIT_PS1_SHOWUNTRACKEDFILES=1
	GIT_PS1_SHOWSTASHSTATE=1
	GIT_PS1_STATESEPARATOR=' '
fi
VIRTUAL_ENV_DISABLE_PROMPT=1

# Commands that are interactive by nature: no "finished" notification when they exit.
__prompt_quiet=' nvim vim vi less man ssh top htop btop claude tmux foot watch journalctl git '

# Start time and command line of the first command since the last prompt.
__prompt_preexec() {
	[[ -n ${__prompt_t0-} ]] && return
	__prompt_t0=$EPOCHREALTIME
	__prompt_cmd=$BASH_COMMAND
}
trap '__prompt_preexec' DEBUG

__prompt_duration() {
	local s=$1
	if ((s >= 3600)); then printf '%dh%02dm' $((s / 3600)) $((s % 3600 / 60))
	elif ((s >= 60)); then printf '%dm%02ds' $((s / 60)) $((s % 60))
	else printf '%ds' "$s"; fi
}

__prompt_urlencode() {
	local LC_ALL=C s=$1 out= c i # bytewise, so UTF-8 is encoded byte by byte
	for ((i = 0; i < ${#s}; i++)); do
		c=${s:i:1}
		case $c in
			[-/._~A-Za-z0-9]) out+=$c ;;
			*) printf -v c '%%%02X' "'$c"; out+=$c ;;
		esac
	done
	printf '%s' "$out"
}

__prompt() {
	local status=$? console=0
	[[ $TERM == linux ]] && console=1
	# \001 and \002 bracket the non-printing parts, as \[ \] would in PS1 itself.
	local o=$'\001' c=$'\002' e=$'\e'
	local reset=$o$e'[0m'$c

	# The command that just ran: duration, and a notification if it was long.
	local took= secs=0
	if [[ -n ${__prompt_t0-} ]]; then
		local now=$EPOCHREALTIME
		secs=$(( ${now%.*} - ${__prompt_t0%.*} ))
		((secs >= 5)) && took=$(__prompt_duration "$secs")
		local word=${__prompt_cmd%% *}
		if ((secs >= 30 && !console)) && [[ $__prompt_quiet != *" ${word##*/} "* ]]; then
			local verdict=finished
			((status)) && verdict="failed ($status)"
			printf '\e]777;notify;%s %s;%s\e\\' "${word##*/}" "$verdict" "${__prompt_cmd:0:120}, $took"
		fi
	fi
	unset __prompt_t0 __prompt_cmd

	local line=$o$e'[37m'$c${PWD/#$HOME/\~}

	if declare -F __git_ps1 >/dev/null; then
		local git
		git=$(__git_ps1 '%s')
		if [[ -n $git ]]; then
			local glyph='  ' # Nerd Font branch
			((console)) && glyph=' '
			local ab= counts
			if counts=$(git rev-list --count --left-right '@{upstream}...HEAD' 2>/dev/null); then
				local behind=${counts%%[[:space:]]*} ahead=${counts##*[[:space:]]}
				((ahead)) && ab+=" ↑$ahead"
				((behind)) && ab+=" ↓$behind"
				((console)) && { ab=${ab//↑/+}; ab=${ab//↓/-}; }
			fi
			line+=$o$e'[32m'$c$glyph$git$ab
		fi
	fi

	[[ -n $took ]] && line+="$o$e[90m$c  took $took"
	if ((status)); then
		local sig=
		((status > 128 && status < 160)) && sig=" $(kill -l $((status - 128)) 2>/dev/null)"
		local cross='✗'
		((console)) && cross='x'
		line+="$o$e[31m$c  $cross $status$sig"
	fi
	local jobs
	jobs=$(jobs -p | wc -l)
	((jobs)) && line+="$o$e[35m$c  $jobs job$( ((jobs > 1)) && echo s)"
	[[ -n ${VIRTUAL_ENV-} ]] && line+="$o$e[34m$c  venv ${VIRTUAL_ENV##*/}"
	[[ -n ${SSH_CONNECTION-} ]] && line+="$o$e[34m$c  $USER@${HOSTNAME%%.*}"

	local arrow='❯' mark=33
	((console)) && arrow='>'
	((status)) && mark=31
	((EUID == 0)) && { arrow='#'; mark=31; }

	# OSC 133;D ends the last command's output; 133;A starts this prompt; OSC 7 and the title
	# report the directory.
	local osc=
	((console)) || osc=$o$e']133;D'$e'\'$e']133;A'$e'\'$e"]7;file://$HOSTNAME$(__prompt_urlencode "$PWD")"$e'\'$e"]0;${PWD/#$HOME/\~}"$'\a'$c
	__prompt_line=$osc$'\n'$line$reset
	__prompt_arrow=$o$e"[${mark}m"$c$arrow$reset
	PS1='${__prompt_line}\n${__prompt_arrow} '
	# OSC 133;C: command output starts here.
	((console)) && PS0= || PS0='\e]133;C\e\\'
}
PROMPT_COMMAND=__prompt

# A terminal reopened after a crash (deck-terminals): its last command goes on ↑, and runs again
# at the first prompt when DECK_RESTORE_RUN is set (ssh, nvim, vim, claude --resume). At the first
# prompt, since bash reads the history file only after .bashrc.
if [[ -n ${DECK_RESTORE_CMD-} ]]; then
	__restore() {
		local cmd=$DECK_RESTORE_CMD run=${DECK_RESTORE_RUN-}
		unset DECK_RESTORE_CMD DECK_RESTORE_RUN
		PROMPT_COMMAND=__prompt
		history -s -- "$cmd"
		if [[ -n $run ]]; then
			printf '\e[90mreopened after a crash: %s\e[0m\n' "$cmd"
			eval "$cmd"
		else
			printf '\e[90mreopened after a crash; on ↑: %s\e[0m\n' "$cmd"
		fi
		unset __prompt_t0 __prompt_cmd
		__prompt
	}
	PROMPT_COMMAND=__restore
fi
