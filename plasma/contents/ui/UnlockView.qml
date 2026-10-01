/*
    SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
    SPDX-License-Identifier: GPL-2.0-or-later
*/

// The locked state: type the passphrase right in the popup.

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PC3

Item {
    id: view

    required property var backend
    property alias field: passphrase

    function submit() {
        if (passphrase.text.length === 0 || view.backend.unlocking) {
            return
        }
        view.backend.unlockWith(passphrase.text)
    }

    function focusField() {
        passphrase.forceActiveFocus()
    }

    Connections {
        target: view.backend
        function onUnlockFailed(wrong) {
            passphrase.selectAll()
            passphrase.forceActiveFocus()
            if (wrong) shake.restart()
        }
        function onVaultStateChanged() {
            if (view.backend.unlocked) {
                passphrase.text = ""
                view.backend.unlockError = ""
            }
        }
    }

    ColumnLayout {
        id: column
        anchors.centerIn: parent
        width: Math.min(parent.width - Kirigami.Units.gridUnit * 2, Kirigami.Units.gridUnit * 16)
        spacing: Kirigami.Units.largeSpacing

        Kirigami.Icon {
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: Kirigami.Units.iconSizes.huge
            Layout.preferredHeight: Layout.preferredWidth
            source: "object-locked"
        }

        Kirigami.Heading {
            Layout.fillWidth: true
            level: 3
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            text: i18n("Your codes are locked")
        }

        Kirigami.PasswordField {
            id: passphrase
            Layout.fillWidth: true
            placeholderText: i18n("Passphrase")
            enabled: !view.backend.unlocking
            onAccepted: view.submit()

            transform: Translate { id: nudge }

            SequentialAnimation {
                id: shake
                NumberAnimation { target: nudge; property: "x"; to: 8; duration: 50 }
                NumberAnimation { target: nudge; property: "x"; to: -8; duration: 70 }
                NumberAnimation { target: nudge; property: "x"; to: 5; duration: 60 }
                NumberAnimation { target: nudge; property: "x"; to: 0; duration: 50 }
            }
        }

        PC3.Label {
            Layout.fillWidth: true
            visible: view.backend.unlockError.length > 0 && !view.backend.unlocking
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            color: Kirigami.Theme.negativeTextColor
            text: view.backend.unlockError === "wrong passphrase" ? i18n("Wrong passphrase") : view.backend.unlockError
        }

        PC3.Button {
            Layout.alignment: Qt.AlignHCenter
            icon.name: "object-unlocked"
            text: i18n("Unlock")
            enabled: passphrase.text.length > 0 && !view.backend.unlocking
            onClicked: view.submit()

            PC3.BusyIndicator {
                anchors.centerIn: parent
                width: parent.height
                height: width
                running: view.backend.unlocking
                visible: running
            }
        }
    }
}
