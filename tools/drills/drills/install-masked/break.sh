#!/bin/bash
# Someone masked it by hand a while ago, among other entries that must stay.
mkdir -p /etc/portage/package.mask
cat > /etc/portage/package.mask/local <<'X'
# keep the old screen
>=app-misc/screen-6
# testing, remove later
sys-apps/pv
X
true
