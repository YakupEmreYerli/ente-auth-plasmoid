#!/usr/bin/python3
"""Render the widget's popup to PNG files without opening a window.

The popup is drawn off-screen (QT_QPA_PLATFORM=offscreen) against a fake
backend with made-up entries and codes, so screenshots never involve real
accounts and never steal focus on the desktop. Needs the system PySide6.

    tools/preview.py OUTDIR [--scale 2] [--states unlocked locked empty]
                            [--icons DIR]   (an ENTE_CODES_HOME whose icon cache is filled)
"""

import argparse
import os
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UI = ROOT / "plasma" / "contents" / "ui"

# Made-up: names of well-known services, random codes, no secrets anywhere.
ENTRIES = [
    {"id": "1", "issuer": "GitHub", "account": "octocat", "code": "482913", "remaining": 21, "period": 30},
    {"id": "2", "issuer": "Google", "account": "jane.doe@gmail.com", "code": "305774", "remaining": 21, "period": 30},
    {"id": "3", "issuer": "Proton", "account": "jane@proton.me", "code": "917260", "remaining": 21, "period": 30},
    {"id": "4", "issuer": "Cloudflare", "account": "jane@example.com", "code": "064318", "remaining": 21, "period": 30},
    {"id": "5", "issuer": "Discord", "account": "jane", "code": "750291", "remaining": 4, "period": 30},
    {"id": "6", "issuer": "Mozilla", "account": "jane@example.com", "code": "228406", "remaining": 21, "period": 30},
    {"id": "7", "issuer": "Steam", "account": "jane_plays", "code": "K7RNB", "remaining": 21, "period": 30},
    {"id": "8", "issuer": "Corner Bakery", "account": "jane", "code": "551208", "remaining": 21, "period": 30},
]

STATES = {
    "unlocked": {"vaultState": "unlocked", "entries": ENTRIES},
    "search": {"vaultState": "unlocked", "entries": ENTRIES, "search": "goo"},
    "locked": {"vaultState": "locked", "entries": []},
    "empty": {"vaultState": "empty", "entries": []},
}

WRAPPER = """
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.ksvg as KSvg
import "file://%(ui)s" as Ui

Item {
    id: stage
    width: frame.width + 2 * pad
    height: frame.height + 2 * pad
    readonly property int pad: Kirigami.Units.gridUnit

    QtObject {
        id: fake
        property string vaultState: previewState.vaultState
        property var entries: previewState.entries
        property real fetchedAt: Date.now()
        property int count: entries.length
        property string error: ""
        property string unlockError: ""
        property bool unlocking: false
        property var sync: ({ configured: true, running: false, last: Date.now() / 1000 - 180, error: "", cli: true })
        readonly property bool unlocked: vaultState === "unlocked"
        signal copied(string id)
        signal copyFailed(string message)
        signal unlockFailed(bool wrong)
        function unlockWith(p) {}
        function fetchCodes() {}
        function refreshStatus() {}
        function lock() {}
        function syncNow() {}
        function openSetup() {}
        function importFile() {}
        function unlock() {}
        function copy(id) {}
    }
    QtObject {
        id: fakeRoot
        property bool expanded: true
    }

    KSvg.FrameSvgItem {
        id: frame
        x: stage.pad
        y: stage.pad
        imagePath: "dialogs/background"
        clip: true
        width: popup.Layout.preferredWidth + margins.left + margins.right
        height: popup.Layout.preferredHeight + margins.top + margins.bottom

        Ui.FullRepresentation {
            id: popup
            anchors.fill: parent
            anchors.leftMargin: frame.margins.left
            anchors.rightMargin: frame.margins.right
            anchors.topMargin: frame.margins.top
            anchors.bottomMargin: frame.margins.bottom
            backend: fake
            root: fakeRoot
            cfg: ({ closeOnCopy: true })
            Component.onCompleted: {
                if (previewState.search) {
                    const field = findSearch(popup)
                    if (field) field.text = previewState.search
                }
            }
            function findSearch(item) {
                if (item.placeholderText !== undefined && item.text !== undefined) return item
                for (let i = 0; i < item.children.length; i++) {
                    const hit = findSearch(item.children[i])
                    if (hit) return hit
                }
                return null
            }
        }
    }
}
"""


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("outdir", type=Path)
    ap.add_argument("--scale", default="2")
    ap.add_argument("--states", nargs="*", default=list(STATES))
    ap.add_argument("--suffix", default="")
    ap.add_argument("--icons", type=Path, help="ENTE_CODES_HOME with a downloaded icon cache")
    args = ap.parse_args()

    if args.icons:
        # Same lookup as the real widget: ask the core, against a filled icon cache.
        import json
        import subprocess

        binary = ROOT / "core" / "target" / "release" / "ente-codes"
        env = dict(os.environ, ENTE_CODES_HOME=str(args.icons))
        for entry in ENTRIES:
            out = subprocess.run([str(binary), "--json", "icon", entry["issuer"]], env=env,
                                 capture_output=True, text=True).stdout
            hit = json.loads(out) if out.strip() else {}
            entry["icon"], entry["tint"] = hit.get("icon", ""), hit.get("tint", "")

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    os.environ.setdefault("QT_QPA_PLATFORMTHEME", "kde")
    os.environ.setdefault("QT_QUICK_BACKEND", "software")
    os.environ["QT_FORCE_STDERR_LOGGING"] = "1"
    os.environ["QT_SCALE_FACTOR"] = args.scale

    from PySide6.QtCore import QObject, QTimer, QUrl, Slot
    from PySide6.QtGui import QColor, QGuiApplication
    from PySide6.QtQuick import QQuickView

    class Ki18n(QObject):
        """Stands in for KLocalizedContext: English text, %1..%n filled in."""

        @staticmethod
        def _fill(text, *subs):
            for i, sub in enumerate(subs, 1):
                if isinstance(sub, float) and sub.is_integer():
                    sub = int(sub)
                text = text.replace(f"%{i}", str(sub))
            return text

        @Slot(str, result=str)
        @Slot(str, "QVariant", result=str)
        def i18n(self, text, *subs):
            return self._fill(text, *subs)

        @Slot(str, str, result=str)
        @Slot(str, str, "QVariant", result=str)
        def i18nd(self, _domain, text, *subs):
            return self._fill(text, *subs)

        @Slot(str, str, "QVariant", result=str)
        def i18np(self, one, many, n):
            return self._fill(one if int(n) == 1 else many, n)

    app = QGuiApplication(sys.argv)
    args.outdir.mkdir(parents=True, exist_ok=True)
    wrapper = args.outdir / ".preview.qml"
    wrapper.write_text(WRAPPER % {"ui": UI})
    ki18n = Ki18n()
    queue = list(args.states)
    failures = []

    def render_next():
        if not queue:
            wrapper.unlink(missing_ok=True)
            app.exit(1 if failures else 0)
            return
        name = queue.pop(0)
        view = QQuickView()
        view.setColor(QColor(0, 0, 0, 0))
        ctx = view.rootContext()
        ctx.setContextObject(ki18n)
        ctx.setContextProperty("previewState", STATES[name])
        view.setSource(QUrl.fromLocalFile(str(wrapper)))
        if view.status() != QQuickView.Status.Ready:
            for err in view.errors():
                print(err.toString(), file=sys.stderr)
            failures.append(name)
            QTimer.singleShot(0, render_next)
            return
        view.show()

        def fit():
            root = view.rootObject()
            view.resize(int(root.width()), int(root.height()))
            QTimer.singleShot(400, grab)

        def grab():
            out = args.outdir / f"{name}{args.suffix}.png"
            view.grabWindow().save(str(out))
            print(out)
            view.close()
            view.deleteLater()
            render_next()

        QTimer.singleShot(700, fit)

    QTimer.singleShot(0, render_next)
    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
