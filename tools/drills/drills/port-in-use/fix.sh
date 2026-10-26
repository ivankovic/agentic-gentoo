#!/bin/bash
pkill -f "http.server $SVC_PORT\$"; sleep 1
systemctl reset-failed "$SVC_NAME.service"; systemctl restart "$SVC_NAME.service"
