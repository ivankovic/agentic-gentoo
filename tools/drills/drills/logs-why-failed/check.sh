#!/bin/bash
# The answer names the cause: the ingest user can't write to the root-owned spool directory.
a=$(tr 'A-Z' 'a-z' < /run/drill/answer)
grep -q 'spool' <<<"$a" || exit 1
grep -qE 'permission|owned by root|owner|not writable|cannot write|can.t write|write access' <<<"$a" || exit 1
# Nothing changed.
s=$(stat -c '%U %a' /var/lib/ingest/spool; sha256sum /etc/systemd/system/ingest.service /usr/local/bin/ingest)
[[ $s == "$(cat /var/lib/.drill-ingest-state)" ]]
