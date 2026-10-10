pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Things that need a human: security advisories for installed packages (written daily by
// etc/systemd/system/gentoo-sync.service), firmware updates (fwupd) and failed systemd units
// (btrbk, gentoo-sync, btrfs-scrub, ...). The omni panel shows a tile while any is pending.
Singleton {
    id: root

    // "202601-01 [N] Title ( category/package )" per advisory.
    readonly property var advisories: glsa.text().split("\n").filter(l => /^\d{6}-\d+ /.test(l))
    property var firmware: []
    property var failedUnits: []

    readonly property var issues: [].concat(
        advisories.length ? [advisories.length === 1 ? "1 security advisory" : `${advisories.length} security advisories`] : [],
        firmware.length ? [`Firmware update: ${firmware.join(", ")}`] : [],
        failedUnits.length ? [`Failed: ${failedUnits.join(", ")}`] : [])

    // Called when the panel opens. fwupd is asked at most hourly; the rest is cheap.
    property real firmwareCheckedAt: 0
    function refresh() {
        failed.running = true;
        if (Date.now() - firmwareCheckedAt > 3600 * 1000) {
            firmwareCheckedAt = Date.now();
            fwupd.running = true;
        }
    }

    // Details in a terminal: the advisories, firmware and failed units with their logs.
    function show() {
        Quickshell.execDetached(["foot", "-T", "maintenance", "sh", "-c", `
            echo '# Security advisories'; glsa-check -l affected -n 2>/dev/null
            echo; echo '# Firmware'; fwupdmgr get-updates --no-unreported-check 2>&1
            echo; echo '# Failed units'; systemctl --failed --no-pager; systemctl --user --failed --no-pager
            for u in $(systemctl --failed --no-legend --plain | cut -d' ' -f1); do
                echo; journalctl -u "$u" -n 15 --no-pager
            done
            echo; read -r -p 'Press Enter to close.' _`]);
    }

    FileView {
        id: glsa
        path: "/var/lib/gentoo-security/affected"
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
