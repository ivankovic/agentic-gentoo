#!/bin/bash
sed -i "\|^$PKG_ATOM\$|d" "/etc/portage/package.mask/$FILE"
emerge "$PKG_ATOM"
