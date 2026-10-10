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

    // Brightness by time of day, applied on resume (swayidle's after-resume, see niri's
    // config.kdl): [hour it starts, level]. Before the first entry the last one holds.
    readonly property var schedule: [[7, 0.45], [9, 0.7], [18, 0.45], [21, 0.25]]

    function auto() {
        const hour = new Date().getHours();
        const entry = schedule.filter(e => e[0] <= hour).pop() ?? schedule[schedule.length - 1];
        set(entry[1]);
    }

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

        function auto(): void {
            root.auto();
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
