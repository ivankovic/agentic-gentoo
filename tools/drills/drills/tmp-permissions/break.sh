#!/bin/bash
useradd -m "$U_NAME"
chmod "$MODE" "$DIR"
# The user's programs use $DIR.
echo "export TMPDIR=$DIR" >> "/home/$U_NAME/.bash_profile"
