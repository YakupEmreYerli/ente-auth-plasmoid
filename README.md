# Ente Auth Codes for KDE Plasma

[![CI](https://github.com/YakupEmreYerli/ente-auth-plasmoid/actions/workflows/ci.yml/badge.svg)](https://github.com/YakupEmreYerli/ente-auth-plasmoid/actions/workflows/ci.yml) [![License: GPL-2.0-or-later](https://img.shields.io/badge/license-GPL--2.0--or--later-1b1d2a)](LICENSE) [![KDE Plasma 6](https://img.shields.io/badge/KDE%20Plasma-6-ADD5FF?logo=kde&logoColor=white)](https://kde.org/plasma-desktop/) [![Rust](https://img.shields.io/badge/core-Rust-b7410e?logo=rust&logoColor=white)](core)

Your [Ente Auth](https://ente.io/auth/) two-factor codes in the Plasma panel: click the icon, type a few letters,
press Enter, and the code is on your clipboard.

> Türkçe: [README.tr.md](README.tr.md)

![Ente Auth Codes: the panel popup with a searchable list of accounts, their logos, codes and countdowns](docs/banner.png)

- **Search and copy.** Type to filter, Enter or a click copies the code; the popup closes so you can paste.
- **Stays in sync.** Add a code on your phone and it shows up here: while unlocked, the widget fetches changes
  from Ente every 10 minutes (and when you press the sync button) through the official Ente CLI.
- **Service logos.** The same logos Ente Auth shows, plus Simple Icons for the rest. Both sets are downloaded
  whole, once, so no request ever names one of your services.
- **Locked until you unlock.** Your codes are stored encrypted on disk (scrypt + AES-256-GCM) and decrypted only
  into the memory of a small background process, after you type your passphrase in the widget. The passphrase
  travels over D-Bus, never through a command line.
- **Clipboard hygiene.** Copied codes are marked sensitive (Klipper does not keep them in its history) and are
  cleared after 30 seconds if they are still there.

## Install

Needs KDE Plasma 6, Rust (`cargo`) to build, and `wl-clipboard`.

```sh
git clone https://github.com/YakupEmreYerli/ente-auth-plasmoid
cd ente-auth-plasmoid
./install.sh
```

This installs the `ente-codes` command, its background service (`systemctl --user status ente-auth-plasmoid`)
and the widget. You unlock by typing your passphrase in the widget; `--login-unlock` asks once at login instead. Then right-click the
panel, choose *Add Widgets…* and add **Ente Auth Codes**.

## Bring your codes in

Open the widget: the first time it shows a short guide with two buttons. **Connect…** opens a terminal with
`ente-codes setup`, which signs the Ente CLI in for you (you type only your Ente e-mail, password and
verification code), downloads the codes and asks for a passphrase. **Choose file…** imports an export file.

<p><img src="docs/screenshots/empty.png" width="280" alt="First-run guide with two ways to bring the codes in"> <img src="docs/screenshots/locked.png" width="280" alt="Locked: a passphrase field in the popup"></p>

To do the same by hand, run these in your own terminal. You choose a passphrase the first time; it protects the codes on this computer
and is separate from your Ente password.

**From the Ente Auth app** (simplest): *Settings → Data → Export codes → Plain text*, save the file, then

```sh
ente-codes import ~/Downloads/ente-auth-codes.txt --delete-source
```

`--delete-source` overwrites and removes the plain-text file afterwards.

**With the official [Ente CLI](https://github.com/ente-io/ente/tree/main/cli)** (recommended: new codes then
arrive by themselves):

```sh
ente account add                       # once; choose the "auth" app, any export directory
ente-codes ente-account you@example.com
ente-codes sync                        # first time: choose your passphrase
```

From then on the background process syncs by itself while unlocked (`ente-codes settings --sync-minutes N`).
Each sync points the CLI's export at `$XDG_RUNTIME_DIR` (RAM), reads the decrypted list and wipes it at once,
so it never touches the disk; the vault is re-sealed with the key derived at unlock (the passphrase itself is
not kept).

## The command

```text
ente-codes setup               first-time wizard (Ente sign-in, download, passphrase)
ente-codes status              locked, unlocked or empty
ente-codes unlock [--gui]      type your passphrase
ente-codes lock                forget the codes until the next unlock
ente-codes codes               current codes (terminal only)
ente-codes copy github         copy one code to the clipboard
ente-codes list                entry names, no codes
ente-codes passwd              change the passphrase
ente-codes sync                sync with Ente now
ente-codes icons [--force]     download the logo sets again
ente-codes settings --clear-seconds 30 --lock-minutes 15 --sync-minutes 10
```

Add `--json` for machine-readable output. Exit codes: 3 locked, 4 nothing imported, 5 refused.

## How it protects your codes

| Where | What is there |
| --- | --- |
| `~/.local/share/ente-auth-plasmoid/vault.json` | Encrypted entries (mode 0600). Nothing readable, not even account names. |
| Background process | Decrypted entries while unlocked, wiped from memory when locked (`zeroize`). The pages holding secrets are pinned in RAM (`mlock`), core dumps are off, and the process is not ptrace-able or readable through `/proc` by your other programs. |
| `$XDG_RUNTIME_DIR/ente-auth-plasmoid/daemon.sock` | The socket (mode 0600). Each request is checked against the caller's real process tree, not anything it claims. |

## Development

The core (`core/`, Rust) is the `ente-codes` command and its background process; the widget (`plasma/`, QML)
only runs that command.

```sh
cargo test --manifest-path core/Cargo.toml
tools/preview.py /tmp/preview     # renders the popup off-screen with made-up entries
tools/update-translations.sh      # after changing any i18n() text
```

Tests use only public RFC 6238 test vectors and made-up secrets.

## Credits

- [Ente Auth](https://github.com/ente-io/ente) and its CLI do the real work of keeping your codes; this widget reads
  them. Its custom icon set (AGPL-3.0) is downloaded to your cache, not shipped here.
- [Simple Icons](https://simpleicons.org) (CC0) supplies the other logos. Brand names and logos belong to their owners.

## License

GPL-2.0-or-later. Not affiliated with Ente.
