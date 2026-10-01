/*
    SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
    SPDX-License-Identifier: GPL-2.0-or-later
*/

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PC3
import org.kde.plasma.extras as PlasmaExtras

PlasmaExtras.Representation {
    id: full

    required property var backend
    required property var root
    required property var cfg      // Plasmoid.configuration, or a stand-in in previews

    // Current time in ms; the countdowns are measured from backend.fetchedAt.
    property real now: Date.now()
    property string copiedId: ""
    property string filter: search.text.trim().toLowerCase()

    readonly property var shown: backend.entries.filter(e => filter.length === 0
        || (e.issuer || "").toLowerCase().indexOf(filter) >= 0
        || (e.account || "").toLowerCase().indexOf(filter) >= 0)

    // Plasma lets the user resize the popup by dragging its edge and remembers
    // the size; these are only the first-time default and the smallest size.
    Layout.minimumWidth: Kirigami.Units.gridUnit * 14
    Layout.minimumHeight: Kirigami.Units.gridUnit * 14
    Layout.preferredWidth: Kirigami.Units.gridUnit * 17
    Layout.preferredHeight: backend.vaultState === "empty" ? Kirigami.Units.gridUnit * 30 : Kirigami.Units.gridUnit * 25
    collapseMarginsHint: true

    function remainingFor(entry) {
        const elapsed = (full.now - full.backend.fetchedAt) / 1000
        return Math.max(0, Math.ceil(entry.remaining - elapsed))
    }

    function formatCode(code) {
        if (!code) return "—"
        if (code.length === 6) return code.slice(0, 3) + " " + code.slice(3)
        if (code.length === 8) return code.slice(0, 4) + " " + code.slice(4)
        return code
    }

    function copyEntry(entry) {
        if (entry && entry.code) {
            full.backend.copy(entry.id)
        }
    }

    Connections {
        target: full.backend
        function onCopied(id) {
            full.copiedId = id
            copiedReset.restart()
            if (full.cfg.closeOnCopy) {
                closeLater.restart()
            }
        }
    }

    Timer {
        id: copiedReset
        interval: 1500
        onTriggered: full.copiedId = ""
    }

    Timer {
        id: closeLater
        interval: 450
        onTriggered: full.root.expanded = false
    }

    // Ticks the countdowns; fetches new codes the moment any of them runs out.
    Timer {
        interval: 1000
        repeat: true
        running: full.root.expanded && full.backend.unlocked
        onTriggered: {
            full.now = Date.now()
            if (full.backend.entries.some(e => e.code && full.remainingFor(e) === 0)) {
                full.backend.fetchCodes()
            }
        }
    }

    Connections {
        target: full.root
        function onExpandedChanged() {
            if (full.root.expanded) {
                search.text = ""
                full.now = Date.now()
                if (full.backend.unlocked) search.forceActiveFocus()
                else if (full.backend.vaultState === "locked") unlockView.focusField()
            }
        }
    }

    header: PlasmaExtras.PlasmoidHeading {
        leftPadding: Kirigami.Units.largeSpacing
        rightPadding: Kirigami.Units.smallSpacing

        contentItem: RowLayout {
            spacing: Kirigami.Units.smallSpacing

            PlasmaExtras.SearchField {
                id: search
                Layout.fillWidth: true
                // Only once there is something to search: a focused search
                // field shows its Ctrl+F hint over the lock message otherwise.
                visible: full.backend.unlocked
                placeholderText: i18n("Search…")
                Keys.onReturnPressed: full.copyEntry(full.shown[list.currentIndex >= 0 ? list.currentIndex : 0])
                Keys.onEnterPressed: full.copyEntry(full.shown[list.currentIndex >= 0 ? list.currentIndex : 0])
                Keys.onDownPressed: list.incrementCurrentIndex()
                Keys.onUpPressed: list.decrementCurrentIndex()
                onTextChanged: list.currentIndex = 0
            }

            Kirigami.Heading {
                Layout.fillWidth: true
                visible: !full.backend.unlocked
                level: 4
                text: i18n("Ente Auth Codes")
            }

            PC3.ToolButton {
                id: syncButton
                readonly property var sync: full.backend.sync || ({})
                visible: full.backend.unlocked && sync.configured === true
                enabled: sync.running !== true
                icon.name: sync.error ? "emblem-warning" : "view-refresh"
                onClicked: full.backend.syncNow()
                PC3.ToolTip.text: {
                    if (sync.running) return i18n("Syncing with Ente…")
                    if (sync.error) return i18n("Sync failed: %1", sync.error)
                    if (!sync.last) return i18n("Sync with Ente now")
                    const minutes = Math.round((full.now / 1000 - sync.last) / 60)
                    return minutes < 1 ? i18n("Synced with Ente just now")
                        : i18np("Synced with Ente %1 minute ago", "Synced with Ente %1 minutes ago", minutes)
                }
                PC3.ToolTip.visible: hovered
                PC3.ToolTip.delay: Kirigami.Units.toolTipDelay
                Accessible.name: i18n("Sync with Ente now")

                PC3.BusyIndicator {
                    anchors.fill: parent
                    running: syncButton.sync.running === true
                    visible: running
                }
            }

            PC3.ToolButton {
                icon.name: "object-locked"
                visible: full.backend.unlocked
                onClicked: full.backend.lock()
                PC3.ToolTip.text: i18n("Lock now")
                PC3.ToolTip.visible: hovered
                PC3.ToolTip.delay: Kirigami.Units.toolTipDelay
                Accessible.name: i18n("Lock now")
            }
        }
    }

    SetupGuide {
        anchors.fill: parent
        visible: full.backend.vaultState === "empty"
        backend: full.backend
    }

    // A sync runs in the background: follow it closely while it lasts, then
    // fetch the codes it may have brought.
    Timer {
        interval: 1000
        repeat: true
        running: full.root.expanded && full.backend.unlocked && (full.backend.sync || {}).running === true
        onTriggered: full.backend.refreshStatus()
        onRunningChanged: if (!running && full.root.expanded && full.backend.unlocked) full.backend.fetchCodes()
    }

    // While the guide is up, notice when setup finished in the terminal.
    Timer {
        interval: 2000
        repeat: true
        running: full.root.expanded && full.backend.vaultState === "empty"
        onTriggered: full.backend.refreshStatus()
    }

    Connections {
        target: full.backend
        function onVaultStateChanged() {
            if (full.backend.vaultState === "unlocked" && full.root.expanded) {
                if (full.backend.entries.length === 0) full.backend.fetchCodes()
                search.forceActiveFocus()
            }
        }
    }

    UnlockView {
        id: unlockView
        anchors.fill: parent
        visible: full.backend.vaultState === "locked"
        backend: full.backend
    }

    PlasmaExtras.PlaceholderMessage {
        anchors.centerIn: parent
        width: parent.width - Kirigami.Units.gridUnit * 3
        visible: !full.backend.unlocked && ["unknown", "empty", "locked"].indexOf(full.backend.vaultState) < 0
        iconName: {
            switch (full.backend.vaultState) {
            case "refused": return "security-low"
            default: return "dialog-warning"
            }
        }
        text: {
            switch (full.backend.vaultState) {
            case "missing": return i18n("ente-codes is not installed")
            case "refused": return i18n("Access refused")
            default: return i18n("Something went wrong")
            }
        }
        explanation: {
            switch (full.backend.vaultState) {
            case "missing": return i18n("Install it with install.sh (see the README), or set its path in the widget settings.")
            default: return full.backend.error
            }
        }
        helpfulAction: QQC2.Action {
            icon.name: "view-refresh"
            text: i18n("Try again")
            onTriggered: full.backend.refreshStatus()
        }
    }

    PlasmaExtras.PlaceholderMessage {
        anchors.centerIn: parent
        width: parent.width - Kirigami.Units.gridUnit * 3
        visible: full.backend.unlocked && full.shown.length === 0 && full.backend.entries.length > 0
        iconName: "edit-none"
        text: i18n("No matches")
    }

    PC3.ScrollView {
        anchors.fill: parent
        visible: full.backend.unlocked && full.shown.length > 0

        contentItem: ListView {
            id: list
            model: full.shown
            currentIndex: 0
            keyNavigationEnabled: true
            highlightMoveDuration: 0
            highlight: PlasmaExtras.Highlight {}
            boundsBehavior: Flickable.StopAtBounds

            delegate: PC3.ItemDelegate {
                id: row
                required property var modelData
                required property int index
                readonly property int secondsLeft: full.remainingFor(modelData)
                readonly property bool justCopied: full.copiedId === modelData.id

                width: ListView.view.width
                hoverEnabled: true
                onHoveredChanged: if (hovered) list.currentIndex = index
                onClicked: full.copyEntry(modelData)
                Accessible.name: (modelData.issuer || modelData.account) + " " + (modelData.code || "")

                contentItem: RowLayout {
                    spacing: Kirigami.Units.largeSpacing

                    ServiceIcon {
                        Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                        Layout.preferredHeight: Layout.preferredWidth
                        source: row.modelData.icon || ""
                        tint: row.modelData.tint || ""
                        name: row.modelData.issuer || row.modelData.account
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        PC3.Label {
                            Layout.fillWidth: true
                            text: row.modelData.issuer || row.modelData.account
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                        }
                        PC3.Label {
                            Layout.fillWidth: true
                            visible: row.modelData.issuer.length > 0 && row.modelData.account.length > 0
                            text: row.modelData.account
                            opacity: 0.7
                            font: Kirigami.Theme.smallFont
                            elide: Text.ElideMiddle
                        }
                    }

                    PC3.Label {
                        text: row.justCopied ? i18n("Copied") : full.formatCode(row.modelData.code)
                        font.family: row.justCopied ? Kirigami.Theme.defaultFont.family : "monospace"
                        font.pixelSize: Kirigami.Theme.defaultFont.pixelSize * (row.justCopied ? 1.0 : 1.35)
                        font.weight: Font.Medium
                        color: row.justCopied ? Kirigami.Theme.positiveTextColor
                            : row.secondsLeft <= 5 ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                    }

                    CountdownRing {
                        Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                        Layout.preferredHeight: Layout.preferredWidth
                        visible: !!row.modelData.code
                        fraction: row.modelData.period > 0 ? row.secondsLeft / row.modelData.period : 0
                        urgent: row.secondsLeft <= 5
                    }
                }
            }
        }
    }
}
