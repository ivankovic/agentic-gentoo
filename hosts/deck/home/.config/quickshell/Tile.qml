import QtQuick
import QtQuick.Layouts

// A toggle in the omni panel's status grid.
Rectangle {
    id: root

    property int icon
    property string label
    property string detail
    property bool active
    signal clicked

    Layout.fillWidth: true
    implicitHeight: 64
    radius: 10
    color: active ? Qt.alpha(Theme.brass, mouse.containsMouse ? 0.26 : 0.18)
                  : Qt.alpha(Theme.parchment, mouse.containsMouse ? 0.08 : 0.04)
    border.width: 1
    border.color: active ? Qt.alpha(Theme.brass, 0.45) : Qt.alpha(Theme.parchment, 0.08)

    Behavior on color { ColorAnimation { duration: 120 } }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 14
        anchors.rightMargin: 10
        spacing: 12

        Text {
            text: Theme.glyph(root.icon)
            color: root.active ? Theme.brass : Theme.taupe
            font.family: Theme.icons
            font.pixelSize: 20
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0

            Text {
                Layout.fillWidth: true
                text: root.label
                color: Theme.parchment
                font.family: Theme.serif
                font.pixelSize: 14
                elide: Text.ElideRight
            }

            Text {
                Layout.fillWidth: true
                text: root.detail
                color: Theme.taupe
                font.family: Theme.sans
                font.pixelSize: 11
                elide: Text.ElideRight
            }
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }
}
