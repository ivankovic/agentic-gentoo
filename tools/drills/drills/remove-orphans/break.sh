#!/bin/bash
# tree was pulled in with --oneshot for a one-off job; jq is wanted.
emerge -q --oneshot app-text/tree >/dev/null && emerge -q app-misc/jq >/dev/null
