pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// What the status bar shows, as deck-status (usr-local/bin/deck-status, a user service) gathered
// it into ~/.local/state/deck/status.json. Reading a file never waits; refresh() asks the service
// to gather again now (the bar does when a popup opens), and the file's change updates the bar.
Singleton {
    id: root

    readonly property var data: {
        try {
            return JSON.parse(file.text() || "{}");
        } catch (e) {
            return {};
        }
    }
    readonly property var network: data.network ?? {state: "unknown", line: "Status not gathered yet"}
    readonly property var hosts: data.hosts ?? []
    readonly property var items: data.items ?? []
    readonly property var now: data.now ?? []
    readonly property int soon: data.soon ?? 0
    readonly property int backlog: items.filter(i => i.level === "backlog").length

    // Seconds since the service last wrote: stale when it stopped.
    property real clock: Date.now()
    readonly property int age: data.updated ? Math.floor(clock / 1000 - data.updated) : -1
    Timer {
        running: true
        repeat: true
        interval: 30000
        onTriggered: root.clock = Date.now()
    }

    function refresh() {
        Quickshell.execDetached(["systemctl", "--user", "kill", "--signal=USR1", "deck-status.service"]);
    }

    function ago(t) {
        if (!t)
            return "never";
        const s = Math.floor(clock / 1000 - t);
        return s < 90 ? "just now" : s < 5400 ? `${Math.round(s / 60)} min ago` : `${Math.round(s / 3600)} h ago`;
    }

    FileView {
        id: file
        path: `${Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state"}/deck/status.json`
        watchChanges: true
        onFileChanged: reload()
    }
}
