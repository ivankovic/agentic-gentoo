import QtQuick
import QtQuick.Effects
import Quickshell
import Quickshell.Wayland

// Time and date floating in the top-right corner. The top layer sits above
// windows but below fullscreen ones; an empty mask lets clicks through.
PanelWindow {
    id: root

    required property var modelData
    screen: modelData

    anchors {
        top: true
        right: true
    }
    margins {
        top: 10
        right: 22
    }
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "clock"
    color: "transparent"
    mask: Region {}

    implicitWidth: column.implicitWidth + 24
    implicitHeight: column.implicitHeight + 24

    IdleInhibitor {
        window: root
        enabled: Session.keepAwake
    }

    SystemClock {
        id: clock
        precision: SystemClock.Minutes
    }

    Column {
        id: column
        anchors.centerIn: parent
        spacing: -4

        layer.enabled: true
        layer.effect: MultiEffect {
            shadowEnabled: true
            shadowColor: Theme.espresso
            shadowBlur: 0.8
            shadowOpacity: 0.9
            shadowVerticalOffset: 1
        }

        Row {
            anchors.right: parent.right
            spacing: 10

            // Below 15% and not charging: the level in oxblood next to the time.
            Row {
                visible: Battery.low
                anchors.verticalCenter: parent.verticalCenter
                spacing: 3

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: Theme.glyph(Battery.critical ? 0xF0083 : 0xF007A)
                    color: Theme.oxblood
                    font.family: Theme.icons
                    font.pixelSize: 16
                }

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: Math.round(Battery.level * 100) + "%"
                    color: Theme.oxblood
                    font.family: Theme.serif
                    font.pixelSize: 16
                }
            }

            Text {
                text: Qt.formatDateTime(clock.date, "HH:mm")
                color: Battery.critical ? Theme.oxblood : Theme.antique
                font.family: Theme.serif
                font.pixelSize: 30
            }
        }

        Row {
            anchors.right: parent.right
            spacing: 6

            // Only when not on the first (US) layout.
            Text {
                visible: Niri.layoutIndex !== 0 && Niri.layout !== ""
                anchors.verticalCenter: parent.verticalCenter
                text: Niri.layoutCode(Niri.layout)
                color: Theme.antique
                font.family: Theme.sans
                font.pixelSize: 11
                font.bold: true
                font.letterSpacing: 0.8
            }

            Text {
                visible: Session.keepAwake
                anchors.verticalCenter: parent.verticalCenter
                text: Theme.glyph(0xF0176)
                color: Theme.brass
                font.family: Theme.icons
                font.pixelSize: 12
            }

            Text {
                text: Qt.formatDateTime(clock.date, "dddd, d MMMM")
                color: Theme.antique
                font.family: Theme.serif
                font.italic: true
                font.pixelSize: 13
                font.letterSpacing: 0.4
            }
        }
    }
}
