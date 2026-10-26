#!/bin/bash
sed -i "\|^net-misc/curl|!{\|^#|!d}" /etc/portage/package.use/local
echo "$PKG_ATOM -doc" >> /etc/portage/package.use/local
