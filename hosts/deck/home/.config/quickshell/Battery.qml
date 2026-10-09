pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Services.UPower

// Laptop battery state, plus a notification when it runs low.
Singleton {
    id: root

    readonly property var device: UPower.displayDevice
    readonly property bool present: device.isLaptopBattery
    readonly property real level: device.percentage
    readonly property bool charging: device.state === UPowerDeviceState.Charging
        || device.state === UPowerDeviceState.PendingCharge
        || device.state === UPowerDeviceState.FullyCharged
    readonly property bool low: present && !charging && level < 0.15
    readonly property bool critical: present && !charging && level < 0.05

    function duration(seconds) {
        const h = Math.floor(seconds / 3600);
        const m = Math.round((seconds % 3600) / 60);
        return h > 0 ? `${h} h ${m} min` : `${m} min`;
    }

    readonly property string detail: {
        if (device.state === UPowerDeviceState.FullyCharged)
            return "Full";
        if (charging)
            return device.timeToFull > 0 ? `${duration(device.timeToFull)} to full` : "Charging";
        return device.timeToEmpty > 0 ? `${duration(device.timeToEmpty)} left` : "On battery";
    }

    // Notify once per threshold; plugging in re-arms both.
    property bool warnedLow: false
    property bool warnedCritical: false

    function notify(summary, body, urgency) {
        Quickshell.execDetached(["gdbus", "call", "--session",
            "--dest", "org.freedesktop.Notifications",
            "--object-path", "/org/freedesktop/Notifications",
            "--method", "org.freedesktop.Notifications.Notify",
            "Battery", "0", "battery-caution", summary, body, "[]",
            `{"urgency": <byte ${urgency}>}`, "-1"]);
    }

    onLowChanged: {
        if (low && !warnedLow) {
            warnedLow = true;
            notify("Battery low", `${Math.round(level * 100)}% left, ${detail}.`, 1);
        }
    }

    onCriticalChanged: {
        if (critical && !warnedCritical) {
            warnedCritical = true;
            notify("Battery critical", `${Math.round(level * 100)}% left. Plug in now.`, 2);
        }
    }

    onChargingChanged: {
        if (charging) {
            warnedLow = false;
            warnedCritical = false;
        }
    }
}
