pragma Singleton

import QtQuick
import Quickshell

// Dark academia palette: walnut, leather and parchment, with brass accents.
// foot, mako, swaylock, niri and the GTK css files repeat these values by hand.
Singleton {
    readonly property color espresso: "#1c1612"
    readonly property color walnut: "#241d18"
    readonly property color leather: "#2f2620"
    readonly property color umber: "#3d3129"
    readonly property color bark: "#54453a"
    readonly property color taupe: "#8a7866"
    readonly property color linen: "#c9b89f"
    readonly property color parchment: "#ebdfca"

    readonly property color brass: "#c9a46a"
    readonly property color rose: "#c08a7e"
    readonly property color oxblood: "#b0645a"
    readonly property color sage: "#9ea67e"
    readonly property color ochre: "#d6ac63"
    readonly property color slate: "#8b9eab"

    readonly property string serif: "Iosevka Etoile"
    readonly property string sans: "Iosevka"
    readonly property string icons: "Symbols Nerd Font"

    readonly property int radius: 14

    // Material Design icons from Symbols Nerd Font.
    function glyph(codepoint) {
        return String.fromCodePoint(codepoint);
    }
}
