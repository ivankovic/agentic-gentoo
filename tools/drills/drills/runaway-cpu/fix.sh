#!/bin/bash
systemctl disable --now "$D_NAME-reindex.timer" 2>/dev/null
systemctl disable --now "$D_NAME-reindex.service"
