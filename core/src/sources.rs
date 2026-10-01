// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! Getting entries out of Ente Auth.
//!
//! Two ways, both ending in the same plain-text list of otpauth:// URIs: a
//! file exported from the Ente Auth app, or the official Ente CLI (`ente`),
//! whose export directory is pointed at $XDG_RUNTIME_DIR (RAM) and wiped as
//! soon as it has been read, so the decrypted list never touches the disk.

use crate::otp::{parse_export, Entry};
use crate::paths;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

#[derive(Debug)]
pub struct SourceError(pub String);

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Entries read from a source, and how many lines could not be read.
pub type Fetched = Result<(Vec<Entry>, usize), SourceError>;

fn err(m: impl Into<String>) -> SourceError {
    SourceError(m.into())
}

/// Overwrite a file with zeros before removing it.
pub fn wipe(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        if let Ok(mut fh) = fs::OpenOptions::new().write(true).open(path) {
            let _ = fh.write_all(&vec![0u8; meta.len() as usize]);
            let _ = fh.sync_all();
        }
    }
    let _ = fs::remove_file(path);
}

fn looks_encrypted(text: &str) -> bool {
    matches!(serde_json::from_str::<Value>(text), Ok(Value::Object(m)) if m.contains_key("encryptedData") || m.contains_key("kdfParams"))
}

pub fn from_text(text: &str) -> Result<(Vec<Entry>, usize), SourceError> {
    if looks_encrypted(text) {
        return Err(err("this is an encrypted Ente export: export as plain text instead, or decrypt it first with `ente auth decrypt`"));
    }
    let (entries, skipped) = parse_export(text);
    if entries.is_empty() {
        return Err(err("no otpauth:// entries found"));
    }
    Ok((entries, skipped))
}

pub fn from_file(path: &Path) -> Result<(Vec<Entry>, usize), SourceError> {
    let text = Zeroizing::new(
        fs::read_to_string(path)
            .map_err(|e| err(format!("cannot read {}: {e}", path.display())))?,
    );
    from_text(&text)
}

pub fn ente_cli() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("ente"))
        .find(|p| p.is_file())
}

fn run(binary: &Path, args: &[&str], timeout: Duration) -> Result<(bool, String), SourceError> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| err(format!("cannot run the Ente CLI: {e}")))?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| err(e.to_string()))? {
            let out = child.wait_with_output().map_err(|e| err(e.to_string()))?;
            let text = String::from_utf8_lossy(if out.stderr.is_empty() {
                &out.stdout
            } else {
                &out.stderr
            })
            .trim()
            .to_string();
            return Ok((status.success(), text.chars().take(300).collect()));
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            return Err(err("the Ente CLI took too long"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn exports(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("ente_auth") && n.ends_with(".txt"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
    files
}

pub fn from_ente_cli(email: &str) -> Result<(Vec<Entry>, usize), SourceError> {
    let binary = ente_cli().ok_or_else(|| err("the Ente CLI (`ente`) is not installed"))?;
    if email.is_empty() {
        return Err(err("no Ente account set: run `ente-codes setup` first"));
    }
    let target = paths::ensure_private(&paths::export_dir()).map_err(|e| err(e.to_string()))?;
    exports(&target).iter().for_each(|p| wipe(p));
    let dir = target.to_string_lossy().into_owned();
    let (ok, said) = run(
        &binary,
        &[
            "account", "update", "--app", "auth", "--email", email, "--dir", &dir,
        ],
        Duration::from_secs(60),
    )?;
    if !ok {
        return Err(err(format!(
            "the Ente CLI does not know this account; run `ente-codes setup`. CLI said: {said}"
        )));
    }
    let result = run(&binary, &["export"], Duration::from_secs(600));
    let files = exports(&target);
    let parsed = match (&result, files.last()) {
        (Ok((true, _)), Some(last)) => from_file(last),
        (Ok((_, said)), _) => Err(err(format!("Ente CLI export failed: {said}"))),
        (Err(e), _) => Err(err(e.0.clone())),
    };
    files.iter().for_each(|p| wipe(p));
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_export_is_explained() {
        let e = from_text(r#"{"encryptedData":"x","kdfParams":{}}"#).unwrap_err();
        assert!(e.0.contains("encrypted"));
    }

    #[test]
    fn wipe_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("ente_auth.txt");
        fs::write(&f, "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP").unwrap();
        wipe(&f);
        assert!(!f.exists());
    }
}
