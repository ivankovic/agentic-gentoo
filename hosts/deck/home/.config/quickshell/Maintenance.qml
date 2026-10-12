pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Things that need a human: security advisories for installed packages (written daily by
// etc/systemd/system/gentoo-sync.service), firmware updates (fwupd) and failed systemd units
// (btrbk, gentoo-sync, btrfs-scrub, ...), and a recovery system (p3) more than 6 weeks out of date
// (etc/deck-recovery/refresh.sh). Plus what /usr/local/bin/deck-attention finds: work only on
// deck, config drift from the repo, recent crashes, an unclean shutdown, Portage falling behind, a
// nearly full disk. The omni panel shows a tile while any is pending. Crashes and the unclean
// shutdown are events: opening the details marks them as seen, and the tile drops them.
Singleton {
    id: root

    // "202601-01 [N] Title ( category/package )" per advisory.
    readonly property var advisories: glsa.text().split("\n").filter(l => /^\d{6}-\d+ /.test(l))
    property var firmware: []
    property var failedUnits: []
    property var attention: []
    // Days since the recovery system's last update; -1 when it has never been recorded.
    property real now: Date.now()
    readonly property real recoveryAge: {
        const t = Date.parse(recovery.text().trim());
        return isNaN(t) ? -1 : Math.floor((now - t) / 86400000);
    }

    readonly property var issues: [].concat(
        advisories.length ? [advisories.length === 1 ? "1 security advisory" : `${advisories.length} security advisories`] : [],
        firmware.length ? [`Firmware update: ${firmware.join(", ")}`] : [],
        failedUnits.length ? [`Failed: ${failedUnits.join(", ")}`] : [],
        recoveryAge < 0 ? ["Recovery updates not set up"] : recoveryAge > 42 ? [`Recovery system ${recoveryAge} days old`] : [],
        attention)

    // Called when the panel opens. fwupd is asked at most hourly; the rest is cheap.
    property real firmwareCheckedAt: 0
    function refresh() {
        now = Date.now();
        failed.running = true;
        attentionCheck.running = true;
        if (Date.now() - firmwareCheckedAt > 3600 * 1000) {
            firmwareCheckedAt = Date.now();
            fwupd.running = true;
        }
    }

    // Details in a terminal: the advisories, firmware, failed units with their logs, what
    // deck-attention found, disk health, and the recovery.
    function show() {
        Session.launch(["foot", "-T", "maintenance", "sh", "-c", `
            echo '# Security advisories'; glsa-check -l affected -n 2>/dev/null
            echo; echo '# Firmware'; fwupdmgr get-updates --no-unreported-check 2>&1
            echo; echo '# Failed units'; systemctl --failed --no-pager; systemctl --user --failed --no-pager
            for u in $(systemctl --failed --no-legend --plain | cut -d' ' -f1); do
                echo; journalctl -u "$u" -n 15 --no-pager
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
            echo; read -r -p 'Press Enter to close.' _`], "maintenance");
    }

    FileView {
        id: glsa
        path: "/var/lib/gentoo-security/affected"
        watchChanges: true
        onFileChanged: reload()
    }

    FileView {
        id: recovery
        path: "/var/lib/deck-recovery/updated"
        watchChanges: true
        onFileChanged: reload()
    }

    Process {
        id: failed
        command: ["sh", "-c", "systemctl --failed --no-legend --plain; systemctl --user --failed --no-legend --plain"]
        stdout: StdioCollector {
            onStreamFinished: root.failedUnits = text.split("\n").map(l => l.trim().split(/\s+/)[0]).filter(u => u)
        }
    }

    Process {
        id: attentionCheck
        command: ["deck-attention"]
        stdout: StdioCollector {
            onStreamFinished: root.attention = text.split("\n").filter(l => l.trim())
        }
    }

    Process {
        id: fwupd
        command: ["fwupdmgr", "get-updates", "--json"]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    root.firmware = (JSON.parse(text).Devices ?? []).map(d => d.Name);
                } catch (e) {
                    root.firmware = [];
                }
            }
        }
    }

    Component.onCompleted: refresh()
}
