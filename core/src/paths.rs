// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! Where things live. ENTE_CODES_HOME moves all of it (tests, portable use).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const APP: &str = "ente-auth-plasmoid";

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn override_home() -> Option<PathBuf> {
    std::env::var_os("ENTE_CODES_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn xdg(var: &str, fallback: &[&str], sub: &str) -> PathBuf {
    if let Some(root) = override_home() {
        return root.join(sub);
    }
    let base = std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback.iter().fold(home(), |p, part| p.join(part)));
    base.join(APP)
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", &[".local", "share"], "data")
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", &[".config"], "config")
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", &[".cache"], "cache")
}

/// tmpfs, private to the user: the socket and the short-lived Ente export.
pub fn runtime_dir() -> PathBuf {
    if let Some(root) = override_home() {
        return root.join("run");
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    base.join(APP)
}

pub fn vault_path() -> PathBuf {
    data_dir().join("vault.json")
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn socket_path() -> PathBuf {
    runtime_dir().join("daemon.sock")
}

pub fn export_dir() -> PathBuf {
    runtime_dir().join("export")
}

pub fn ensure_private(path: &Path) -> std::io::Result<PathBuf> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(path.to_path_buf())
}

/// Tests that change ENTE_CODES_HOME take this first: the environment is shared.
#[cfg(test)]
pub static TEST_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());
