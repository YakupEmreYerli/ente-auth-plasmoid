/*
    SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
    SPDX-License-Identifier: GPL-2.0-or-later
*/

import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore

PlasmoidItem {
    id: main

    Backend {
        id: ente
        command: Plasmoid.configuration.backendCommand || "ente-codes"
    }

    Plasmoid.icon: ente.unlocked ? "password-copy" : "object-locked"
    Plasmoid.status: PlasmaCore.Types.ActiveStatus

    toolTipMainText: i18n("Ente Auth Codes")
    toolTipSubText: {
        switch (ente.vaultState) {
        case "unlocked": return i18np("Unlocked, %1 code", "Unlocked, %1 codes", ente.count)
        case "locked": return i18n("Locked")
        case "empty": return i18n("No codes imported yet")
        case "missing": return i18n("The ente-codes command was not found")
        default: return ""
        }
    }

    fullRepresentation: FullRepresentation {
        backend: ente
        root: main
        cfg: Plasmoid.configuration
    }

    onExpandedChanged: {
        if (main.expanded) {
            ente.fetchCodes()
        } else {
            // Nothing stays on screen or in memory here once the popup closes.
            ente.entries = []
        }
    }

    Plasmoid.contextualActions: [
        PlasmaCore.Action {
            text: i18n("Unlock")
            icon.name: "object-unlocked"
            visible: ente.vaultState === "locked"
            onTriggered: main.expanded = true
        },
        PlasmaCore.Action {
            text: i18n("Lock")
            icon.name: "object-locked"
            visible: ente.unlocked
            onTriggered: ente.lock()
        }
    ]

    Timer {
        interval: 60 * 1000
        running: !main.expanded
        repeat: true
        triggeredOnStart: true
        onTriggered: ente.refreshStatus()
    }
}
