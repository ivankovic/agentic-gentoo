# Shown on every interactive login (cleared first, so boot messages don't bury it), and by `help`.
# The host's name, network and mount point come from /etc/deck-recovery.conf (update.sh).
deck_help() {
	local name="Recovery" main=main net
	if [[ -r /etc/deck-recovery.conf ]]; then
		name=$(. /etc/deck-recovery.conf && echo "$RECOVERY_NAME")
		main=$(. /etc/deck-recovery.conf && echo "$MAIN_NAME")
	fi
	if [[ $(. /etc/deck-recovery.conf 2>/dev/null && echo "$NETWORK") == wired ]]; then
		net="Wired, by DHCP. Check:  ping -c1 gentoo.org     (networkctl shows the link)"
	else
		net="Saved Wi-Fi networks connect by themselves. Check:  ping -c1 gentoo.org
                  Otherwise:  iwctl station wlan0 connect <SSID>"
	fi
	cat <<TXT

  $name: a root shell, no desktop. The main system is untouched until you mount it.

  1. Network      $net

  2. Look         deck mount                  main system read-only at /mnt/$main
                  journalctl --root=/mnt/$main -b -1 -p warning              last boot's warnings
                  journalctl --root=/mnt/$main --namespace=vitals -b -1 -n 50  its last temperatures and power
                  deck mount check            offline btrfs check, read-only (when not mounted)

  3. Repair       deck rollback               root back to a snapshot (hourly, or from before an emerge)
                  deck mount off && deck mount rw
                  deck chroot                 a shell inside the main system: emerge, dracut, systemctl
                  deck mount off              when done, then: reboot

  4. Help         curl -fsSL https://claude.ai/install.sh | bash     (or just: install-claude)
                  export PATH=\$HOME/.local/bin:\$PATH; claude

  deck lists these tools; help prints this again; less /usr/local/share/RECOVERY.md has the rest.

TXT
}
alias help=deck_help
if [[ $- == *i* ]]; then
	clear
	deck_help
fi
