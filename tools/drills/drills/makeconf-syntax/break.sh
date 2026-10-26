#!/bin/bash
f=/etc/portage/make.conf
case $KIND in
	common-quote) sed -i '0,/^COMMON_FLAGS=/s/^COMMON_FLAGS="\(.*\)"$/COMMON_FLAGS="\1/' $f ;;
	makeopts-quote) printf '\n# more jobs\nMAKEOPTS="-j2 -l2\n' >> $f ;;
	use-quote) printf '\nUSE="-doc -test\n' >> $f ;;
esac
! emerge --info >/dev/null 2>&1
