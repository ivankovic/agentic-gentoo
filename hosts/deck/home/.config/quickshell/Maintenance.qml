pragma Singleton

import QtQuick
import Quickshell

// The full maintenance report in a terminal, from the status bar's popup ("Full report") or the
// omni panel's search ("Maintenance details"). What the bar shows comes from deck-status
// (Status.qml); this adds the logs: security advisories (the daily sync's list,
// /var/lib/gentoo-security/affected, not glsa-check, which takes seconds), firmware, failed units
// with their journal, deck-attention's details, disk health, the recovery and the servers.
// deck-attention --details marks crashes and an unclean shutdown as seen.
Singleton {
    function show() {
        Session.launch(["foot", "-T", "maintenance", "sh", "-c", `
            echo '# Security advisories'; cat /var/lib/gentoo-security/affected 2>/dev/null || echo 'not checked yet'
            echo; echo '# Firmware'; fwupdmgr get-updates --no-unreported-check 2>&1
            echo; echo '# Failed units'; systemctl --failed --no-pager; systemctl --user --failed --no-pager
            for u in $(systemctl --failed --no-legend --plain | cut -d' ' -f1); do
                echo; journalctl -u "$u" -n 15 --no-pager
            done
            for u in $(systemctl --user --failed --no-legend --plain | cut -d' ' -f1); do
                echo; journalctl --user -u "$u" -n 15 --no-pager
            done
            echo; echo '# Needs a look'; deck-attention --details
            echo; echo '# Disk health'; cat /var/lib/deck-health/report 2>/dev/null || echo 'not checked yet'
            if [ -s /var/lib/deck-health/unclean ]; then
                echo "unclean shutdown noticed $(cat /var/lib/deck-health/unclean); last minutes before it:"
                echo '  journalctl --namespace=vitals -b -1 -n 50'
                systemctl status btrfs-scrub.service --no-pager -n 3 | sed -n '1,3p'
            fi
            echo; echo '# Recovery system'
            echo "last updated: $(cat /var/lib/deck-recovery/updated 2>/dev/null || echo 'never (set up: sudo bash /etc/deck-recovery/refresh.sh --enroll)')"
            echo 'update now:   sudo systemctl start deck-recovery-update.service'
            systemctl list-timers deck-recovery-update.timer --no-pager
            echo; echo '# Servers (deck-status)'
            python3 -c 'import json,os; s=json.load(open(os.path.expanduser("~/.local/state/deck/status.json")))
for h in s.get("hosts", []): print(h["line"], "-", h["state"]); [print("  " + d) for d in h["detail"]]' 2>/dev/null || echo 'not gathered yet'
            echo; read -r -p 'Press Enter to close.' _`], "maintenance");
    }
}
