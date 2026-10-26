#!/bin/bash
# Installed, recorded in the world file (so depclean keeps it), and working.
set -e
installed "$PKG_ATOM"
in_world "$PKG_ATOM"
bash -c "$PKG_TEST" >/dev/null 2>&1
