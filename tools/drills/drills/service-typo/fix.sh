#!/bin/bash
sed -i 's|/usr/bin/pyhton3|/usr/bin/python3|' /etc/systemd/system/hello.service
# It restarted until systemd gave up; the counter has to be cleared too.
systemctl reset-failed hello.service
systemctl daemon-reload && systemctl restart hello.service
