#!/bin/bash
groupadd "$GRP"
useradd -m "$U_NAME"; useradd -m -G "$GRP" "$MATE_NAME"
install -d -m 2770 -o root -g "$GRP" "/srv/$GRP"
echo plan > "/srv/$GRP/plan.txt"; chgrp "$GRP" "/srv/$GRP/plan.txt"; chmod 660 "/srv/$GRP/plan.txt"
