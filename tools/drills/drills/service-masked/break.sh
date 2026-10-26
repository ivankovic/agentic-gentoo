#!/bin/bash
mkdir -p /srv/hello && echo hello > /srv/hello/index.html
mkdir -p /usr/lib/systemd/system
cat > /usr/lib/systemd/system/hello.service <<'X'
[Unit]
Description=Hello web page

[Service]
WorkingDirectory=/srv/hello
ExecStart=/usr/bin/python3 -m http.server 8080

[Install]
WantedBy=multi-user.target
X
systemctl daemon-reload; systemctl enable hello.service 2>/dev/null; systemctl mask hello.service 2>/dev/null
