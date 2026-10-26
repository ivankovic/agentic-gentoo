#!/bin/bash
# Installed, recorded in the world file (so depclean keeps it), and working.
set -e
qlist -I app-misc/jq | grep -q jq
grep -qx 'app-misc/jq' /var/lib/portage/world
echo '{"a":1}' | jq -e '.a == 1' >/dev/null
