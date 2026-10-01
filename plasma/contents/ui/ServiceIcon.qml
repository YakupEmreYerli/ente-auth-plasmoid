/*
    SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
    SPDX-License-Identifier: GPL-2.0-or-later
*/

// A service's logo: a full-colour image, a one-colour mark in its brand
// colour (on a light disc when that colour would vanish on this background),
// or the first letter on a coloured disc when there is no logo.

import QtQuick
import org.kde.kirigami as Kirigami

Item {
    id: icon

    property string source: ""     // absolute path of an SVG, or ""
    property string tint: ""       // "#rrggbb" for one-colour marks, "" for full colour
    property string name: ""

    function luminance(c) {
        const f = v => v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)
        return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    }
    function contrast(a, b) {
        const la = luminance(a), lb = luminance(b)
        return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
    }

    readonly property bool needsBackdrop: tint.length > 0
        && contrast(Qt.color(tint), Kirigami.Theme.backgroundColor) < 2.2

    Rectangle {
        anchors.fill: parent
        visible: icon.source.length > 0 && icon.needsBackdrop
        radius: width / 2
        color: "#f2f2f2"
    }

    Image {
        anchors.fill: parent
        anchors.margins: icon.needsBackdrop ? parent.width * 0.18 : 0
        visible: icon.source.length > 0
        source: icon.source.length > 0 ? "file://" + icon.source : ""
        sourceSize.width: width * 2
        sourceSize.height: height * 2
        fillMode: Image.PreserveAspectFit
        smooth: true
        asynchronous: true
    }

    Rectangle {
        anchors.fill: parent
        visible: icon.source.length === 0
        radius: width / 2
        // A stable colour per name, softened to sit in any theme.
        color: Qt.hsla((Array.from(icon.name).reduce((h, ch) => (h * 31 + ch.charCodeAt(0)) % 360, 7)) / 360, 0.45, 0.45, 1)

        Text {
            anchors.centerIn: parent
            text: (icon.name.trim()[0] || "?").toLocaleUpperCase()
            color: "white"
            font.pixelSize: parent.height * 0.5
            font.weight: Font.DemiBold
        }
    }
}
