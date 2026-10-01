/*
    SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
    SPDX-License-Identifier: GPL-2.0-or-later
*/

// Runs the `ente-codes` command and keeps what it last said. The command
// talks to a background process that holds the unlocked codes; codes are
// fetched only while the popup is open.

import QtQuick
import org.kde.plasma.plasma5support as P5Support
import org.kde.plasma.workspace.dbus as DBus

Item {
    id: backend

    property string command: "ente-codes"

    // empty | locked | unlocked | missing | refused | error | unknown
    property string vaultState: "unknown"
    property int count: 0
    property var entries: []
    property real fetchedAt: 0     // ms, when `entries` were generated
    property string error: ""
    // Only the outcome of the last unlock attempt ("wrong passphrase", …).
    property string unlockError: ""
    property bool unlocking: false
    // {configured, running, last (epoch s), error} from the daemon
    property var sync: ({})

    signal copied(string id)
    signal copyFailed(string message)
    signal unlockFailed(bool wrong)

    readonly property bool unlocked: vaultState === "unlocked"

    function quote(s) {
        return "'" + String(s).replace(/'/g, "'\\''") + "'"
    }

    function refreshStatus() { _run("status", { kind: "status" }) }
    function fetchCodes() { _run("codes", { kind: "codes" }) }
    function lock() { _run("lock", { kind: "status" }) }
    function syncNow() { _run("sync", { kind: "status" }) }

    // First-time setup in a terminal (the wizard needs one for the Ente sign-in).
    function openSetup() {
        const cmd = command + " setup"
        const script = "if command -v konsole >/dev/null 2>&1; then exec konsole --hide-menubar --hide-tabbar -e " + cmd
            + "; else exec xdg-terminal-exec " + cmd + "; fi"
        const src = "sh -c " + quote(script) + " #" + Date.now()
        _jobs[src] = { kind: "launch" }
        exec.connectSource(src)
    }

    function importFile() {
        if (unlocking) return
        unlocking = true
        _run("import --gui", { kind: "unlock" })
    }

    function unlock() {
        if (unlocking) return
        unlocking = true
        _run("unlock --gui", { kind: "unlock" })
    }

    // The passphrase goes over the session bus, never into a command line.
    function unlockWith(passphrase) {
        if (unlocking) return
        unlocking = true
        unlockError = ""
        const reply = DBus.SessionBus.asyncCall({
            service: "io.github.yakupemreyerli.EnteCodes",
            path: "/io/github/yakupemreyerli/EnteCodes",
            iface: "io.github.yakupemreyerli.EnteCodes",
            member: "Unlock",
            arguments: [passphrase],
            signature: "(s)"
        })
        reply.finished.connect(() => {
            unlocking = false
            if (reply.isError) {
                // No daemon on the bus (an older one, or no session bus): ask in a dialog.
                reply.destroy()
                unlock()
                return
            }
            let value = reply.value
            if (value && value.value !== undefined) value = value.value
            reply.destroy()
            let parsed = null
            try {
                parsed = JSON.parse(String(value))
            } catch (e) {
                parsed = null
            }
            if (parsed && parsed.ok) {
                _apply(parsed, 0)
                fetchCodes()
            } else {
                unlockError = parsed ? (parsed.error || "") : ""
                unlockFailed(parsed ? parsed.wrong === true : false)
            }
        })
    }

    function copy(id) {
        _run("copy " + quote(id), { kind: "copy", id: id })
    }

    function _run(args, job) {
        const cmd = command + " --json " + args
        // A unique shell comment makes every call a new source, so the same
        // command issued twice still runs twice.
        const src = cmd + " #" + Date.now() + Math.random().toString(36).slice(2, 6)
        _jobs[src] = job
        exec.connectSource(src)
    }

    property var _jobs: ({})

    function _apply(parsed, code) {
        if (code === 127) {
            vaultState = "missing"
            error = ""
            return
        }
        if (!parsed) {
            vaultState = "error"
            return
        }
        if (parsed.sync) sync = parsed.sync
        if (parsed.ok) {
            if (parsed.state) vaultState = parsed.state
            if (parsed.count !== undefined) count = parsed.count
            error = ""
            return
        }
        error = parsed.error || ""
        // Locked or empty is a state, not an error worth showing.
        if (code === 3 || parsed.state === "locked") { vaultState = "locked"; error = "" }
        else if (code === 4 || parsed.state === "empty") { vaultState = "empty"; error = "" }
        else if (code === 5) vaultState = "refused"
        else vaultState = "error"
    }

    function _finish(source, data) {
        const job = _jobs[source] || { kind: "status" }
        delete _jobs[source]
        exec.disconnectSource(source)

        const code = data["exit code"]
        const out = String(data["stdout"] || "").trim()
        let parsed = null
        try {
            parsed = out.length ? JSON.parse(out.split("\n").pop()) : null
        } catch (e) {
            parsed = null
        }
        if (!parsed && code !== 127) {
            error = String(data["stderr"] || "").trim()
        }

        if (job.kind === "codes") {
            if (parsed && parsed.ok) {
                entries = parsed.entries
                if (parsed.sync) sync = parsed.sync
                fetchedAt = Date.now()
                count = entries.length
                vaultState = "unlocked"
                error = ""
            } else {
                entries = []
                _apply(parsed, code)
            }
            return
        }
        if (job.kind === "copy") {
            if (parsed && parsed.ok) {
                copied(job.id)
            } else {
                _apply(parsed, code)
                copyFailed(error)
            }
            return
        }
        if (job.kind === "launch") {
            refreshStatus()
            return
        }
        if (job.kind === "unlock") {
            unlocking = false
            _apply(parsed, code)
            if (vaultState === "unlocked") fetchCodes()
            return
        }
        _apply(parsed, code)
        if (!unlocked) entries = []
    }

    P5Support.DataSource {
        id: exec
        engine: "executable"
        connectedSources: []
        onNewData: (source, data) => backend._finish(source, data)
    }
}
