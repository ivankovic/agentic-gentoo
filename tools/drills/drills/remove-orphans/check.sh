#!/bin/bash
set -e
! installed "$PKG_ATOM" || exit 1
installed "$KEEP_ATOM"
installed net-misc/openssh
installed app-admin/sudo
