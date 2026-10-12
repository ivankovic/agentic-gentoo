# Shown on every interactive login.
if [[ $- == *i* ]]; then
	cat <<'TXT'

  Deck recovery. The main system is untouched until you mount it.

    iwctl station wlan0 connect <SSID>   Wi-Fi (saved networks connect by themselves)
    deck-mount                           open and mount deck read-only at /mnt/deck
    deck-mount rw && deck-chroot         repair it from inside
    install-claude                       Claude Code, if you want help
    less /usr/local/share/RECOVERY.md    what this is and how it's kept up to date

TXT
fi
