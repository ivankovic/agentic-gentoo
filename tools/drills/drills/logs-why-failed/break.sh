#!/bin/bash
useradd -r -s /sbin/nologin ingest
mkdir -p /var/lib/ingest/spool && chown root:root /var/lib/ingest/spool && chmod 700 /var/lib/ingest/spool
cat > /usr/local/bin/ingest <<'X'
#!/bin/bash
echo "ingest: starting, spool /var/lib/ingest/spool"
touch /var/lib/ingest/spool/.lock || { echo "ingest: cannot write to spool directory" >&2; exit 3; }
X
chmod 755 /usr/local/bin/ingest
cat > /etc/systemd/system/ingest.service <<'X'
[Unit]
Description=Ingest queue

[Service]
User=ingest
ExecStart=/usr/local/bin/ingest
X
systemctl daemon-reload; systemctl start ingest.service || true
# What the check compares: nothing may change.
stat -c '%U %a' /var/lib/ingest/spool > /var/lib/.drill-ingest-state
sha256sum /etc/systemd/system/ingest.service /usr/local/bin/ingest >> /var/lib/.drill-ingest-state
