#!/bin/sh
# Installs everything for the current user, no root needed:
#   - the ente-codes command (built from source with cargo) in ~/.local/bin
#   - its background process as a systemd user service
#   - the Plasma widget with kpackagetool6
#   - optionally, a passphrase dialog at login (the widget can unlock by itself)
# Usage: ./install.sh [--no-widget] [--login-unlock]
set -eu
root=$(cd "$(dirname "$0")" && pwd)
widget=1
login_unlock=0
for arg in "$@"; do
    case "$arg" in
        --no-widget) widget=0 ;;
        --login-unlock) login_unlock=1 ;;
        -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo (Rust) is needed to build ente-codes: install it with your package manager (e.g. pacman -S rust)." >&2
    exit 1
fi
if ! command -v wl-copy >/dev/null 2>&1; then
    echo "note: wl-copy (wl-clipboard) is missing, copying codes will not work until you install it." >&2
fi

# Restarting the service forgets unlocked codes, so leave an unlocked one alone.
unlocked=0
if command -v ente-codes >/dev/null 2>&1 \
    && ente-codes --json status 2>/dev/null | grep -q '"state": *"unlocked"'; then
    unlocked=1
fi

echo "==> Building the ente-codes command"
bindir="$HOME/.local/bin"
# Build first: a broken checkout never replaces a working install.
cargo build --release --locked --manifest-path "$root/core/Cargo.toml"
mkdir -p "$bindir"
# Install beside and rename, so a running daemon keeps its binary until restart.
install -m 755 "$root/core/target/release/ente-codes" "$bindir/.ente-codes.new"
mv -f "$bindir/.ente-codes.new" "$bindir/ente-codes"
command_status="installed"
if ! command -v ente-codes >/dev/null 2>&1; then
    command_status="installed in $bindir, which is not on your PATH"
fi

echo "==> Installing the background service"
units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
# The service may write only these; ReadWritePaths= fails on a missing directory.
mkdir -p "${XDG_DATA_HOME:-$HOME/.local/share}/ente-auth-plasmoid" "${XDG_CACHE_HOME:-$HOME/.cache}/ente-auth-plasmoid"
mkdir -p "$units"
cp "$root/data/ente-auth-plasmoid.service" "$units/"
restart=restart
if [ "$unlocked" = 1 ]; then
    restart=start
fi
if systemctl --user daemon-reload 2>/dev/null \
    && systemctl --user enable ente-auth-plasmoid.service >/dev/null 2>&1 \
    && systemctl --user "$restart" ente-auth-plasmoid.service 2>/dev/null; then
    service_status="running"
    if [ "$restart" = start ]; then
        service_status="running (not restarted, your codes are unlocked; the update applies after the next login)"
    fi
else
    service_status="NOT running (no systemd user session?); the widget starts it on demand"
fi

autostart="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/ente-auth-plasmoid-unlock.desktop"
if [ "$login_unlock" = 1 ]; then
    mkdir -p "$(dirname "$autostart")"
    cp "$root/data/ente-auth-plasmoid-unlock.desktop" "$autostart"
    login_status="asks for your passphrase once at login"
else
    rm -f "$autostart"
    login_status="off (type your passphrase in the widget; --login-unlock asks at login instead)"
fi

widget_status="skipped (--no-widget)"
if [ "$widget" = 1 ]; then
    if ! command -v kpackagetool6 >/dev/null 2>&1; then
        widget_status="NOT installed: kpackagetool6 not found (needs KDE Plasma 6)"
    else
        echo "==> Installing the Plasma widget"
        if command -v msgfmt >/dev/null 2>&1; then
            "$root/tools/build-translations.sh"
        fi
        if kpackagetool6 --type Plasma/Applet --upgrade "$root/plasma" 2>/dev/null \
            || kpackagetool6 --type Plasma/Applet --install "$root/plasma"; then
            widget_status="installed (after an upgrade, restart Plasma once: systemctl --user restart plasma-plasmashell)"
        else
            widget_status="NOT installed: kpackagetool6 failed"
        fi
    fi
fi

cat <<EOF

ente-codes command: $command_status
Background service: $service_status
Login unlock:       $login_status
Plasma widget:      $widget_status

Next: bring your codes in, once, in your own terminal.
  With the Ente CLI (new codes then arrive by themselves):
      ente account add          (app: auth)
      ente-codes ente-account you@example.com && ente-codes sync
  Or from the Ente Auth app: Settings > Data > Export codes > Plain text, then
      ente-codes import ~/Downloads/ente-auth-codes.txt --delete-source
Then add "Ente Auth Codes" to your panel.
EOF
