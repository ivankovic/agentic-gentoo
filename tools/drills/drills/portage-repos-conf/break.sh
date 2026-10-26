#!/bin/bash
case $KIND in
	old-path) loc=/var/db/repos/gentoo-old ;;
	typo) loc=/var/db/repo/gentoo ;;
	usr-portage) loc=/usr/portage ;;
esac
mkdir -p /etc/portage/repos.conf
cat > /etc/portage/repos.conf/gentoo.conf <<X
[DEFAULT]
main-repo = gentoo

[gentoo]
location = $loc
sync-type = rsync
sync-uri = rsync://rsync.gentoo.org/gentoo-portage
auto-sync = yes
X
! emerge -p app-misc/jq >/dev/null 2>&1
