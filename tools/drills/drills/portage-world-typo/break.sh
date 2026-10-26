#!/bin/bash
emerge -q "$PKG_ATOM" >/dev/null
case $KIND in
	double-last) bad=$PKG_ATOM${PKG_ATOM: -1} ;;
	category) bad=${PKG_ATOM/-/s-} ;;
esac
echo "$bad" >> /var/lib/portage/world
echo "$bad" > /var/lib/.drill-bad
