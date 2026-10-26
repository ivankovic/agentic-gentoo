#!/bin/bash
# The page moved to /srv/www/hello; the unit still points at the old place.
mkdir -p /srv/www/hello && echo hello > /srv/www/hello/index.html
cat > /etc/systemd/system/hello.service <<'X'
[Unit]
Description=Hello web page

[Service]
WorkingDirectory=/srv/hello
ExecStart=/usr/bin/python3 -m http.server 8080

[Install]
WantedBy=multi-user.target
X
systemctl daemon-reload; systemctl enable hello.service 2>/dev/null; systemctl start hello.service || true
