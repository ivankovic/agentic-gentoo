pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// How often each app was launched from the omni panel, so frequent ones rank
// first. Stored in ~/.local/state/quickshell/by-shell/<id>/launches.json.
Singleton {
    id: root

    property var counts: ({})

    function record(id) {
        const next = Object.assign({}, counts);
        next[id] = (next[id] ?? 0) + 1;
        counts = next;
        file.setText(JSON.stringify(counts));
    }

    // 0 for never, about 10 after a handful of launches, capped at 20.
    function boost(id) {
        return Math.min(20, 5 * Math.log2(1 + (counts[id] ?? 0)));
    }

    FileView {
        id: file
        path: Quickshell.statePath("launches.json")
        printErrors: false
        onLoaded: {
            try {
                root.counts = JSON.parse(text());
            } catch (e) {
                root.counts = {};
            }
        }
    }
}
