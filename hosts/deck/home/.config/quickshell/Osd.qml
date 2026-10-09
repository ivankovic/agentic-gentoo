import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import Quickshell.Services.Pipewire

// A brass bar near the bottom of the screen when volume, brightness or the
// keyboard layout changes. Click-through; fades out after a moment.
PanelWindow {
    id: root

    anchors.bottom: true
    margins.bottom: 72
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "osd"
    color: "transparent"
    mask: Region {}
    visible: pill.opacity > 0

    implicitWidth: 300
    implicitHeight: 56

    property string kind: "volume"

    readonly property var sink: Pipewire.defaultAudioSink
    PwObjectTracker {
        objects: [root.sink]
    }

    // Startup and default-sink changes set the volume too; don't flash for those.
    property bool armed: false
    Timer {
        running: true
        interval: 1500
        onTriggered: root.armed = true
    }

    function show(what) {
        if (!armed)
            return;
        kind = what;
        pill.opacity = 1;
        hide.restart();
    }

    Timer {
        id: hide
        interval: 1400
        onTriggered: pill.opacity = 0
    }

    Connections {
        target: root.sink?.audio ?? null
        function onVolumesChanged() { root.show("volume"); }
        function onMutedChanged() { root.show("volume"); }
    }

    Connections {
        target: Brightness
        function onAdjusted() { root.show("brightness"); }
    }

    Connections {
        target: Niri
        function onLayoutSwitched() { root.show("layout"); }
    }

    readonly property bool muted: root.sink?.audio?.muted ?? false
    readonly property real level: kind === "volume" ? (root.sink?.audio?.volume ?? 0) : Brightness.value

    Rectangle {
        id: pill

        anchors.fill: parent
        radius: height / 2
        color: Qt.alpha(Theme.walnut, 0.9)
        border.width: 1
        border.color: Qt.alpha(Theme.brass, 0.3)
        opacity: 0

        Behavior on opacity { NumberAnimation { duration: 180 } }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 20
            anchors.rightMargin: 20
            spacing: 14

            Text {
                text: Theme.glyph(root.kind === "layout" ? 0xF030C
                    : root.kind === "brightness" ? 0xF00DF
                    : root.muted ? 0xF0581 : 0xF057E)
                color: Theme.brass
                font.family: Theme.icons
                font.pixelSize: 20
            }

            Text {
                visible: root.kind === "layout"
                Layout.fillWidth: true
                text: Niri.layout
                color: Theme.parchment
                font.family: Theme.serif
                font.pixelSize: 16
                elide: Text.ElideRight
            }

            Rectangle {
                visible: root.kind !== "layout"
                Layout.fillWidth: true
                implicitHeight: 6
                radius: 3
                color: Qt.alpha(Theme.parchment, 0.1)

                Rectangle {
                    width: parent.width * Math.min(root.level, 1)
                    height: parent.height
                    radius: 3
                    color: root.kind === "volume" && root.muted ? Theme.bark : Theme.brass

                    Behavior on width { NumberAnimation { duration: 90 } }
                }
            }

            Text {
                visible: root.kind !== "layout"
                Layout.preferredWidth: 36
                horizontalAlignment: Text.AlignRight
                text: Math.round(root.level * 100) + "%"
                color: Theme.linen
                font.family: Theme.sans
                font.pixelSize: 13
            }
        }
    }
}
