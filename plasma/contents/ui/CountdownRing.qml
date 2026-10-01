/*
    SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
    SPDX-License-Identifier: GPL-2.0-or-later
*/

// A small pie that empties as the code's time runs out.

import QtQuick
import QtQuick.Shapes
import org.kde.kirigami as Kirigami

Item {
    id: ring

    property real fraction: 1
    property bool urgent: false

    readonly property color tint: urgent ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.highlightColor
    readonly property real radius: Math.min(width, height) / 2

    Rectangle {
        anchors.fill: parent
        radius: width / 2
        color: "transparent"
        border.width: 1
        border.color: Qt.alpha(Kirigami.Theme.textColor, 0.25)
    }

    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            fillColor: ring.tint
            strokeColor: "transparent"
            startX: ring.radius
            startY: ring.radius
            PathLine { x: ring.radius; y: ring.radius * 0.2 }
            PathAngleArc {
                centerX: ring.radius
                centerY: ring.radius
                radiusX: ring.radius * 0.8
                radiusY: ring.radius * 0.8
                startAngle: -90
                sweepAngle: 360 * Math.max(0, Math.min(1, ring.fraction))
            }
            PathLine { x: ring.radius; y: ring.radius }
        }
    }
}
