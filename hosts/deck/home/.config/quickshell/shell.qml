import QtQuick
import Quickshell

ShellRoot {
    Variants {
        model: Quickshell.screens

        Clock {}
    }

    Variants {
        model: Quickshell.screens

        Wallpaper {}
    }

    Omni {}

    Osd {}
}
