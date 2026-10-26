#!/bin/bash
sed -i 's|^location = .*|location = /var/db/repos/gentoo|' /etc/portage/repos.conf/gentoo.conf
