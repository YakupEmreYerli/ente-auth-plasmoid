# Contributing

Thanks for helping. Issues and pull requests are both welcome; for anything larger than a fix, open an issue
first so we can agree on the shape.

## Set up

```bash
git clone https://github.com/YakupEmreYerli/ente-auth-plasmoid.git && cd ente-auth-plasmoid
cargo test --manifest-path core/Cargo.toml
./install.sh        # builds the core, installs the service and the widget
```

The tests use public RFC 6238 vectors and made-up secrets; they never need an Ente account. Plasma caches
QML, so after a widget change reopen the popup, or restart Plasma: `systemctl --user restart plasma-plasmashell`.

## Before sending

```bash
cargo fmt --manifest-path core/Cargo.toml --check
cargo clippy --manifest-path core/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path core/Cargo.toml
shellcheck install.sh tools/*.sh
tools/preview.py /tmp/shots        # renders the popup off-screen; look at the PNGs
```

`tools/preview.py` needs the system PySide6 (the same Qt as Plasma), not one from pip, which ships its own Qt
and cannot load Plasma's QML modules. With `--icons DIR` it asks the core for logos from `DIR`'s icon cache.

## Things worth knowing

- **Never test with real codes.** Use made-up base32 secrets (`JBSWY3DPEHPK3PXP`) and `ENTE_CODES_HOME=/tmp/x`,
  which moves the vault, settings, cache and socket out of your real ones.
- **The caller is checked, not trusted.** `core/src/guard.rs` decides from the peer's real process tree
  (`SO_PEERCRED` on the socket, the bus-reported pid on D-Bus). Requests are sorted into harmless (`status`,
  `lock`), open (`unlock`, `sync`) and guarded (`list`, `codes`, `copy`) in `daemon.rs`; a new request needs a
  place in one of those lists.
- **Secrets are wiped, not just dropped.** `Entry` zeroizes on drop, passphrases travel in `Zeroizing` buffers,
  and `lock_in_ram` pins the pages holding secrets. Keep new secret-bearing types on the same path.
- **Locking the whole process does not work.** `mlockall(MCL_FUTURE)` also locks every thread stack and runs
  into the usual 8 MB limit, after which threads cannot start. Only secret pages are locked.
- **The passphrase never goes through a command line.** The widget sends it over D-Bus (`dbus.rs`); the dialog
  path (`unlock --gui`) is only a fallback.
- **The vault format is fixed** (`ente-auth-plasmoid/1`: scrypt, AES-256-GCM with the format as associated
  data). Change it only with a migration and a test that opens the old format.
- **Restarting the daemon locks the codes.** `install.sh` leaves an unlocked daemon running.
- **Widget settings are passed in, not read.** `FullRepresentation` takes `cfg` as a property so previews can
  render it with a stand-in.
- **No shell interpolation of user input in QML.** Entry ids go through `quote()` in `Backend.qml`.

## Translations

Strings live in the QML as `i18n("...")`. After changing them run `tools/update-translations.sh`. To add a
language, copy `po/ente-auth-plasmoid.pot` to `po/<lang>.po`, translate, and run `tools/build-translations.sh`
(install.sh does this too). The setup wizard's terminal text lives in `core/src/setup.rs` (`t(en, tr)`).

## Style

- Rust: `cargo fmt`, clippy clean, no `unwrap` on input from outside the process.
- QML: KDE's conventions (Kirigami units for sizes, Plasma components, theme colours).
- Commits: `feat:`, `fix:`, `docs:`, `test:`, `chore:`, imperative, with the *why* in the body.
