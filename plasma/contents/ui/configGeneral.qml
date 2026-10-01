/*
    SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
    SPDX-License-Identifier: GPL-2.0-or-later
*/

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM

KCM.SimpleKCM {
    property alias cfg_closeOnCopy: closeOnCopy.checked
    property alias cfg_backendCommand: backendCommand.text

    Kirigami.FormLayout {
        QQC2.CheckBox {
            id: closeOnCopy
            Kirigami.FormData.label: i18n("After copying:")
            text: i18n("Close the popup")
        }

        QQC2.TextField {
            id: backendCommand
            Kirigami.FormData.label: i18n("Command:")
            placeholderText: "ente-codes"
        }

        QQC2.Label {
            Layout.maximumWidth: Kirigami.Units.gridUnit * 22
            wrapMode: Text.WordWrap
            opacity: 0.7
            text: i18n("How long a copied code stays on the clipboard and when the codes lock themselves are set with “ente-codes settings” in a terminal.")
        }
    }
}
