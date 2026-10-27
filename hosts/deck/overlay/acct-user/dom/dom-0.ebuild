# Copyright 2026 Marko Ivankovic
# Distributed under the terms of the GNU General Public License v2

EAPI=8

inherit acct-user

DESCRIPTION="User for app-misc/dom"
ACCT_USER_ID=-1
ACCT_USER_GROUPS=( dom )
# dom.service's StateDirectory: the database (with device credentials in plain text) and the log.
ACCT_USER_HOME=/var/lib/dom
ACCT_USER_HOME_PERMS=0700
KEYWORDS="~amd64"

acct-user_add_deps
