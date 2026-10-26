#!/bin/bash
set -e
! emerge -p "$PKG_ATOM" 2>&1 | grep -qi 'invalid atom' || exit 1
grep -rqx 'net-misc/curl -http2' /etc/portage/package.use
