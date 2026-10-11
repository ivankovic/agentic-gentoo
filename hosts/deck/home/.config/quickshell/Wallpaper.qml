import QtQuick
import Quickshell
import Quickshell.Wayland

// The wallpaper, on the background layer. niri's layer rule for the "wallpaper" namespace puts
// it in the backdrop. The candlelit night variant fades in through dusk, following the sun as
// wlsunset does (same rounded Zurich coordinates). Both images come from
// ~/.local/share/wallpapers/academia.py.
PanelWindow {
    id: root

    required property var modelData
    screen: modelData

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.layer: WlrLayer.Background
    WlrLayershell.namespace: "wallpaper"
    color: Theme.espresso
    mask: Region {}

    readonly property real latitude: 47.4
    readonly property real longitude: 8.5

    // Solar elevation in degrees (low-precision almanac formula, good to a fraction of a degree).
    function sunElevation(date) {
        const rad = Math.PI / 180;
        const d = date.getTime() / 86400000 - 10957.5; // days since J2000.0
        const g = (357.529 + 0.98560028 * d) * rad;
        const q = 280.459 + 0.98564736 * d;
        const l = (q + 1.915 * Math.sin(g) + 0.020 * Math.sin(2 * g)) * rad;
        const e = (23.439 - 0.00000036 * d) * rad;
        const ra = Math.atan2(Math.cos(e) * Math.sin(l), Math.cos(l));
        const dec = Math.asin(Math.sin(e) * Math.sin(l));
        const gmst = (18.697374558 + 24.06570982441908 * d) % 24;
        const ha = (gmst * 15 + longitude) * rad - ra;
        const lat = latitude * rad;
        return Math.asin(Math.sin(lat) * Math.sin(dec) + Math.cos(lat) * Math.cos(dec) * Math.cos(ha)) / rad;
    }

    // 0 by day, 1 at night: the sun from 3° above the horizon to 6° below (civil dusk).
    function nightness() {
        return Math.max(0, Math.min(1, (3 - sunElevation(new Date())) / 9));
    }

    SystemClock {
        id: clock
        precision: SystemClock.Minutes
    }

    readonly property real night: { clock.date; return nightness(); }

    Image {
        anchors.fill: parent
        source: "file://" + Quickshell.env("HOME") + "/.local/share/wallpapers/academia.png"
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
    }

    Image {
        anchors.fill: parent
        source: "file://" + Quickshell.env("HOME") + "/.local/share/wallpapers/academia-night.png"
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        opacity: root.night

        // Steps are a minute apart; glide between them.
        Behavior on opacity {
            NumberAnimation { duration: 30000 }
        }
    }
}
