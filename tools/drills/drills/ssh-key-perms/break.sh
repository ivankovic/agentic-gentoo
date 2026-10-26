#!/bin/bash
# No password, but not locked either: sshd refuses key logins to locked accounts.
useradd -m -p "*" "$U_NAME"
h=/home/$U_NAME
install -d -m 700 -o "$U_NAME" -g "$U_NAME" "$h/.ssh"
ssh-keygen -q -t ed25519 -N '' -f /var/lib/.drill-key
install -m 600 -o "$U_NAME" -g "$U_NAME" /var/lib/.drill-key.pub "$h/.ssh/authorized_keys"
case $KIND in
	home-777) chmod 777 "$h" ;;
	ssh-777) chmod 777 "$h/.ssh" ;;
	keys-666) chmod 666 "$h/.ssh/authorized_keys" ;;
	ssh-owner) chown -R root:root "$h/.ssh" ;;
esac
