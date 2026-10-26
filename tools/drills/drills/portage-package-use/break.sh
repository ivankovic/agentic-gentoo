#!/bin/bash
mkdir -p /etc/portage/package.use
name=${PKG_ATOM#*/}
case $KIND in
	version-no-op) bad="$PKG_ATOM-1.0 -doc" ;;
	no-category) bad="$name -doc" ;;
	space) bad="${PKG_ATOM%/*} $name -doc" ;;
esac
cat > /etc/portage/package.use/local <<X
# mine
net-misc/curl -http2
$bad
X
emerge -p "$PKG_ATOM" 2>&1 | grep -qi 'invalid atom'
