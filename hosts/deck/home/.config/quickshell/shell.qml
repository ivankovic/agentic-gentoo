import QtQuick
import Quickshell

ShellRoot {
    Variants {
        model: Quickshell.screens

        Clock {}
    }

    Omni {}
}
