#!/bin/bash
echo "$D_NAME.service runs as user $D_NAME, but /var/lib/$D_NAME/$SUB is owned by root with mode 700, so it cannot write to the $SUB directory (permission denied) and exits with status 3."
