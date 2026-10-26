#!/bin/bash
h=/home/$U_NAME
chown -R "$U_NAME:$U_NAME" "$h/.ssh"
chmod 755 "$h" && chmod 700 "$h/.ssh" && chmod 600 "$h/.ssh/authorized_keys"
