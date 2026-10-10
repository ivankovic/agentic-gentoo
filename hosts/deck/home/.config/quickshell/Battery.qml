pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.UPower

// Laptop battery state, a notification when it runs low, and the power profile per plug state.
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

    // Charge limit (etc/tmpfiles.d/battery.conf): 75/80 normally, 95/100 for one full charge.
    // The kernel wants start < end, so the order of the two writes matters.
    readonly property string thresholds: "/sys/class/power_supply/BAT0/charge_control_"
    readonly property bool fullCharge: parseInt(endThreshold.text()) === 100

    function refresh() {
        endThreshold.reload();
    }

    function setFullCharge(on) {
        limiter.command = ["sh", "-c", on
            ? `echo 100 > ${thresholds}end_threshold && echo 95 > ${thresholds}start_threshold`
            : `echo 75 > ${thresholds}start_threshold && echo 80 > ${thresholds}end_threshold`];
        limiter.running = true;
    }

    FileView {
        id: endThreshold
        path: root.thresholds + "end_threshold"
    }

    Process {
        id: limiter
        onExited: root.refresh()
    }

    // Power profile follows the plug: Performance on AC, Balanced on battery. Applied at startup
    // and on every plug change; a choice made in the panel holds until the next one.
    function applyProfile() {
        PowerProfiles.profile = UPower.onBattery ? PowerProfile.Balanced
            : PowerProfiles.hasPerformanceProfile ? PowerProfile.Performance : PowerProfile.Balanced;
    }

    Connections {
        target: UPower
        function onOnBatteryChanged() {
            root.applyProfile();
        }
    }

    // At startup quickshell hasn't read power-profiles-daemon's profile list yet (and refuses
    // Performance until it has), so apply again once it arrives.
    Connections {
        target: PowerProfiles
        function onHasPerformanceProfileChanged() {
            root.applyProfile();
        }
    }

    Component.onCompleted: applyProfile()

    function duration(seconds) {
        const h = Math.floor(seconds / 3600);
        const m = Math.round((seconds % 3600) / 60);
        return h > 0 ? `${h} h ${m} min` : `${m} min`;
    }

    readonly property string detail: {
        if (device.state === UPowerDeviceState.FullyCharged)
            return "Full";
        // On AC but held below the charge_control thresholds (75/80).
        if (device.state === UPowerDeviceState.PendingCharge)
            return fullCharge ? "Plugged in" : "Plugged in, charge limit";
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
