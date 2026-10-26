#!/bin/bash
echo "ingest.service runs as user ingest, but /var/lib/ingest/spool is owned by root with mode 700, so it cannot write to the spool directory (permission denied) and exits with status 3."
