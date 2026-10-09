pragma Singleton

import QtQuick
import Quickshell

// State shared between the clock and the omni panel.
Singleton {
    // Held by an IdleInhibitor on the clock, which is always mapped; pauses swayidle.
    property bool keepAwake: false
}
