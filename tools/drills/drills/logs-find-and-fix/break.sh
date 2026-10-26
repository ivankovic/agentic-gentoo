#!/bin/bash
cat > /etc/worker.conf <<'X'
# worker settings
queue = /var/lib/worker
threads = many
X
mkdir -p /var/lib/worker
cat > /usr/local/bin/worker <<'X'
#!/bin/bash
t=$(sed -n 's/^threads *= *//p' /etc/worker.conf)
[[ $t =~ ^[0-9]+$ ]] || { echo "worker: /etc/worker.conf: invalid value for threads: '$t' (expected a number)" >&2; exit 1; }
echo "worker: running with $t threads"; exec sleep infinity
X
chmod 755 /usr/local/bin/worker
cat > /etc/systemd/system/worker.service <<'X'
[Unit]
Description=Queue worker

[Service]
ExecStart=/usr/local/bin/worker
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
X
systemctl daemon-reload; systemctl enable --now worker.service 2>/dev/null || true
