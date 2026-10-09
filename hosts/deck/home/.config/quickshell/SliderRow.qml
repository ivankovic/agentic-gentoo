import QtQuick
import QtQuick.Layouts

// Icon, bar and percentage. Clicking the icon emits iconClicked (e.g. mute).
RowLayout {
    id: root

    property int icon
    property real value // 0..1
    property bool dimmed
    signal moved(real value)
    signal iconClicked

    Layout.fillWidth: true
    spacing: 12

    Text {
        Layout.preferredWidth: 22
        text: Theme.glyph(root.icon)
        color: root.dimmed ? Theme.taupe : Theme.linen
        font.family: Theme.icons
        font.pixelSize: 18

        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: root.iconClicked()
        }
    }

    Item {
        Layout.fillWidth: true
        implicitHeight: 20

        Rectangle {
            id: track
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width
            height: 6
            radius: 3
            color: Qt.alpha(Theme.parchment, 0.08)

            Rectangle {
                width: Math.max(height, track.width * Math.min(root.value, 1))
                height: parent.height
                radius: 3
                color: root.dimmed ? Theme.bark : Theme.brass
            }
        }

        Rectangle {
            x: track.width * Math.min(root.value, 1) - width / 2
            anchors.verticalCenter: parent.verticalCenter
            width: 14
            height: 14
            radius: 7
            color: Theme.parchment
            visible: drag.containsMouse || drag.pressed
        }

        MouseArea {
            id: drag
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            function set(x) { root.moved(Math.max(0, Math.min(1, x / width))); }
            onPressed: mouse => set(mouse.x)
            onPositionChanged: mouse => { if (pressed) set(mouse.x); }
            onWheel: wheel => root.moved(Math.max(0, Math.min(1, root.value + (wheel.angleDelta.y > 0 ? 0.05 : -0.05))))
        }
    }

    Text {
        Layout.preferredWidth: 40
        horizontalAlignment: Text.AlignRight
        text: Math.round(root.value * 100) + "%"
        color: Theme.taupe
        font.family: Theme.sans
        font.pixelSize: 12
    }
}
