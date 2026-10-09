import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Widgets
import Quickshell.Bluetooth
import Quickshell.Networking
import Quickshell.Services.Mpris
import Quickshell.Services.Pipewire
import Quickshell.Services.UPower

// Mod+D: status at a glance, plus one search field for apps and actions.
// An empty query shows the dashboard; typing replaces it with results.
PanelWindow {
    id: root

    visible: false
    color: "transparent"
    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "omni"
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None
    BackgroundEffect.blurRegion: Region { item: card }

    function open() {
        search.text = "";
        brightness.refresh();
        visible = true;
        search.forceActiveFocus();
    }

    function close() {
        visible = false;
    }

    IpcHandler {
        target: "omni"

        function toggle(): void {
            root.visible ? root.close() : root.open();
        }

        // Open with a query already typed, e.g. `qs ipc call omni query wifi`.
        function query(text: string): void {
            root.open();
            search.text = text;
        }
    }

    // --- State ---------------------------------------------------------------

    SystemClock {
        id: clock
        precision: SystemClock.Minutes
    }

    readonly property var battery: UPower.displayDevice
    readonly property bool charging: battery.state === UPowerDeviceState.Charging
        || battery.state === UPowerDeviceState.PendingCharge
        || battery.state === UPowerDeviceState.FullyCharged

    readonly property var sink: Pipewire.defaultAudioSink
    PwObjectTracker {
        objects: [root.sink]
    }

    readonly property var wifi: Networking.devices.values.find(d => d.type === DeviceType.Wifi) ?? null
    readonly property var wifiNetwork: wifi ? wifi.networks.values.find(n => n.connected) ?? null : null

    readonly property var bluetooth: Bluetooth.defaultAdapter
    readonly property var bluetoothConnected: bluetooth ? bluetooth.devices.values.filter(d => d.connected) : []

    readonly property var player: Mpris.players.values.find(p => p.isPlaying) ?? Mpris.players.values[0] ?? null

    QtObject {
        id: brightness

        property real value: 0

        function refresh() {
            reader.running = true;
        }

        function set(v) {
            value = Math.max(0.01, v);
            writer.restart();
        }

        property Process reader: Process {
            // e.g. "intel_backlight,backlight,9600,50%,19200"
            command: ["brightnessctl", "--class=backlight", "-m", "info"]
            stdout: StdioCollector {
                onStreamFinished: {
                    const f = text.trim().split(",");
                    if (f.length >= 5)
                        brightness.value = Number(f[2]) / Number(f[4]);
                }
            }
        }

        // Coalesce slider drags into at most one brightnessctl call per 50 ms.
        property Timer writer: Timer {
            interval: 50
            onTriggered: Quickshell.execDetached(["brightnessctl", "--class=backlight", "set", Math.round(brightness.value * 100) + "%"])
        }
    }

    function duration(seconds) {
        const h = Math.floor(seconds / 3600);
        const m = Math.round((seconds % 3600) / 60);
        return h > 0 ? `${h} h ${m} min` : `${m} min`;
    }

    readonly property string batteryDetail: {
        if (battery.state === UPowerDeviceState.FullyCharged)
            return "Full";
        if (charging)
            return battery.timeToFull > 0 ? `${duration(battery.timeToFull)} to full` : "Charging";
        return battery.timeToEmpty > 0 ? `${duration(battery.timeToEmpty)} left` : "On battery";
    }

    readonly property var profiles: [PowerProfile.PowerSaver, PowerProfile.Balanced]
        .concat(PowerProfiles.hasPerformanceProfile ? [PowerProfile.Performance] : [])
    readonly property var profileNames: ["Power saver", "Balanced", "Performance"]
    readonly property var profileIcons: [0xF032A, 0xF05D1, 0xF04C5]

    // --- Search --------------------------------------------------------------

    function spawn(command) {
        Quickshell.execDetached(command);
    }

    // Static actions; toggles read their current state so the label says what Enter does.
    readonly property var actions: [
        { name: "Lock", keywords: "screen swaylock", icon: 0xF033E, run: () => spawn(["swaylock", "-f"]) },
        { name: "Suspend", keywords: "sleep", icon: 0xF0904, run: () => spawn(["systemctl", "suspend"]) },
        { name: "Reboot", keywords: "restart", icon: 0xF0709, run: () => spawn(["systemctl", "reboot"]) },
        { name: "Power off", keywords: "shutdown poweroff", icon: 0xF0425, run: () => spawn(["systemctl", "poweroff"]) },
        { name: "Log out", keywords: "quit exit niri session", icon: 0xF0343, run: () => spawn(["niri", "msg", "action", "quit", "--skip-confirmation"]) },
        { name: "Screenshot", keywords: "capture screen", icon: 0xF0100, run: () => screenshot.start() },
        { name: Networking.wifiEnabled ? "Turn Wi-Fi off" : "Turn Wi-Fi on", keywords: "wifi wireless network", icon: 0xF05A9, run: () => Networking.wifiEnabled = !Networking.wifiEnabled },
        { name: bluetooth?.enabled ? "Turn Bluetooth off" : "Turn Bluetooth on", keywords: "bluetooth bt", icon: 0xF00AF, run: () => { if (bluetooth) bluetooth.enabled = !bluetooth.enabled; } },
        { name: Session.keepAwake ? "Stop keeping awake" : "Keep awake", keywords: "caffeine idle inhibit", icon: 0xF0176, run: () => Session.keepAwake = !Session.keepAwake },
        { name: "Power saver", keywords: "profile battery", icon: 0xF032A, run: () => PowerProfiles.profile = PowerProfile.PowerSaver },
        { name: "Balanced", keywords: "profile power", icon: 0xF05D1, run: () => PowerProfiles.profile = PowerProfile.Balanced },
        { name: "Performance", keywords: "profile power", icon: 0xF04C5, run: () => PowerProfiles.profile = PowerProfile.Performance },
    ]

    // Let the panel unmap before niri's screenshot UI captures the screen.
    Timer {
        id: screenshot
        interval: 250
        onTriggered: root.spawn(["niri", "msg", "action", "screenshot"])
    }

    // Fuzzy (in-order letters) matches only count when `fuzzy` is set; on
    // keywords they match nearly anything.
    function score(text, q, fuzzy) {
        const t = (text ?? "").toLowerCase();
        if (!t)
            return 0;
        if (t.startsWith(q))
            return 100 - t.length * 0.1;
        if (t.split(/[\s\-_.]+/).some(w => w.startsWith(q)))
            return 80;
        if (t.includes(q))
            return 60;
        if (!fuzzy)
            return 0;
        let i = 0;
        for (const c of t)
            if (c === q[i] && ++i === q.length)
                return 30;
        return 0;
    }

    function calculate(q) {
        if (!/^[\d\s+\-*/().,%^]+$/.test(q) || !/\d/.test(q) || !/[+\-*/%^]/.test(q))
            return null;
        try {
            const r = Function(`"use strict"; return (${q.replace(/\^/g, "**").replace(/,/g, ".")});`)();
            return Number.isFinite(r) ? Number(r.toPrecision(12)) : null;
        } catch (e) {
            return null;
        }
    }

    readonly property var results: {
        const q = search.text.trim().toLowerCase();
        if (!q)
            return [];

        const out = [];
        const r = calculate(q);
        if (r !== null)
            out.push({ name: `= ${r}`, detail: "Copy to clipboard", icon: 0xF00EC, score: 1000, run: () => spawn(["wl-copy", String(r)]) });

        for (const a of actions) {
            const s = Math.max(score(a.name, q, true), score(a.keywords, q) * 0.7);
            if (s > 0)
                out.push({ name: a.name, detail: "Action", icon: a.icon, score: s + 5, run: a.run });
        }

        for (const app of DesktopEntries.applications.values) {
            if (app.noDisplay)
                continue;
            const s = Math.max(score(app.name, q, true), score(app.genericName, q) * 0.8, score(app.keywords.join(" "), q) * 0.6);
            if (s > 0)
                out.push({ name: app.name, detail: app.genericName || app.comment, image: app.icon, score: s, run: () => app.execute() });
        }

        return out.sort((a, b) => b.score - a.score).slice(0, 8);
    }

    function activate(item) {
        if (!item)
            return;
        close();
        item.run();
    }

    // --- Layout --------------------------------------------------------------

    // Dim the desktop; clicking outside the card closes the panel.
    Rectangle {
        anchors.fill: parent
        color: Qt.alpha(Theme.espresso, 0.35)

        MouseArea {
            anchors.fill: parent
            onClicked: root.close()
        }
    }

    Rectangle {
        id: card

        anchors.horizontalCenter: parent.horizontalCenter
        y: Math.round(parent.height * 0.14)
        width: 620
        height: content.implicitHeight + 40
        radius: Theme.radius
        color: Qt.alpha(Theme.walnut, 0.82)
        border.width: 1
        border.color: Qt.alpha(Theme.brass, 0.25)
        clip: true

        Behavior on height { NumberAnimation { duration: 140; easing.type: Easing.OutCubic } }

        // Swallow clicks so they don't reach the backdrop.
        MouseArea {
            anchors.fill: parent
        }

        ColumnLayout {
            id: content

            anchors {
                top: parent.top
                left: parent.left
                right: parent.right
                margins: 20
            }
            spacing: 16

            // Search field.
            RowLayout {
                Layout.fillWidth: true
                spacing: 12

                Text {
                    text: Theme.glyph(0xF0349)
                    color: Theme.brass
                    font.family: Theme.icons
                    font.pixelSize: 20
                }

                TextInput {
                    id: search

                    Layout.fillWidth: true
                    color: Theme.parchment
                    selectionColor: Theme.bark
                    selectedTextColor: Theme.parchment
                    font.family: Theme.serif
                    font.pixelSize: 20
                    cursorDelegate: Rectangle {
                        width: 2
                        color: Theme.brass
                        visible: search.cursorVisible
                    }

                    onTextChanged: list.currentIndex = 0

                    Keys.onPressed: event => {
                        if (event.key === Qt.Key_Escape) {
                            root.close();
                        } else if (event.key === Qt.Key_Down || event.key === Qt.Key_Tab) {
                            list.incrementCurrentIndex();
                        } else if (event.key === Qt.Key_Up || event.key === Qt.Key_Backtab) {
                            list.decrementCurrentIndex();
                        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                            root.activate(root.results[list.currentIndex]);
                        } else {
                            return;
                        }
                        event.accepted = true;
                    }

                    Text {
                        visible: !search.text
                        text: "Search apps and actions…"
                        color: Theme.taupe
                        font: search.font
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 1
                color: Qt.alpha(Theme.brass, 0.18)
            }

            // Results.
            ListView {
                id: list

                visible: search.text.trim() !== ""
                Layout.fillWidth: true
                implicitHeight: Math.max(contentHeight, 44)
                interactive: false
                model: root.results
                highlightMoveDuration: 80
                highlight: Rectangle {
                    radius: 8
                    color: Qt.alpha(Theme.brass, 0.16)
                }

                delegate: Item {
                    id: row

                    required property var modelData
                    required property int index

                    width: list.width
                    height: 44

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 10
                        spacing: 12

                        Item {
                            implicitWidth: 24
                            implicitHeight: 24

                            IconImage {
                                anchors.fill: parent
                                visible: row.modelData.image !== undefined
                                source: row.modelData.image !== undefined ? Quickshell.iconPath(row.modelData.image, "application-x-executable") : ""
                            }

                            Text {
                                anchors.centerIn: parent
                                visible: row.modelData.image === undefined
                                text: Theme.glyph(row.modelData.icon ?? 0xF0349)
                                color: row.ListView.isCurrentItem ? Theme.brass : Theme.linen
                                font.family: Theme.icons
                                font.pixelSize: 18
                            }
                        }

                        Text {
                            text: row.modelData.name
                            color: Theme.parchment
                            font.family: Theme.serif
                            font.pixelSize: 15
                        }

                        Text {
                            Layout.fillWidth: true
                            text: row.modelData.detail ?? ""
                            color: Theme.taupe
                            font.family: Theme.sans
                            font.pixelSize: 12
                            elide: Text.ElideRight
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onEntered: list.currentIndex = row.index
                        onClicked: root.activate(row.modelData)
                    }
                }

                Text {
                    anchors.centerIn: parent
                    visible: list.count === 0
                    text: "Nothing in the catalogue."
                    color: Theme.taupe
                    font.family: Theme.serif
                    font.italic: true
                    font.pixelSize: 14
                }
            }

            // Dashboard.
            ColumnLayout {
                visible: !list.visible
                Layout.fillWidth: true
                spacing: 16

                RowLayout {
                    Layout.fillWidth: true

                    ColumnLayout {
                        spacing: -6

                        Text {
                            text: Qt.formatDateTime(clock.date, "HH:mm")
                            color: Theme.parchment
                            font.family: Theme.serif
                            font.pixelSize: 52
                        }

                        Text {
                            text: Qt.formatDateTime(clock.date, "dddd, d MMMM yyyy")
                            color: Theme.linen
                            font.family: Theme.serif
                            font.italic: true
                            font.pixelSize: 15
                        }
                    }

                    Item { Layout.fillWidth: true }

                    ColumnLayout {
                        visible: root.battery.isLaptopBattery
                        spacing: 2

                        RowLayout {
                            Layout.alignment: Qt.AlignRight
                            spacing: 6

                            Text {
                                text: Theme.glyph(root.charging ? 0xF0084 : 0xF0079)
                                color: root.battery.percentage < 0.15 && !root.charging ? Theme.oxblood : Theme.brass
                                font.family: Theme.icons
                                font.pixelSize: 18
                            }

                            Text {
                                text: Math.round(root.battery.percentage * 100) + "%"
                                color: Theme.parchment
                                font.family: Theme.serif
                                font.pixelSize: 22
                            }
                        }

                        Text {
                            Layout.alignment: Qt.AlignRight
                            text: root.batteryDetail
                            color: Theme.taupe
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                    }
                }

                GridLayout {
                    Layout.fillWidth: true
                    columns: 2
                    rowSpacing: 10
                    columnSpacing: 10

                    Tile {
                        icon: Networking.wifiEnabled ? 0xF05A9 : 0xF05AA
                        label: "Wi-Fi"
                        detail: !Networking.wifiEnabled ? "Off"
                            : root.wifiNetwork ? `${root.wifiNetwork.name} · ${Math.round(root.wifiNetwork.signalStrength * 100)}%`
                            : "Not connected"
                        active: Networking.wifiEnabled
                        onClicked: Networking.wifiEnabled = !Networking.wifiEnabled
                    }

                    Tile {
                        icon: root.bluetooth?.enabled ? 0xF00AF : 0xF00B2
                        label: "Bluetooth"
                        detail: !root.bluetooth ? "No adapter"
                            : !root.bluetooth.enabled ? "Off"
                            : root.bluetoothConnected.length ? root.bluetoothConnected.map(d => d.name).join(", ")
                            : "No devices"
                        active: root.bluetooth?.enabled ?? false
                        onClicked: if (root.bluetooth) root.bluetooth.enabled = !root.bluetooth.enabled
                    }

                    Tile {
                        icon: root.profileIcons[PowerProfiles.profile]
                        label: root.profileNames[PowerProfiles.profile]
                        detail: "Power profile"
                        active: PowerProfiles.profile !== PowerProfile.Balanced
                        onClicked: {
                            const i = root.profiles.indexOf(PowerProfiles.profile);
                            PowerProfiles.profile = root.profiles[(i + 1) % root.profiles.length];
                        }
                    }

                    Tile {
                        icon: 0xF0176
                        label: "Keep awake"
                        detail: Session.keepAwake ? "Screen stays on" : "Locks after 5 min"
                        active: Session.keepAwake
                        onClicked: Session.keepAwake = !Session.keepAwake
                    }
                }

                SliderRow {
                    visible: root.sink?.audio !== undefined
                    icon: root.sink?.audio?.muted ? 0xF0581 : 0xF057E
                    value: root.sink?.audio?.volume ?? 0
                    dimmed: root.sink?.audio?.muted ?? false
                    onMoved: v => root.sink.audio.volume = v
                    onIconClicked: root.sink.audio.muted = !root.sink.audio.muted
                }

                SliderRow {
                    icon: 0xF00DF
                    value: brightness.value
                    onMoved: v => brightness.set(v)
                }

                // Now playing.
                Rectangle {
                    visible: root.player !== null
                    Layout.fillWidth: true
                    implicitHeight: 52
                    radius: 10
                    color: Qt.alpha(Theme.parchment, 0.04)

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 14
                        anchors.rightMargin: 8
                        spacing: 4

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0

                            Text {
                                Layout.fillWidth: true
                                text: root.player?.trackTitle || root.player?.identity || ""
                                color: Theme.parchment
                                font.family: Theme.serif
                                font.pixelSize: 14
                                elide: Text.ElideRight
                            }

                            Text {
                                Layout.fillWidth: true
                                text: root.player?.trackArtist || root.player?.identity || ""
                                color: Theme.taupe
                                font.family: Theme.sans
                                font.pixelSize: 11
                                elide: Text.ElideRight
                            }
                        }

                        Repeater {
                            model: [
                                { icon: 0xF04AE, run: () => root.player.previous() },
                                { icon: root.player?.isPlaying ? 0xF03E4 : 0xF040A, run: () => root.player.togglePlaying() },
                                { icon: 0xF04AD, run: () => root.player.next() },
                            ]

                            Text {
                                required property var modelData

                                Layout.preferredWidth: 32
                                horizontalAlignment: Text.AlignHCenter
                                text: Theme.glyph(modelData.icon)
                                color: Theme.linen
                                font.family: Theme.icons
                                font.pixelSize: 18

                                MouseArea {
                                    anchors.fill: parent
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: parent.modelData.run()
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
