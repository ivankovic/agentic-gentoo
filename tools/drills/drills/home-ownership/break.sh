#!/bin/bash
# Restored as root without keeping ownership.
useradd -m "$U_NAME"
h=/home/$U_NAME
mkdir -p "$h/Documents" "$h/.config/app"; echo notes > "$h/Documents/notes.txt"; echo x=1 > "$h/.config/app/rc"
chmod 600 "$h/.config/app/rc"
chown -R root:root "$h"
