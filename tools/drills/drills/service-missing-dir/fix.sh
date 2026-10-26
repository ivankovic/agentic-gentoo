#!/bin/bash
sed -i 's|^WorkingDirectory=/srv/hello$|WorkingDirectory=/srv/www/hello|' /etc/systemd/system/hello.service
systemctl daemon-reload && systemctl restart hello.service
