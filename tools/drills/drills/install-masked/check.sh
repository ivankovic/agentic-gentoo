#!/bin/bash
set -e
qlist -I sys-apps/pv | grep -q pv
grep -qx 'sys-apps/pv' /var/lib/portage/world
# The unrelated mask is still there.
grep -rqx '>=app-misc/screen-6' /etc/portage/package.mask
echo hi | pv -q >/dev/null
