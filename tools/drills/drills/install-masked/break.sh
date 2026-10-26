#!/bin/bash
# Someone masked it by hand a while ago, among other entries that must stay.
mkdir -p /etc/portage/package.mask
cat > "/etc/portage/package.mask/$FILE" <<X
# keep this one away for now
$KEEP_ATOM
# testing, remove later
$PKG_ATOM
X
