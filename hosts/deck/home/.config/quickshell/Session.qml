pragma Singleton

import QtQuick
import Quickshell

// State shared between the clock and the omni panel, and launching.
Singleton {
    // Held by an IdleInhibitor on the clock, which is always mapped; pauses swayidle.
    property bool keepAwake: false

    // Start something that should outlive the shell (apps, terminals, wl-copy's clipboard
    // holder) in its own systemd scope under app.slice. Plain execDetached would leave it in
    // quickshell.service's cgroup: its memory would count as the shell's, and restarting the
    // shell would kill it. `name` becomes part of the unit name, app-quickshell-<name>-<n>.scope.
    function launch(command, name) {
        const unit = `app-quickshell-${String(name).replace(/[^A-Za-z0-9_.:]/g, "_")}-${Date.now()}.scope`;
        Quickshell.execDetached(["systemd-run", "--user", "--scope", "--slice=app.slice", "--collect",
            "--quiet", `--unit=${unit}`, "--"].concat(command));
    }

    // A desktop entry, honouring Terminal= and Path=.
    function launchApp(entry) {
        let command = entry.command;
        if (entry.runInTerminal)
            command = ["foot", "--"].concat(command);
        if (entry.workingDirectory)
            command = ["env", "-C", entry.workingDirectory].concat(command);
        launch(command, entry.id);
    }
}
