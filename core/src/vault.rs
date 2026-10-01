// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! The encrypted store on disk: entries sealed with a passphrase.
//!
//! scrypt turns the passphrase into a key, AES-256-GCM seals the entry list.
//! Nothing about the entries (not even their names) is stored in the clear.

use crate::otp::Entry;
use crate::paths;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

pub const FORMAT: &str = "ente-auth-plasmoid/1";
pub const SCRYPT_LOG_N: u8 = 17;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;

#[derive(Debug)]
pub enum VaultError {
    Empty,
    WrongPassphrase,
    Other(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultError::Empty => write!(f, "no codes imported yet"),
            VaultError::WrongPassphrase => write!(f, "wrong passphrase"),
            VaultError::Other(m) => write!(f, "{m}"),
        }
    }
}

fn other(m: impl std::fmt::Display) -> VaultError {
    VaultError::Other(m.to_string())
}

type Key = Zeroizing<[u8; 32]>;

/// The derived key of an unlocked vault, so it can be re-sealed after a sync
/// without asking for the passphrase again. The passphrase itself is never kept.
pub struct Sealer {
    key: Key,
    kdf: Value,
    path: PathBuf,
}

impl Sealer {
    pub fn seal(&self, entries: &[Entry]) -> Result<(), VaultError> {
        write(&self.path, &self.kdf, &self.key, entries)
    }

    /// The key's bytes, only so the daemon can pin them in RAM.
    pub fn key_bytes(&self) -> &[u8] {
        self.key.as_ref()
    }
}

pub fn exists() -> bool {
    paths::vault_path().is_file()
}

fn derive(passphrase: &str, salt: &[u8], log_n: u8, r: u32, p: u32) -> Result<Key, VaultError> {
    let params = scrypt::Params::new(log_n, r, p, 32).map_err(other)?;
    let mut key = Zeroizing::new([0u8; 32]);
    scrypt::scrypt(passphrase.as_bytes(), salt, &params, key.as_mut()).map_err(other)?;
    Ok(key)
}

pub fn save(entries: &[Entry], passphrase: &str) -> Result<Sealer, VaultError> {
    save_to(&paths::vault_path(), entries, passphrase, SCRYPT_LOG_N)
}

pub fn save_to(
    path: &Path,
    entries: &[Entry],
    passphrase: &str,
    log_n: u8,
) -> Result<Sealer, VaultError> {
    if passphrase.chars().count() < 8 {
        return Err(other("the passphrase must be at least 8 characters"));
    }
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(other)?;
    let kdf = json!({"name": "scrypt", "n": 1u64 << log_n, "r": SCRYPT_R, "p": SCRYPT_P, "salt": B64.encode(salt)});
    let key = derive(passphrase, &salt, log_n, SCRYPT_R, SCRYPT_P)?;
    write(path, &kdf, &key, entries)?;
    Ok(Sealer {
        key,
        kdf,
        path: path.to_path_buf(),
    })
}

fn write(path: &Path, kdf: &Value, key: &Key, entries: &[Entry]) -> Result<(), VaultError> {
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce).map_err(other)?;
    let plain = Zeroizing::new(serde_json::to_vec(entries).map_err(other)?);
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(other)?;
    let sealed = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plain,
                aad: FORMAT.as_bytes(),
            },
        )
        .map_err(|_| other("encryption failed"))?;
    let doc = json!({"format": FORMAT, "kdf": kdf, "nonce": B64.encode(nonce), "data": B64.encode(sealed)});
    if let Some(dir) = path.parent() {
        paths::ensure_private(dir).map_err(other)?;
    }
    let tmp = path.with_extension("tmp");
    let mut fh = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(other)?;
    fh.write_all(doc.to_string().as_bytes()).map_err(other)?;
    fh.sync_all().map_err(other)?;
    fs::rename(&tmp, path).map_err(other)?;
    Ok(())
}

pub fn open(passphrase: &str) -> Result<(Vec<Entry>, Sealer), VaultError> {
    open_at(&paths::vault_path(), passphrase)
}

pub fn open_at(path: &Path, passphrase: &str) -> Result<(Vec<Entry>, Sealer), VaultError> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(VaultError::Empty),
        Err(e) => return Err(other(format!("cannot read {}: {e}", path.display()))),
    };
    let doc: Value = serde_json::from_str(&text)
        .map_err(|e| other(format!("cannot read {}: {e}", path.display())))?;
    if doc["format"] != FORMAT {
        return Err(other("unknown vault format"));
    }
    let kdf = doc["kdf"].clone();
    let n = kdf["n"].as_u64().ok_or_else(|| other("bad vault"))?;
    if !n.is_power_of_two() || n < 2 {
        return Err(other("bad vault"));
    }
    let salt = B64
        .decode(kdf["salt"].as_str().unwrap_or(""))
        .map_err(other)?;
    let r = kdf["r"].as_u64().unwrap_or(8) as u32;
    let p = kdf["p"].as_u64().unwrap_or(1) as u32;
    let key = derive(passphrase, &salt, n.trailing_zeros() as u8, r, p)?;
    let nonce = B64
        .decode(doc["nonce"].as_str().unwrap_or(""))
        .map_err(other)?;
    let data = B64
        .decode(doc["data"].as_str().unwrap_or(""))
        .map_err(other)?;
    if nonce.len() != 12 {
        return Err(other("bad vault"));
    }
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(other)?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &data,
                    aad: FORMAT.as_bytes(),
                },
            )
            .map_err(|_| VaultError::WrongPassphrase)?,
    );
    let entries: Vec<Entry> = serde_json::from_slice(&plain).map_err(other)?;
    Ok((
        entries,
        Sealer {
            key,
            kdf,
            path: path.to_path_buf(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE: &str = "JBSWY3DPEHPK3PXP";

    #[test]
    fn roundtrip_and_wrong_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.json");
        save_to(
            &path,
            &[Entry::new("a", "GitHub", "alice", FAKE)],
            "correct horse",
            10,
        )
        .unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains(FAKE) && !raw.contains("GitHub"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let (entries, sealer) = open_at(&path, "correct horse").unwrap();
        assert_eq!(entries[0].secret, FAKE);
        assert!(matches!(
            open_at(&path, "wrong horse"),
            Err(VaultError::WrongPassphrase)
        ));
        sealer
            .seal(&[Entry::new("b", "Proton", "jane", FAKE)])
            .unwrap();
        assert_eq!(
            open_at(&path, "correct horse").unwrap().0[0].issuer,
            "Proton"
        );
    }

    #[test]
    fn short_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save_to(&dir.path().join("v.json"), &[], "short", 10).is_err());
    }
}
