#!/bin/bash
emerge -q "$PKG_ATOM" "$KEEP_ATOM" >/dev/null
# A variant where the kept package needs the other one cannot be done: not a drill.
[[ -z $(qdepends -qQ "$PKG_ATOM") ]]
