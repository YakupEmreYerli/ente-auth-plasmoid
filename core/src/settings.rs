// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! Small user settings, read on every use so edits apply without a restart.

use crate::paths;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub clear_clipboard_seconds: u64,
    /// 0: stay unlocked until you lock or log out.
    pub lock_after_minutes: u64,
    /// How often to fetch changes from Ente; 0: only on unlock.
    pub sync_minutes: u64,
    /// Account the Ente CLI syncs from.
    pub ente_email: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            clear_clipboard_seconds: 30,
            lock_after_minutes: 0,
            sync_minutes: 10,
            ente_email: String::new(),
        }
    }
}

/// Unknown or invalid values fall back to their defaults one by one.
pub fn load() -> Settings {
    let mut s = Settings::default();
    let Ok(text) = fs::read_to_string(paths::settings_path()) else {
        return s;
    };
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) else {
        return s;
    };
    let num = |k: &str| map.get(k).and_then(Value::as_u64);
    if let Some(v) = num("clear_clipboard_seconds") {
        s.clear_clipboard_seconds = v;
    }
    if let Some(v) = num("lock_after_minutes") {
        s.lock_after_minutes = v;
    }
    if let Some(v) = num("sync_minutes") {
        s.sync_minutes = v;
    }
    if let Some(v) = map.get("ente_email").and_then(Value::as_str) {
        s.ente_email = v.to_string();
    }
    s
}

pub fn save(s: &Settings) -> std::io::Result<()> {
    let path = paths::settings_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, serde_json::to_string_pretty(s).unwrap() + "\n")
}
