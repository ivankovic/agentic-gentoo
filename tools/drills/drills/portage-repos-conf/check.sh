#!/bin/bash
set -e
[[ $(portageq get_repo_path / gentoo) == /var/db/repos/gentoo ]]
emerge -p app-misc/jq >/dev/null 2>&1
test -f /var/db/repos/gentoo/profiles/repo_name
