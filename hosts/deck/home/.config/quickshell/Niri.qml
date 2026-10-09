pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Windows and keyboard layouts from `niri msg -j event-stream`, which first
// sends the full state and then one JSON event per line.
Singleton {
    id: root

    property var windows: []
    property var layouts: []
    property int layoutIndex: 0
    readonly property string layout: layouts[layoutIndex] ?? ""

    signal layoutSwitched

    function focusWindow(id) {
        Quickshell.execDetached(["niri", "msg", "action", "focus-window", "--id", String(id)]);
    }

    // Short label for an xkb layout name, e.g. "German (Switzerland)" -> "CH".
    function layoutCode(name) {
        const codes = { "English (US)": "US", "German (Switzerland)": "CH", "Croatian": "HR" };
        return codes[name] ?? name.slice(0, 2).toUpperCase();
    }

    function handle(event) {
        if (event.WindowsChanged) {
            windows = event.WindowsChanged.windows;
        } else if (event.WindowOpenedOrChanged) {
            const w = event.WindowOpenedOrChanged.window;
            windows = windows.filter(x => x.id !== w.id).concat([w]);
        } else if (event.WindowClosed) {
            windows = windows.filter(x => x.id !== event.WindowClosed.id);
        } else if (event.KeyboardLayoutsChanged) {
            layouts = event.KeyboardLayoutsChanged.keyboard_layouts.names;
            layoutIndex = event.KeyboardLayoutsChanged.keyboard_layouts.current_idx;
        } else if (event.KeyboardLayoutSwitched) {
            layoutIndex = event.KeyboardLayoutSwitched.idx;
            layoutSwitched();
        }
    }

    Process {
        id: stream
        command: ["niri", "msg", "-j", "event-stream"]
        running: true
        stdout: SplitParser {
            onRead: line => {
                try {
                    root.handle(JSON.parse(line));
                } catch (e) {
                    console.warn("niri event:", e);
                }
            }
        }
        // niri restarted or the socket hiccuped: reconnect.
        onExited: restart.start()
    }

    Timer {
        id: restart
        interval: 2000
        onTriggered: stream.running = true
    }
}
