#!/bin/bash
# The answer names the cause: the service's user can't write to its root-owned directory.
set -e
answer_has "$SUB" 'permission|owned by root|owner|not writable|cannot write|can.t write|write access'
# Nothing changed.
stat -c '%U %a' "/var/lib/$D_NAME/$SUB" > /var/lib/.drill-dirstate
unchanged files
