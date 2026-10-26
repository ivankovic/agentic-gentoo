#!/bin/bash
emerge --deselect "$PKG_ATOM" && emerge --depclean "$PKG_ATOM"
