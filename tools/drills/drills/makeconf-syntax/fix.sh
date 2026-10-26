#!/bin/bash
sed -i 's/^\(COMMON_FLAGS="[^"]*\)$/\1"/' /etc/portage/make.conf
