#!/bin/bash
# A big leftover fills the disk; the user's own large file must survive.
useradd -m "$U_NAME"
fallocate -l 1G "/home/$U_NAME/$MINE" && chown "$U_NAME:" "/home/$U_NAME/$MINE"
mkdir -p "${JUNK%/*}"
avail=$(df --output=avail -B1 / | tail -1)
fallocate -l $((avail - 200*1024*1024)) "$JUNK"
