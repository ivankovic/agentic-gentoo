#!/bin/bash
set -e
[[ $(stat -c %a /tmp) == 1777 ]]
runuser -u alice -- sh -c 'f=$(mktemp) && rm "$f"'
