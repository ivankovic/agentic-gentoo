#!/bin/bash
set -e
! grep -qx 'app-misc/jqq' /var/lib/portage/world || exit 1
grep -qx 'app-misc/jq' /var/lib/portage/world
emerge -puDN @world >/dev/null 2>&1
