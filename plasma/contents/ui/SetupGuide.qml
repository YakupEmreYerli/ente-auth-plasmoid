/*
    SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
    SPDX-License-Identifier: GPL-2.0-or-later
*/

// What a first-time user sees: two ways to bring the codes in, each one click.

import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PC3

PC3.ScrollView {
    id: guide

    required property var backend
    readonly property bool cliInstalled: (backend.sync || {}).cli === true

    contentWidth: availableWidth
    QQC2.ScrollBar.horizontal.policy: QQC2.ScrollBar.AlwaysOff

    ColumnLayout {
        width: guide.availableWidth
        spacing: Kirigami.Units.largeSpacing

        Item { Layout.preferredHeight: Kirigami.Units.smallSpacing }

        Kirigami.Icon {
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: Kirigami.Units.iconSizes.huge
            Layout.preferredHeight: Layout.preferredWidth
            source: "password-copy"
        }

        Kirigami.Heading {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            level: 3
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            text: i18n("Bring in your Ente Auth codes")
        }

        PC3.Label {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            opacity: 0.8
            text: i18n("Once they are here, click an account to copy its code.")
        }

        GuideOption {
            Layout.fillWidth: true
            recommended: true
            iconName: "folder-sync"
            title: i18n("Connect your Ente account")
            text: guide.cliInstalled
                ? i18n("A terminal opens: type your Ente e-mail, password and verification code, then choose a passphrase for this computer. New codes you add on your phone then appear here by themselves.")
                : i18n("Needs the Ente CLI, which is not installed. On Arch Linux install the ente-cli-bin package; elsewhere get it from Ente's GitHub releases.")
            actionText: guide.cliInstalled ? i18n("Connect…") : i18n("Get the Ente CLI")
            actionIcon: guide.cliInstalled ? "go-next" : "internet-services"
            onTriggered: {
                if (guide.cliInstalled) {
                    guide.backend.openSetup()
                } else {
                    Qt.openUrlExternally("https://github.com/ente-io/ente/tree/main/cli#readme")
                }
            }
        }

        GuideOption {
            Layout.fillWidth: true
            iconName: "document-import"
            title: i18n("Import an exported file")
            text: i18n("In the Ente Auth app: Settings → Data → Export codes → Plain text. Then pick that file here; it is wiped after importing. New codes must be imported again.")
            actionText: i18n("Choose file…")
            actionIcon: "document-open"
            busy: guide.backend.unlocking
            onTriggered: guide.backend.importFile()
        }

        Item { Layout.preferredHeight: Kirigami.Units.smallSpacing }
    }

    component GuideOption: Rectangle {
        id: option

        property bool recommended: false
        property bool busy: false
        property string iconName
        property string title
        property string text
        property string actionText
        property string actionIcon
        signal triggered()

        Layout.leftMargin: Kirigami.Units.largeSpacing
        Layout.rightMargin: Kirigami.Units.largeSpacing
        implicitHeight: body.implicitHeight + Kirigami.Units.largeSpacing * 2
        radius: Kirigami.Units.cornerRadius
        color: Qt.alpha(Kirigami.Theme.textColor, recommended ? 0.07 : 0.04)
        border.width: 1
        border.color: recommended ? Qt.alpha(Kirigami.Theme.highlightColor, 0.6) : Qt.alpha(Kirigami.Theme.textColor, 0.12)

        ColumnLayout {
            id: body
            anchors.fill: parent
            anchors.margins: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Kirigami.Icon {
                    Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                    Layout.preferredHeight: Layout.preferredWidth
                    source: option.iconName
                }
                PC3.Label {
                    Layout.fillWidth: true
                    text: option.title
                    font.weight: Font.DemiBold
                    wrapMode: Text.WordWrap
                }
                PC3.Label {
                    visible: option.recommended
                    text: i18n("Recommended")
                    font: Kirigami.Theme.smallFont
                    color: Kirigami.Theme.highlightColor
                }
            }

            PC3.Label {
                Layout.fillWidth: true
                text: option.text
                wrapMode: Text.WordWrap
                opacity: 0.8
            }

            PC3.Button {
                Layout.alignment: Qt.AlignRight
                text: option.actionText
                icon.name: option.actionIcon
                enabled: !option.busy
                onClicked: option.triggered()
            }
        }
    }
}
