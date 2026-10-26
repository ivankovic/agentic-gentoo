#!/bin/bash
mkdir -p /srv/hello && echo hello > /srv/hello/index.html
cat > /etc/systemd/system/hello.service <<'X'
[Unit]
Description=Hello web page

[Service]
WorkingDirectory=/srv/hello
ExecStart=/usr/bin/python3 -m http.server 8080

[Install]
WantedBy=multi-user.target
X
systemctl daemon-reload
