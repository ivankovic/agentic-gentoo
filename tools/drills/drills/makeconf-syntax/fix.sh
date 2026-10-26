#!/bin/bash
f=/etc/portage/make.conf
case $KIND in
	common-quote) sed -i 's/^\(COMMON_FLAGS="[^"]*\)$/\1"/' $f ;;
	makeopts-quote) sed -i 's/^\(MAKEOPTS="[^"]*\)$/\1"/' $f ;;
	use-quote) sed -i 's/^\(USE="[^"]*\)$/\1"/' $f ;;
esac
