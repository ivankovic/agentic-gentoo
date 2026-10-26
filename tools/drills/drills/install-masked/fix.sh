#!/bin/bash
sed -i '/^sys-apps\/pv$/d' /etc/portage/package.mask/local
emerge sys-apps/pv
