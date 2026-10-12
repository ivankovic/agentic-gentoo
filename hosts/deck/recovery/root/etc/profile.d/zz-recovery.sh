# Shown on every interactive login (cleared first, so boot messages don't bury it), and by `help`.
deck_help() {
	cat <<'TXT'

  Deck recovery: a root shell, no desktop. The main system is untouched until you mount it.

  1. Network      Saved Wi-Fi networks connect by themselves. Check:  ping -c1 gentoo.org
                  Otherwise:  iwctl station wlan0 connect <SSID>

  2. Look         deck-mount                  main system read-only at /mnt/deck
                  journalctl --root=/mnt/deck -b -1 -p warning              last boot's warnings
                  journalctl --root=/mnt/deck --namespace=vitals -b -1 -n 50  its last temperatures and power

  3. Repair       deck-mount off && deck-mount rw
                  deck-chroot                 a shell inside the main system: emerge, dracut, systemctl
                  deck-mount off              when done, then: reboot

  4. Help         curl -fsSL https://claude.ai/install.sh | bash     (or just: install-claude)
                  export PATH=$HOME/.local/bin:$PATH; claude

  help prints this again; less /usr/local/share/RECOVERY.md has the rest.

TXT
}
alias help=deck_help
if [[ $- == *i* ]]; then
	clear
	deck_help
fi
