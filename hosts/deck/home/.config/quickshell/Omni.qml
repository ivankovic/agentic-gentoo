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

// Mod+D: status at a glance, plus one search field for open windows, apps and
// actions. An empty query shows the dashboard (or the Wi-Fi or Bluetooth page);
// typing replaces it with results.
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

    // "", "wifi" or "bluetooth".
    property string page: ""

    function open() {
        search.text = "";
        page = "";
        Brightness.refresh();
        visible = true;
        search.forceActiveFocus();
    }

    function close() {
        visible = false;
        page = "";
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

        // Open on a page: `qs ipc call omni page wifi` or `bluetooth`.
        function page(name: string): void {
            root.open();
            root.page = name;
        }
    }

    // --- State ---------------------------------------------------------------

    SystemClock {
        id: clock
        precision: SystemClock.Minutes
    }

    readonly property var sink: Pipewire.defaultAudioSink
    PwObjectTracker {
        objects: [root.sink]
    }

    readonly property var wifi: Networking.devices.values.find(d => d.type === DeviceType.Wifi) ?? null
    readonly property var wifiNetwork: wifi ? wifi.networks.values.find(n => n.connected) ?? null : null

    readonly property var bluetooth: Bluetooth.defaultAdapter
    readonly property var bluetoothConnected: bluetooth ? bluetooth.devices.values.filter(d => d.connected) : []

    readonly property var player: Mpris.players.values.find(p => p.isPlaying) ?? Mpris.players.values[0] ?? null

    readonly property var profiles: [PowerProfile.PowerSaver, PowerProfile.Balanced]
        .concat(PowerProfiles.hasPerformanceProfile ? [PowerProfile.Performance] : [])
    readonly property var profileNames: ["Power saver", "Balanced", "Performance"]
    readonly property var profileIcons: [0xF032A, 0xF05D1, 0xF04C5]

    // Scan for networks only while the Wi-Fi page is open; stop Bluetooth discovery on leaving.
    onPageChanged: {
        if (wifi)
            wifi.scannerEnabled = page === "wifi";
        if (bluetooth && page !== "bluetooth" && bluetooth.discovering)
            bluetooth.discovering = false;
        psk.network = null;
    }

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
        { name: "Wi-Fi networks", keywords: "wifi wireless network connect ssid", icon: 0xF05A9, stay: true, run: () => { search.text = ""; page = "wifi"; } },
        { name: bluetooth?.enabled ? "Turn Bluetooth off" : "Turn Bluetooth on", keywords: "bluetooth bt", icon: 0xF00AF, run: () => { if (bluetooth) bluetooth.enabled = !bluetooth.enabled; } },
        { name: "Bluetooth devices", keywords: "bluetooth bt headphones pair connect", icon: 0xF00AF, stay: true, run: () => { search.text = ""; page = "bluetooth"; } },
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

        // Open windows rank just above launching the same app again.
        for (const w of Niri.windows) {
            const app = DesktopEntries.heuristicLookup(w.app_id);
            const appName = app?.name ?? w.app_id ?? "";
            const s = Math.max(score(w.title, q, true), score(appName, q) * 0.9, score(w.app_id, q) * 0.9);
            if (s > 0)
                out.push({ name: w.title || appName, detail: `${appName} · window`, image: app?.icon ?? w.app_id ?? "", score: s + 8, run: () => Niri.focusWindow(w.id) });
        }

        for (const a of actions) {
            const s = Math.max(score(a.name, q, true), score(a.keywords, q) * 0.7);
            if (s > 0)
                out.push({ name: a.name, detail: "Action", icon: a.icon, score: s + 5, stay: a.stay, run: a.run });
        }

        for (const app of DesktopEntries.applications.values) {
            if (app.noDisplay)
                continue;
            const s = Math.max(score(app.name, q, true), score(app.genericName, q) * 0.8, score(app.keywords.join(" "), q) * 0.6);
            if (s > 0)
                out.push({ name: app.name, detail: app.genericName || app.comment, image: app.icon, score: s + Launches.boost(app.id), run: () => { Launches.record(app.id); app.execute(); } });
        }

        return out.sort((a, b) => b.score - a.score).slice(0, 8);
    }

    function activate(item) {
        if (!item)
            return;
        if (!item.stay)
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
                            if (root.page && !search.text)
                                root.page = "";
                            else
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
                        text: "Search windows, apps and actions…"
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
                            Layout.maximumWidth: 360
                            text: row.modelData.name
                            color: Theme.parchment
                            font.family: Theme.serif
                            font.pixelSize: 15
                            elide: Text.ElideRight
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
                visible: !list.visible && root.page === ""
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
                        visible: Battery.present
                        spacing: 2

                        RowLayout {
                            Layout.alignment: Qt.AlignRight
                            spacing: 6

                            Text {
                                text: Theme.glyph(Battery.charging ? 0xF0084 : 0xF0079)
                                color: Battery.low ? Theme.oxblood : Theme.brass
                                font.family: Theme.icons
                                font.pixelSize: 18
                            }

                            Text {
                                text: Math.round(Battery.level * 100) + "%"
                                color: Battery.low ? Theme.oxblood : Theme.parchment
                                font.family: Theme.serif
                                font.pixelSize: 22
                            }
                        }

                        Text {
                            Layout.alignment: Qt.AlignRight
                            text: Battery.detail
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
                        expandable: true
                        onClicked: Networking.wifiEnabled = !Networking.wifiEnabled
                        onExpanded: root.page = "wifi"
                    }

                    Tile {
                        icon: root.bluetooth?.enabled ? 0xF00AF : 0xF00B2
                        label: "Bluetooth"
                        detail: !root.bluetooth ? "No adapter"
                            : !root.bluetooth.enabled ? "Off"
                            : root.bluetoothConnected.length ? root.bluetoothConnected.map(d => d.name).join(", ")
                            : "No devices"
                        active: root.bluetooth?.enabled ?? false
                        expandable: root.bluetooth !== null
                        onClicked: if (root.bluetooth) root.bluetooth.enabled = !root.bluetooth.enabled
                        onExpanded: root.page = "bluetooth"
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
                    value: Brightness.value
                    onMoved: v => Brightness.set(v)
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

            // Wi-Fi and Bluetooth pages share a header: back, title, on/off.
            RowLayout {
                visible: !list.visible && root.page !== ""
                Layout.fillWidth: true
                spacing: 10

                Text {
                    text: Theme.glyph(0xF0141)
                    color: Theme.linen
                    font.family: Theme.icons
                    font.pixelSize: 22

                    MouseArea {
                        anchors.fill: parent
                        anchors.margins: -6
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.page = ""
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: root.page === "wifi" ? "Wi-Fi" : "Bluetooth"
                    color: Theme.parchment
                    font.family: Theme.serif
                    font.pixelSize: 22
                }

                // Bluetooth only: look for new devices.
                Pill {
                    visible: root.page === "bluetooth" && (root.bluetooth?.enabled ?? false)
                    text: root.bluetooth?.discovering ? "Searching…" : "Search"
                    active: root.bluetooth?.discovering ?? false
                    onClicked: root.bluetooth.discovering = !root.bluetooth.discovering
                }

                Pill {
                    readonly property bool on: root.page === "wifi" ? Networking.wifiEnabled : (root.bluetooth?.enabled ?? false)
                    text: on ? "On" : "Off"
                    active: on
                    onClicked: {
                        if (root.page === "wifi")
                            Networking.wifiEnabled = !Networking.wifiEnabled;
                        else if (root.bluetooth)
                            root.bluetooth.enabled = !root.bluetooth.enabled;
                    }
                }
            }

            // Password for a secured network that NetworkManager doesn't know yet.
            RowLayout {
                id: psk

                property var network: null
                property string error: ""

                visible: !list.visible && root.page === "wifi" && network !== null
                Layout.fillWidth: true
                spacing: 10

                Text {
                    text: Theme.glyph(0xF033E)
                    color: Theme.brass
                    font.family: Theme.icons
                    font.pixelSize: 18
                }

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 36
                    radius: 8
                    color: Qt.alpha(Theme.parchment, 0.05)
                    border.width: 1
                    border.color: pskInput.activeFocus ? Qt.alpha(Theme.brass, 0.6) : Qt.alpha(Theme.parchment, 0.1)

                    TextInput {
                        id: pskInput

                        anchors.fill: parent
                        anchors.leftMargin: 12
                        anchors.rightMargin: 12
                        verticalAlignment: TextInput.AlignVCenter
                        echoMode: TextInput.Password
                        color: Theme.parchment
                        font.family: Theme.sans
                        font.pixelSize: 14

                        Keys.onReturnPressed: {
                            psk.error = "";
                            psk.network.connectWithPsk(text);
                            text = "";
                        }
                        Keys.onEscapePressed: {
                            psk.network = null;
                            search.forceActiveFocus();
                        }

                        Text {
                            visible: !pskInput.text
                            anchors.verticalCenter: parent.verticalCenter
                            text: psk.error || `Password for ${psk.network?.name ?? ""}`
                            color: psk.error ? Theme.oxblood : Theme.taupe
                            font: pskInput.font
                        }
                    }
                }
            }

            ListView {
                id: pageList

                visible: !list.visible && root.page !== ""
                Layout.fillWidth: true
                implicitHeight: Math.min(Math.max(contentHeight, 44), 352)
                interactive: contentHeight > height
                clip: true
                spacing: 2

                model: {
                    if (root.page === "wifi") {
                        if (!root.wifi || !Networking.wifiEnabled)
                            return [];
                        // Connected, then saved, then by signal.
                        return root.wifi.networks.values
                            .filter(n => n.name)
                            .sort((a, b) => (b.connected - a.connected) || (b.known - a.known) || (b.signalStrength - a.signalStrength));
                    }
                    if (root.page === "bluetooth") {
                        if (!root.bluetooth?.enabled)
                            return [];
                        return root.bluetooth.devices.values
                            .filter(d => d.paired || d.connected || (root.bluetooth.discovering && d.deviceName))
                            .sort((a, b) => (b.connected - a.connected) || (b.paired - a.paired) || a.name.localeCompare(b.name));
                    }
                    return [];
                }

                delegate: Rectangle {
                    id: item

                    required property var modelData
                    readonly property bool isWifi: root.page === "wifi"
                    readonly property bool busy: isWifi ? modelData.stateChanging
                        : modelData.state === BluetoothDeviceState.Connecting || modelData.state === BluetoothDeviceState.Disconnecting || modelData.pairing

                    width: pageList.width
                    height: 44
                    radius: 8
                    color: hover.containsMouse ? Qt.alpha(Theme.brass, 0.12) : "transparent"

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 12
                        spacing: 12

                        Text {
                            Layout.preferredWidth: 24
                            horizontalAlignment: Text.AlignHCenter
                            text: Theme.glyph(!item.isWifi ? 0xF00AF
                                : item.modelData.signalStrength > 0.66 ? 0xF0928
                                : item.modelData.signalStrength > 0.33 ? 0xF0925
                                : 0xF091F)
                            color: item.modelData.connected ? Theme.brass : Theme.linen
                            font.family: Theme.icons
                            font.pixelSize: 18
                        }

                        Text {
                            Layout.maximumWidth: 320
                            text: item.modelData.name
                            color: Theme.parchment
                            font.family: Theme.serif
                            font.pixelSize: 15
                            elide: Text.ElideRight
                        }

                        Text {
                            Layout.fillWidth: true
                            text: {
                                const d = item.modelData;
                                if (item.busy)
                                    return d.connected ? "Disconnecting…" : "Connecting…";
                                if (item.isWifi) {
                                    if (d.connected)
                                        return "Connected";
                                    if (d.known)
                                        return "Saved";
                                    return d.security === WifiSecurityType.Open ? "Open" : "Secured";
                                }
                                if (d.connected)
                                    return d.batteryAvailable ? `Connected · ${Math.round(d.battery * 100)}%` : "Connected";
                                return d.paired ? "Paired" : "Pair";
                            }
                            color: item.modelData.connected ? Theme.brass : Theme.taupe
                            font.family: Theme.sans
                            font.pixelSize: 12
                            elide: Text.ElideRight
                        }

                        Text {
                            visible: item.isWifi && item.modelData.security !== WifiSecurityType.Open
                            text: Theme.glyph(0xF033E)
                            color: Theme.taupe
                            font.family: Theme.icons
                            font.pixelSize: 13
                        }
                    }

                    // A device being paired from this list connects as soon as pairing succeeds.
                    Connections {
                        target: item.isWifi ? null : item.modelData
                        function onPairedChanged() {
                            if (item.modelData.paired) {
                                item.modelData.trusted = true;
                                item.modelData.connect();
                            }
                        }
                    }

                    Connections {
                        target: item.isWifi ? item.modelData : null
                        function onConnectionFailed(reason) {
                            psk.network = item.modelData;
                            psk.error = "Wrong password? Try again.";
                            pskInput.forceActiveFocus();
                        }
                    }

                    MouseArea {
                        id: hover
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            const d = item.modelData;
                            if (item.busy)
                                return;
                            if (d.connected) {
                                d.disconnect();
                            } else if (!item.isWifi) {
                                d.paired ? d.connect() : d.pair();
                            } else if (d.known || d.security === WifiSecurityType.Open) {
                                d.connect();
                            } else {
                                psk.network = d;
                                psk.error = "";
                                pskInput.forceActiveFocus();
                            }
                        }
                    }
                }

                Text {
                    anchors.centerIn: parent
                    visible: pageList.count === 0
                    text: root.page === "wifi"
                        ? (Networking.wifiEnabled ? "Looking for networks…" : "Wi-Fi is off.")
                        : (root.bluetooth?.enabled ? "No paired devices. Search to find some." : "Bluetooth is off.")
                    color: Theme.taupe
                    font.family: Theme.serif
                    font.italic: true
                    font.pixelSize: 14
                }
            }
        }
    }
}
