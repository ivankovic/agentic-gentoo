#!/bin/bash
set -e
! grep -qxF "$(cat /var/lib/.drill-bad)" /var/lib/portage/world || exit 1
in_world "$PKG_ATOM"
emerge -puDN @world >/dev/null 2>&1
