#!/bin/bash
set -e
! qlist -I app-misc/screen | grep -q . || exit 1
! grep -qx 'app-misc/screen' /var/lib/portage/world || exit 1
# Nothing else the user asked for went with it.
qlist -I app-misc/jq | grep -q jq
grep -qx 'app-misc/jq' /var/lib/portage/world
