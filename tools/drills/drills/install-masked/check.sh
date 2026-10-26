#!/bin/bash
set -e
installed "$PKG_ATOM"
in_world "$PKG_ATOM"
bash -c "$PKG_TEST" >/dev/null 2>&1
# The unrelated mask is still in force.
grep -rqx "$KEEP_ATOM" /etc/portage/package.mask
! emerge -p "$KEEP_ATOM" >/dev/null 2>&1 || exit 1
