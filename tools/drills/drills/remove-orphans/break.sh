#!/bin/bash
# One was pulled in with --oneshot for a one-off job; the other is wanted.
emerge -q --oneshot "$PKG_ATOM" >/dev/null; emerge -q "$KEEP_ATOM" >/dev/null
# A variant where the kept package needs the other one cannot be done: not a drill.
[[ -z $(qdepends -qQ "$PKG_ATOM") ]]
