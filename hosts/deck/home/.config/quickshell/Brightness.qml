pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Backlight through brightnessctl. The brightness keys call `qs ipc call
// brightness up|down`, so the OSD sees every change.
Singleton {
    id: root

    property real value: 0

    signal adjusted

    function refresh() {
        reader.running = true;
    }

    function set(v) {
        value = Math.max(0.01, Math.min(1, v));
        writer.restart();
        adjusted();
    }

    IpcHandler {
        target: "brightness"

        function up(): void {
            root.set(root.value + 0.1);
        }

        function down(): void {
            root.set(root.value - 0.1);
        }
    }

    Process {
        id: reader
        // e.g. "intel_backlight,backlight,9600,50%,19200"
        command: ["brightnessctl", "--class=backlight", "-m", "info"]
        running: true
        stdout: StdioCollector {
            onStreamFinished: {
                const f = text.trim().split(",");
                if (f.length >= 5)
                    root.value = Number(f[2]) / Number(f[4]);
            }
        }
    }

    // Coalesce slider drags into at most one brightnessctl call per 50 ms.
    Timer {
        id: writer
        interval: 50
        onTriggered: Quickshell.execDetached(["brightnessctl", "--class=backlight", "set", Math.round(root.value * 100) + "%"])
    }
}
