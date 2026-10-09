import QtQuick

// Small rounded button in the omni panel's page headers.
Rectangle {
    id: root

    property string text
    property bool active
    signal clicked

    implicitWidth: label.implicitWidth + 24
    implicitHeight: 28
    radius: height / 2
    color: active ? Qt.alpha(Theme.brass, mouse.containsMouse ? 0.32 : 0.22)
                  : Qt.alpha(Theme.parchment, mouse.containsMouse ? 0.1 : 0.05)
    border.width: 1
    border.color: active ? Qt.alpha(Theme.brass, 0.5) : Qt.alpha(Theme.parchment, 0.1)

    Text {
        id: label
        anchors.centerIn: parent
        text: root.text
        color: root.active ? Theme.parchment : Theme.linen
        font.family: Theme.sans
        font.pixelSize: 12
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }
}
