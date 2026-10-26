#!/bin/bash
sed -i 's/^/# /' "/etc/sudoers.d/$FILE" && visudo -c
