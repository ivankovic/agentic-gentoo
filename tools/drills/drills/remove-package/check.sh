#!/bin/bash
set -e
! installed "$PKG_ATOM" || exit 1
! in_world "$PKG_ATOM" || exit 1
# Nothing else the user asked for went with it.
installed "$KEEP_ATOM"
in_world "$KEEP_ATOM"
