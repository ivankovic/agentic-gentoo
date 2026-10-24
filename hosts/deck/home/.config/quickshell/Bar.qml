import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

// The status bar along the bottom: where the laptop is (home, away, a sign-in page, offline),
// then deck, domaci and core (hosts.toml's order), each a dot and a name with its problems. Calm
// when all is well: grey text and green dots. Only "now" items are named (red); "soon" ones are
// counted (amber); the backlog is only in the popup. A click opens a popup above the bar from
// what deck-status already gathered (Status.qml), and asks it to gather again.
PanelWindow {
    id: bar

    required property var modelData
    screen: modelData

    anchors {
        left: true
        right: true
        bottom: true
    }
    implicitHeight: 24
    exclusiveZone: implicitHeight
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "status-bar"
    color: Theme.espresso

    // "" (closed), "network", "maintenance" or a host's name.
    property string open: ""
    property real openX: 0

    function toggle(what, item) {
        if (open === what) {
            open = "";
            return;
        }
        openX = item.mapToItem(row, 0, 0).x;
        open = what;
        Status.refresh();
    }

    function stateColor(s) {
        return s === "ok" ? Theme.sage : s === "partial" ? Theme.ochre : s === "down" || s === "now" ? Theme.oxblood : Theme.taupe;
    }
    function stateDot(s) {
        return s === "ok" ? "●" : s === "partial" || s === "now" ? "◐" : s === "down" ? "○" : "–";
    }
    readonly property var netGlyph: ({home: 0xF02DC, away: 0xF001D, portal: 0xF0342, "no-internet": 0xF0A8E, offline: 0xF05AA})

    Rectangle {
        anchors.top: parent.top
        width: parent.width
        height: 1
        color: Theme.umber
    }

    RowLayout {
        id: row
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        spacing: 18

        Segment {
            id: net
            glyph: bar.netGlyph[Status.network.state] ?? 0xF0A8E
            text: Status.network.line
            color: ["portal", "no-internet", "offline"].includes(Status.network.state) ? Theme.ochre : Theme.linen
            onClicked: bar.toggle("network", net)
        }

        // This machine, then each server in hosts.toml's order: a dot and a name, problems after it.
        Segment {
            id: deck
            readonly property string state: Status.age < 0 || Status.age > 900 ? "unchecked"
                : Status.now.length ? "now" : Status.soon ? "partial" : "ok"
            dot: bar.stateDot(state)
            dotColor: bar.stateColor(state)
            color: state === "ok" ? Theme.taupe : bar.stateColor(state)
            text: Status.age < 0 ? `${Status.host}: deck-status isn't running`
                : Status.age > 900 ? `${Status.host}: status ${Math.round(Status.age / 60)} min old`
                : state === "ok" ? Status.host
                : Status.host + ": " + [Status.now.length ? Status.now[0] + (Status.now.length > 1 ? ` +${Status.now.length - 1}` : "") : "",
                   Status.soon ? `${Status.soon} to look at` : ""].filter(t => t).join(" · ")
            onClicked: bar.toggle("maintenance", deck)
        }

        Repeater {
            model: Status.hosts
            Segment {
                id: host
                required property var modelData
                dot: bar.stateDot(modelData.state)
                dotColor: bar.stateColor(modelData.state)
                text: modelData.line
                color: modelData.state === "ok" || modelData.state === "unchecked" ? Theme.taupe : bar.stateColor(modelData.state)
                onClicked: bar.toggle(modelData.name, host)
            }
        }

        Item {
            Layout.fillWidth: true
        }
    }

    PopupWindow {
        id: popup
        visible: bar.open !== ""
        anchor.window: bar
        anchor.rect.x: Math.min(bar.openX, bar.width - implicitWidth - 8)
        anchor.rect.y: -implicitHeight - 6
        implicitWidth: 460
        implicitHeight: Math.min(body.implicitHeight + 28, 560)
        color: "transparent"

        readonly property var host: Status.hosts.find(h => h.name === bar.open)

        Rectangle {
            anchors.fill: parent
            radius: Theme.radius
            color: Theme.walnut
            border.color: Theme.umber

            Flickable {
                anchors.fill: parent
                anchors.margins: 14
                contentHeight: body.implicitHeight
                clip: true

                ColumnLayout {
                    id: body
                    width: parent.width
                    spacing: 6

                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            Layout.fillWidth: true
                            text: bar.open === "network" ? "Network" : bar.open === "maintenance" ? Status.host : bar.open
                            color: Theme.parchment
                            font.family: Theme.serif
                            font.pixelSize: 16
                        }
                        Text {
                            text: "×"
                            color: Theme.taupe
                            font.pixelSize: 18
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: bar.open = ""
                            }
                        }
                    }

                    // Network
                    Repeater {
                        model: bar.open === "network" ? [
                            Status.network.line,
                            `checked ${Status.ago(Status.network.checked)}`,
                            Status.network.state === "home" ? "home: the home probe answers" : Status.network.state === "away" ? "away: internet works, the home probe doesn't answer" : ""
                        ].filter(l => l) : []
                        Line {
                            required property var modelData
                            text: modelData
                        }
                    }
                    Button {
                        visible: bar.open === "network" && Status.network.state === "portal"
                        text: "Open the sign-in page"
                        onClicked: {
                            Session.launch(["xdg-open", "http://neverssl.com"], "portal");
                            bar.open = "";
                        }
                    }

                    // A host
                    Repeater {
                        model: popup.host ? popup.host.detail.concat([`checked ${Status.ago(Status.data.hosts_checked)}`]) : []
                        Line {
                            required property var modelData
                            text: modelData
                            color: modelData.startsWith("DOWN") ? Theme.oxblood : Theme.linen
                        }
                    }

                    // Maintenance, by level
                    Repeater {
                        model: bar.open === "maintenance" ? [["now", "Now", Theme.oxblood], ["soon", "Soon", Theme.ochre], ["backlog", "Backlog", Theme.taupe]] : []
                        ColumnLayout {
                            id: group
                            required property var modelData
                            readonly property var list: Status.items.filter(i => i.level === modelData[0])
                            visible: list.length > 0
                            Layout.fillWidth: true
                            spacing: 4
                            Text {
                                Layout.topMargin: 6
                                text: group.modelData[1]
                                color: group.modelData[2]
                                font.family: Theme.sans
                                font.pixelSize: 12
                                font.bold: true
                            }
                            Repeater {
                                model: group.list
                                ColumnLayout {
                                    id: entry
                                    required property var modelData
                                    property bool expanded: false
                                    Layout.fillWidth: true
                                    spacing: 2
                                    Line {
                                        text: (entry.expanded ? "▾ " : "▸ ") + entry.modelData.line
                                        color: Theme.parchment
                                        MouseArea {
                                            anchors.fill: parent
                                            cursorShape: Qt.PointingHandCursor
                                            onClicked: entry.expanded = !entry.expanded
                                        }
                                    }
                                    Line {
                                        visible: entry.expanded && entry.modelData.detail
                                        Layout.leftMargin: 14
                                        text: entry.modelData.detail
                                        color: Theme.taupe
                                    }
                                }
                            }
                        }
                    }
                    Line {
                        visible: bar.open === "maintenance" && Status.items.length === 0
                        text: "Nothing. Checked " + Status.ago(Status.data.local_checked) + "."
                    }
                    RowLayout {
                        visible: bar.open === "maintenance"
                        Layout.topMargin: 8
                        spacing: 8
                        Button {
                            text: "Full report"
                            onClicked: {
                                bar.open = "";
                                Maintenance.show();
                            }
                        }
                        Button {
                            visible: Status.items.some(i => i.line.startsWith("Crashed") || i.line.startsWith("Unclean"))
                            text: "Mark crashes seen"
                            onClicked: {
                                Quickshell.execDetached(["sh", "-c", "deck-attention --seen; systemctl --user kill --kill-whom=main --signal=USR1 deck-status.service"]);
                            }
                        }
                    }
                }
            }
        }
    }

    component Segment: MouseArea {
        id: seg
        property int glyph: 0
        property string dot: ""
        property color dotColor: Theme.taupe
        property string text: ""
        property color color: Theme.linen
        Layout.fillHeight: true
        implicitWidth: segRow.implicitWidth
        cursorShape: Qt.PointingHandCursor
        RowLayout {
            id: segRow
            anchors.verticalCenter: parent.verticalCenter
            spacing: 6
            Text {
                visible: seg.glyph !== 0
                text: Theme.glyph(seg.glyph)
                color: seg.color
                font.family: Theme.icons
                font.pixelSize: 13
            }
            Text {
                visible: seg.dot !== ""
                text: seg.dot
                color: seg.dotColor
                font.pixelSize: 11
            }
            Text {
                text: seg.text
                color: seg.color
                font.family: Theme.sans
                font.pixelSize: 12
                elide: Text.ElideRight
                Layout.maximumWidth: 420
            }
        }
    }

    component Line: Text {
        Layout.fillWidth: true
        color: Theme.linen
        font.family: Theme.sans
        font.pixelSize: 12
        wrapMode: Text.Wrap
    }

    component Button: MouseArea {
        id: btn
        property string text: ""
        implicitWidth: label.implicitWidth + 20
        implicitHeight: 26
        cursorShape: Qt.PointingHandCursor
        Rectangle {
            anchors.fill: parent
            radius: 8
            color: btn.containsMouse ? Theme.umber : Theme.leather
        }
        hoverEnabled: true
        Text {
            id: label
            anchors.centerIn: parent
            text: btn.text
            color: Theme.parchment
            font.family: Theme.sans
            font.pixelSize: 12
        }
    }
}
