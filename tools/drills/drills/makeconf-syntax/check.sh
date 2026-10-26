#!/bin/bash
set -e
emerge --info >/dev/null 2>&1
emerge -p app-misc/jq >/dev/null 2>&1
# The flags are the stage3's, not dropped.
emerge --info 2>/dev/null | grep -q '^CFLAGS=".*-O2'
