// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! ente-codes: Ente Auth codes for the KDE Plasma panel, locked in memory.

mod cli;
mod client;
mod daemon;
mod dbus;
mod guard;
mod icons;
mod otp;
mod paths;
mod settings;
mod setup;
mod sources;
mod vault;

fn main() {
    std::process::exit(cli::main());
}
