// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! One-time codes: parsing otpauth:// URIs and generating TOTP and Steam codes.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;
use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const STEAM_ALPHABET: &[u8] = b"23456789BCDFGHJKMNPQRTVWXY";

#[derive(Debug)]
pub enum OtpError {
    Invalid(String),
    Trashed,
}

impl fmt::Display for OtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OtpError::Invalid(m) => write!(f, "{m}"),
            OtpError::Trashed => write!(f, "in the trash"),
        }
    }
}

fn invalid(msg: impl Into<String>) -> OtpError {
    OtpError::Invalid(msg.into())
}

/// One account. The secret is wiped from memory when the entry is dropped.
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Entry {
    pub id: String,
    #[serde(default)]
    pub issuer: String,
    #[serde(default)]
    pub account: String,
    pub secret: String,
    #[serde(default = "d_kind")]
    pub kind: String, // totp | steam | hotp
    #[serde(default = "d_alg")]
    pub algorithm: String,
    #[serde(default = "d_digits")]
    pub digits: u32,
    #[serde(default = "d_period")]
    pub period: u64,
    #[serde(default)]
    pub counter: u64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub position: i64,
    #[serde(default)]
    pub icon_src: String,
    #[serde(default)]
    pub icon_id: String,
}

fn d_kind() -> String {
    "totp".into()
}
fn d_alg() -> String {
    "SHA1".into()
}
fn d_digits() -> u32 {
    6
}
fn d_period() -> u64 {
    30
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("id", &self.id)
            .field("issuer", &self.issuer)
            .finish_non_exhaustive()
    }
}

impl Entry {
    #[cfg(test)]
    pub fn new(id: &str, issuer: &str, account: &str, secret: &str) -> Self {
        Entry {
            id: id.into(),
            issuer: issuer.into(),
            account: account.into(),
            secret: secret.into(),
            kind: d_kind(),
            algorithm: d_alg(),
            digits: 6,
            period: 30,
            counter: 0,
            pinned: false,
            position: 0,
            icon_src: String::new(),
            icon_id: String::new(),
        }
    }

    /// Everything except the secret.
    pub fn public(&self) -> Value {
        json!({
            "id": self.id, "issuer": self.issuer, "account": self.account, "kind": self.kind,
            "digits": self.digits, "period": self.period, "pinned": self.pinned,
        })
    }
}

/// Pinned first, then Ente's own order, then by name.
pub fn sort(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        (
            !a.pinned,
            a.position,
            a.issuer.to_lowercase(),
            a.account.to_lowercase(),
        )
            .cmp(&(
                !b.pinned,
                b.position,
                b.issuer.to_lowercase(),
                b.account.to_lowercase(),
            ))
    });
}

pub fn decode_secret(secret: &str) -> Result<Zeroizing<Vec<u8>>, OtpError> {
    let mut cleaned: Zeroizing<String> = Zeroizing::new(
        secret
            .chars()
            .filter(|c| *c != ' ' && *c != '-')
            .collect::<String>()
            .to_uppercase(),
    );
    while cleaned.ends_with('=') {
        cleaned.pop();
    }
    if cleaned.is_empty() {
        return Err(invalid("empty secret"));
    }
    let mut out = Zeroizing::new(Vec::with_capacity(cleaned.len() * 5 / 8));
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in cleaned.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'2'..=b'7' => c - b'2' + 26,
            _ => return Err(invalid("secret is not valid base32")),
        } as u32;
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    buffer.zeroize();
    Ok(out)
}

macro_rules! hmac_digest {
    ($digest:ty, $key:expr, $msg:expr) => {{
        let mut m = <Hmac<$digest> as Mac>::new_from_slice($key).expect("any key length");
        m.update($msg);
        m.finalize().into_bytes().to_vec()
    }};
}

fn hotp_int(key: &[u8], counter: u64, algorithm: &str) -> Result<u32, OtpError> {
    let msg = counter.to_be_bytes();
    let mac: Zeroizing<Vec<u8>> = Zeroizing::new(match algorithm.to_uppercase().as_str() {
        "SHA1" => hmac_digest!(sha1::Sha1, key, &msg),
        "SHA256" => hmac_digest!(sha2::Sha256, key, &msg),
        "SHA512" => hmac_digest!(sha2::Sha512, key, &msg),
        other => return Err(invalid(format!("unsupported algorithm {other}"))),
    });
    let offset = (mac[mac.len() - 1] & 0x0f) as usize;
    let v = u32::from_be_bytes([
        mac[offset],
        mac[offset + 1],
        mac[offset + 2],
        mac[offset + 3],
    ]);
    Ok(v & 0x7fff_ffff)
}

pub fn hotp(secret: &str, counter: u64, digits: u32, algorithm: &str) -> Result<String, OtpError> {
    let key = decode_secret(secret)?;
    let v = hotp_int(&key, counter, algorithm)?;
    let modulo = 10u64.pow(digits.clamp(1, 10));
    Ok(format!(
        "{:0width$}",
        v as u64 % modulo,
        width = digits as usize
    ))
}

pub fn steam(secret: &str, counter: u64) -> Result<String, OtpError> {
    let key = decode_secret(secret)?;
    let mut v = hotp_int(&key, counter, "SHA1")?;
    let mut out = String::with_capacity(5);
    for _ in 0..5 {
        out.push(STEAM_ALPHABET[(v % STEAM_ALPHABET.len() as u32) as usize] as char);
        v /= STEAM_ALPHABET.len() as u32;
    }
    Ok(out)
}

/// The current code of an entry and how long it stays valid.
pub fn code_for(e: &Entry, now: f64) -> Result<(String, u64), OtpError> {
    if e.kind == "hotp" {
        return Err(invalid("counter-based (HOTP) codes are not supported"));
    }
    let period = e.period.max(1);
    let step = (now / period as f64).floor() as u64;
    let remaining = period - (now as u64 % period);
    let code = if e.kind == "steam" {
        steam(&e.secret, step)?
    } else {
        hotp(&e.secret, step, e.digits, &e.algorithm)?
    };
    Ok((code, remaining))
}

fn stable_id(path: &str, secret: &str) -> String {
    let digest = sha2::Sha256::digest(format!("{}|{}", path, secret.to_uppercase()).as_bytes());
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Parse one otpauth:// line as written by Ente Auth's plain-text export.
pub fn parse_uri(line: &str) -> Result<Entry, OtpError> {
    let url = url::Url::parse(line.trim()).map_err(|_| invalid("not a URI"))?;
    if url.scheme() != "otpauth" {
        return Err(invalid("not an otpauth URI"));
    }
    let mut kind = url.host_str().unwrap_or("").to_lowercase();
    if !["totp", "hotp", "steam"].contains(&kind.as_str()) {
        return Err(invalid(format!("unknown code type {kind:?}")));
    }
    let mut params = std::collections::HashMap::new();
    for (k, v) in url.query_pairs() {
        params
            .entry(k.to_lowercase())
            .or_insert_with(|| v.into_owned());
    }
    let raw_path = url.path().trim_start_matches('/');
    let label = percent_decode(raw_path);
    let (issuer_label, account) = match label.split_once(':') {
        Some((i, a)) => (i.to_string(), a.to_string()),
        None => (String::new(), label.clone()),
    };
    let issuer = params
        .get("issuer")
        .cloned()
        .unwrap_or(issuer_label)
        .trim()
        .to_string();
    let secret = params.get("secret").cloned().unwrap_or_default();
    decode_secret(&secret)?;
    let algorithm = params
        .get("algorithm")
        .map(|a| a.to_uppercase())
        .unwrap_or_else(d_alg);
    if !["SHA1", "SHA256", "SHA512"].contains(&algorithm.as_str()) {
        return Err(invalid(format!("unsupported algorithm {algorithm}")));
    }
    if issuer.eq_ignore_ascii_case("steam") && kind == "totp" {
        kind = "steam".into();
    }
    let parse_num = |k: &str, d: u64| {
        params
            .get(k)
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(d)
    };
    let digits = if kind == "steam" {
        5
    } else {
        parse_num("digits", 6) as u32
    };
    let display: Value = params
        .get("codedisplay")
        .and_then(|raw| serde_json::from_str(raw).ok())
        .filter(|v: &Value| v.is_object())
        .unwrap_or(Value::Null);
    if display.get("trashed").and_then(Value::as_bool) == Some(true) {
        return Err(OtpError::Trashed);
    }
    let text = |k: &str| {
        display
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Ok(Entry {
        id: stable_id(url.path(), &secret),
        issuer,
        account: account.trim().to_string(),
        secret,
        kind,
        algorithm,
        digits,
        period: parse_num("period", 30).max(1),
        counter: parse_num("counter", 0),
        pinned: display
            .get("pinned")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        position: display.get("position").and_then(Value::as_i64).unwrap_or(0),
        icon_src: text("iconSrc"),
        icon_id: text("iconID"),
    })
}

fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={}", s.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

/// Entries from an export file, plus how many lines were unreadable.
pub fn parse_export(text: &str) -> (Vec<Entry>, usize) {
    let mut entries = Vec::new();
    let mut skipped = 0;
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("otpauth://"))
    {
        match parse_uri(line) {
            Ok(e) => entries.push(e),
            Err(OtpError::Trashed) => {}
            Err(_) => skipped += 1,
        }
    }
    sort(&mut entries);
    (entries, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_SHA1: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
    const RFC_SHA256: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZA";
    const FAKE: &str = "JBSWY3DPEHPK3PXP";

    #[test]
    fn totp_rfc6238_sha1() {
        let mut e = Entry::new("x", "", "", RFC_SHA1);
        e.digits = 8;
        for (t, want) in [
            (59.0, "94287082"),
            (1111111109.0, "07081804"),
            (2000000000.0, "69279037"),
        ] {
            assert_eq!(code_for(&e, t).unwrap().0, want);
        }
    }

    #[test]
    fn totp_rfc6238_sha256() {
        let mut e = Entry::new("x", "", "", RFC_SHA256);
        e.digits = 8;
        e.algorithm = "SHA256".into();
        assert_eq!(code_for(&e, 59.0).unwrap().0, "46119246");
    }

    #[test]
    fn remaining_seconds() {
        let e = Entry::new("x", "", "", FAKE);
        assert_eq!(code_for(&e, 60.0).unwrap().1, 30);
        assert_eq!(code_for(&e, 89.0).unwrap().1, 1);
    }

    #[test]
    fn steam_shape_and_hotp_refused() {
        let mut e = Entry::new("x", "Steam", "me", FAKE);
        e.kind = "steam".into();
        let code = code_for(&e, 1000.0).unwrap().0;
        assert!(code.len() == 5 && code.bytes().all(|c| STEAM_ALPHABET.contains(&c)));
        e.kind = "hotp".into();
        assert!(code_for(&e, 0.0).is_err());
    }

    #[test]
    fn parse_export_lines() {
        let text = [
            format!("otpauth://totp/GitHub:alice?secret={FAKE}&issuer=GitHub"),
            format!("otpauth://totp/bob%40example.com?secret={FAKE}&algorithm=SHA256&digits=8&period=60"),
            format!("otpauth://totp/Steam:gamer?secret={FAKE}&issuer=Steam"),
            "otpauth://totp/Broken?secret=not*base32".into(),
            "other".into(),
        ]
        .join("\n");
        let (entries, skipped) = parse_export(&text);
        assert_eq!(skipped, 1);
        let gh = entries.iter().find(|e| e.issuer == "GitHub").unwrap();
        assert_eq!(gh.account, "alice");
        let mail = entries.iter().find(|e| e.issuer.is_empty()).unwrap();
        assert_eq!(
            (
                mail.account.as_str(),
                mail.algorithm.as_str(),
                mail.digits,
                mail.period
            ),
            ("bob@example.com", "SHA256", 8, 60)
        );
        let st = entries.iter().find(|e| e.issuer == "Steam").unwrap();
        assert_eq!((st.kind.as_str(), st.digits), ("steam", 5));
        let again = parse_export(&text).0;
        assert_eq!(
            entries.iter().map(|e| &e.id).collect::<Vec<_>>(),
            again.iter().map(|e| &e.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn ente_display_settings() {
        let d = |extra: &str| {
            url::form_urlencoded::byte_serialize(
                format!("{{\"trashed\":false,\"pinned\":false,\"position\":0{extra}}}").as_bytes(),
            )
            .collect::<String>()
        };
        let text = [
            format!(
                "otpauth://totp/Zeta:z?secret={FAKE}&issuer=Zeta&codeDisplay={}",
                d(",\"position\":2")
            ),
            format!(
                "otpauth://totp/Alpha:a?secret={FAKE}&issuer=Alpha&codeDisplay={}",
                d(",\"position\":1")
            ),
            format!(
                "otpauth://totp/Pinned:p?secret={FAKE}&issuer=Pinned&codeDisplay={}",
                d(",\"pinned\":true,\"position\":9")
            ),
            format!(
                "otpauth://totp/Gone:g?secret={FAKE}&issuer=Gone&codeDisplay={}",
                d(",\"trashed\":true")
            ),
            format!(
                "otpauth://totp/Mine:m?secret={FAKE}&issuer=Mine&codeDisplay={}",
                d(",\"iconSrc\":\"customIcon\",\"iconID\":\"github\"")
            ),
        ]
        .join("\n");
        let (entries, skipped) = parse_export(&text);
        assert_eq!(skipped, 0);
        let names: Vec<_> = entries.iter().map(|e| e.issuer.as_str()).collect();
        assert_eq!(names, ["Pinned", "Mine", "Alpha", "Zeta"]);
        assert_eq!(
            (entries[1].icon_src.as_str(), entries[1].icon_id.as_str()),
            ("customIcon", "github")
        );
    }

    #[test]
    fn public_has_no_secret() {
        let e = Entry::new("x", "A", "b", FAKE);
        assert!(e.public().get("secret").is_none());
        assert!(!format!("{e:?}").contains(FAKE));
    }
}
