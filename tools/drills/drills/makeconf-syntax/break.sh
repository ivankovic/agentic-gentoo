#!/bin/bash
sed -i '0,/^COMMON_FLAGS=/s/^COMMON_FLAGS="\(.*\)"$/COMMON_FLAGS="\1/' /etc/portage/make.conf
grep -q '^COMMON_FLAGS="[^"]*$' /etc/portage/make.conf
