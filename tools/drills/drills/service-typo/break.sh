#!/bin/bash
mkdir -p /srv/hello && echo hello > /srv/hello/index.html
cat > /etc/systemd/system/hello.service <<'X'
[Unit]
Description=Hello web page

[Service]
WorkingDirectory=/srv/hello
ExecStart=/usr/bin/pyhton3 -m http.server 8080
Restart=on-failure

[Install]
WantedBy=multi-user.target
X
systemctl daemon-reload; systemctl enable hello.service 2>/dev/null; systemctl start hello.service || true
