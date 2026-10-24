pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// What the status bar shows, as deck-status (usr-local/bin/deck-status, a user service) gathered
// it into ~/.local/state/deck/status.json. Reading a file never waits; refresh() asks the service
// to gather again now (the bar does when a popup opens), and the file's change updates the bar.
Singleton {
    id: root

    // Set when the file has loaded: file.text() in a binding doesn't update when it does.
    property string raw: ""
    readonly property var data: {
        try {
            return JSON.parse(raw || "{}");
        } catch (e) {
            return {};
        }
    }
    // This machine's name, as the bar's first segment calls it.
    property string host: "this machine"
    readonly property var network: data.network ?? {state: "unknown", line: "Status not gathered yet"}
    readonly property var hosts: data.hosts ?? []
    // Deck's own: each server's problems are on its own segment, not counted again here.
    readonly property var items: (data.local ?? []).filter(i => i.line)
    readonly property var now: items.filter(i => i.level === "now").map(i => i.line)
    readonly property int soon: items.filter(i => i.level === "soon").length
    readonly property int backlog: items.filter(i => i.level === "backlog").length

    // Seconds since the service last wrote: stale when it stopped; -1 with no status at all. The
    // clock moves on each load too: a status newer than the last tick read as a negative age.
    property real clock: Date.now()
    readonly property int age: data.updated ? Math.max(0, Math.floor(clock / 1000 - data.updated)) : -1
    Timer {
        running: true
        repeat: true
        interval: 30000
        onTriggered: {
            root.clock = Date.now();
            file.reload(); // deck-status replaces the file, which the watch may not follow
        }
    }

    function refresh() {
        // Only the main process: the rest of the unit (ip monitor, a check's ssh) would die of it.
        Quickshell.execDetached(["systemctl", "--user", "kill", "--kill-whom=main", "--signal=USR1", "deck-status.service"]);
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
        // Only what parses: a bad read keeps the last good status.
        onLoaded: {
            try {
                JSON.parse(text());
                root.raw = text();
                root.clock = Date.now();
            } catch (e) {}
        }
    }

    FileView {
        path: "/etc/hostname"
        onLoaded: root.host = text().trim() || root.host
    }
}
