#!/bin/sh
# Compiles po/*.po into the widget package, where Plasma looks for them.
# Run after editing a translation; install.sh runs it too.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
domain=plasma_applet_com.github.yakupemreyerli.enteauth
for po in "$root"/po/*.po; do
    [ -e "$po" ] || continue
    lang=$(basename "$po" .po)
    dir="$root/plasma/contents/locale/$lang/LC_MESSAGES"
    mkdir -p "$dir"
    msgfmt --check -o "$dir/$domain.mo" "$po"
    echo "built $lang"
done
