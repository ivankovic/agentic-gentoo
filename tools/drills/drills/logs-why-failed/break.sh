#!/bin/bash
useradd -r -s /sbin/nologin "$D_NAME"
dir=/var/lib/$D_NAME/$SUB
mkdir -p "$dir"; chown root:root "$dir"; chmod 700 "$dir"
cat > "/usr/local/bin/$D_NAME" <<X
#!/bin/bash
echo "$D_NAME: starting, $SUB directory $dir"
touch $dir/.lock || { echo "$D_NAME: cannot write to $SUB directory" >&2; exit 3; }
X
chmod 755 "/usr/local/bin/$D_NAME"
cat > "/etc/systemd/system/$D_NAME.service" <<X
[Unit]
Description=$D_DESC

[Service]
User=$D_NAME
ExecStart=/usr/local/bin/$D_NAME
X
systemctl daemon-reload; systemctl start "$D_NAME.service" || true
# What the check compares: nothing may change.
stat -c '%U %a' "$dir" > /var/lib/.drill-dirstate
remember files "/etc/systemd/system/$D_NAME.service" "/usr/local/bin/$D_NAME" /var/lib/.drill-dirstate
