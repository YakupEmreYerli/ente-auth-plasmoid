// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! Service logos, matched the way Ente Auth matches them.
//!
//! Two icon sets are downloaded whole, once, into the user's cache, so no
//! request ever names one of your services: Ente Auth's own custom icons
//! (AGPL-3.0, which is why they are fetched on your machine instead of shipped
//! here) and Simple Icons (CC0). Only issuer names are looked at here.

use crate::paths;
use regex::Regex;
use serde_json::Value;
use sha2::Digest;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use unicode_normalization::UnicodeNormalization;

const ENTE_PREFIX: &str = "mobile/apps/auth/assets/custom-icons/";
const ENTE_RAW: &str = "https://raw.githubusercontent.com/ente-io/ente/main/";
const SIMPLE_META: &str = "https://registry.npmjs.org/simple-icons/latest";
const REFRESH: Duration = Duration::from_secs(14 * 24 * 3600);
const USER_AGENT: &str =
    "ente-auth-plasmoid (+https://github.com/YakupEmreYerli/ente-auth-plasmoid)";

#[derive(Debug, Clone, PartialEq)]
pub struct Icon {
    pub path: String,
    /// "#rrggbb" for a one-colour mark (already painted), "" for full colour.
    pub tint: String,
}

pub fn icons_dir() -> PathBuf {
    paths::cache_dir().join("icons")
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(120))
        .user_agent(USER_AGENT)
        .build()
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    let resp = agent().get(url).call().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(64 << 20)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Lower case, no accents, letters and digits only: "Proton Mail" -> "protonmail".
pub fn normalize(name: &str) -> String {
    name.nfkd()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Simple Icons' own titleToSlug.
pub fn simple_slug(title: &str) -> String {
    let mut t = title
        .to_lowercase()
        .replace('+', "plus")
        .replace('.', "dot")
        .replace('&', "and");
    for (a, b) in [
        ("đ", "d"),
        ("ħ", "h"),
        ("ı", "i"),
        ("ĸ", "k"),
        ("ŀ", "l"),
        ("ł", "l"),
        ("ß", "ss"),
        ("ŧ", "t"),
    ] {
        t = t.replace(a, b);
    }
    t.nfd()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

// ---------------------------------------------------------------- download

fn download_ente(target: &Path) -> Result<usize, String> {
    // The directory listing (one request, under 1000 files) names every icon.
    let list = get(&format!(
        "https://api.github.com/repos/ente-io/ente/contents/{ENTE_PREFIX}icons?ref=main"
    ))?;
    let list: Value = serde_json::from_slice(&list).map_err(|e| e.to_string())?;
    let mut files: Vec<String> = list
        .as_array()
        .ok_or("unexpected icon listing")?
        .iter()
        .filter_map(|i| i["path"].as_str())
        .filter(|p| p.ends_with(".svg"))
        .map(String::from)
        .collect();
    if files.is_empty() {
        return Err("Ente icon list is empty".into());
    }
    files.push(format!("{ENTE_PREFIX}_data/custom-icons.json"));
    fs::create_dir_all(target.join("icons")).map_err(|e| e.to_string())?;
    fs::create_dir_all(target.join("_data")).map_err(|e| e.to_string())?;
    let queue = Mutex::new(files.clone());
    let failure = Mutex::new(None::<String>);
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| loop {
                let Some(path) = queue.lock().unwrap().pop() else {
                    break;
                };
                let rel = &path[ENTE_PREFIX.len()..];
                match get(&format!("{ENTE_RAW}{path}"))
                    .and_then(|b| fs::write(target.join(rel), b).map_err(|e| e.to_string()))
                {
                    Ok(()) => {}
                    Err(e) => *failure.lock().unwrap() = Some(e),
                }
            });
        }
    });
    match failure.into_inner().unwrap() {
        Some(e) => Err(e),
        None => Ok(files.len()),
    }
}

fn download_simple(target: &Path) -> Result<usize, String> {
    let meta: Value = serde_json::from_slice(&get(SIMPLE_META)?).map_err(|e| e.to_string())?;
    let tarball = meta["dist"]["tarball"].as_str().ok_or("no tarball")?;
    let blob = get(tarball)?;
    fs::create_dir_all(target.join("icons")).map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&blob[..]));
    let mut count = 0;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let name = entry
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        let dest = if name == "package/data/simple-icons.json" {
            target.join("simple-icons.json")
        } else if let Some(file) = name
            .strip_prefix("package/icons/")
            .filter(|f| f.ends_with(".svg") && !f.contains('/'))
        {
            count += 1;
            target.join("icons").join(file)
        } else {
            continue;
        };
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        fs::write(dest, buf).map_err(|e| e.to_string())?;
    }
    Ok(count)
}

fn clear(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

/// Download both sets if missing or older than two weeks.
pub fn update(force: bool) -> Vec<(String, String)> {
    let root = icons_dir();
    let mut result = Vec::new();
    type Fetch = fn(&Path) -> Result<usize, String>;
    for (name, fetch) in [
        ("ente", download_ente as Fetch),
        ("simple", download_simple as Fetch),
    ] {
        let target = root.join(name);
        let fresh = fs::metadata(target.join(".complete"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < REFRESH);
        if fresh && !force {
            result.push((name.into(), "cached".into()));
            continue;
        }
        let staging = root.join(format!("{name}.new"));
        clear(&staging);
        if let Err(e) = fs::create_dir_all(&staging) {
            result.push((name.into(), format!("failed: {e}")));
            continue;
        }
        match fetch(&staging) {
            Ok(n) => {
                let _ = fs::write(staging.join(".complete"), "ok");
                clear(&target);
                let _ = fs::rename(&staging, &target);
                result.push((name.into(), n.to_string()));
            }
            Err(e) => {
                clear(&staging);
                result.push((name.into(), format!("failed: {e}")));
            }
        }
    }
    *index_cell().lock().unwrap() = None;
    result
}

// ---------------------------------------------------------------- lookup

#[derive(Default)]
struct Index {
    ente: HashMap<String, Icon>,
    ente_file: HashMap<String, Icon>,
    simple: HashMap<String, Icon>,
    stamp: Option<SystemTime>,
}

fn index_cell() -> &'static Mutex<Option<Index>> {
    static CELL: OnceLock<Mutex<Option<Index>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

fn stamp() -> Option<SystemTime> {
    ["ente", "simple"]
        .iter()
        .filter_map(|n| {
            fs::metadata(icons_dir().join(n).join(".complete"))
                .and_then(|m| m.modified())
                .ok()
        })
        .max()
}

fn build_index() -> Index {
    let mut idx = Index {
        stamp: stamp(),
        ..Default::default()
    };
    let root = icons_dir().join("ente");
    let data: Value = fs::read_to_string(root.join("_data/custom-icons.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    for item in data["icons"].as_array().into_iter().flatten() {
        let title = item["title"].as_str().unwrap_or("");
        let slug = item["slug"].as_str();
        let file = slug
            .map(String::from)
            .unwrap_or_else(|| title.to_lowercase().replace(' ', ""));
        let path = root.join("icons").join(format!("{file}.svg"));
        if !path.exists() {
            continue;
        }
        let tint = item["hex"]
            .as_str()
            .or(item["color"].as_str())
            .map(|h| format!("#{h}"))
            .unwrap_or_default();
        let icon = Icon {
            path: path.to_string_lossy().into_owned(),
            tint,
        };
        idx.ente_file.insert(file, icon.clone());
        let mut names = vec![
            title,
            slug.unwrap_or(""),
            item["name"].as_str().unwrap_or(""),
        ];
        names.extend(
            item["altNames"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        );
        for n in names {
            let key = normalize(n);
            if !key.is_empty() {
                idx.ente.entry(key).or_insert_with(|| icon.clone());
            }
        }
    }
    let root = icons_dir().join("simple");
    let data: Value = fs::read_to_string(root.join("simple-icons.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    let list = data.as_array().or_else(|| data["icons"].as_array());
    for item in list.into_iter().flatten() {
        let title = item["title"].as_str().unwrap_or("");
        let slug = item["slug"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| simple_slug(title));
        let path = root.join("icons").join(format!("{slug}.svg"));
        if !path.exists() {
            continue;
        }
        let icon = Icon {
            path: path.to_string_lossy().into_owned(),
            tint: format!("#{}", item["hex"].as_str().unwrap_or("000000")),
        };
        let mut names = vec![title.to_string(), slug.clone()];
        for (_, v) in item["aliases"].as_object().into_iter().flatten() {
            for a in v.as_array().into_iter().flatten() {
                names.extend(
                    a.as_str()
                        .map(String::from)
                        .or_else(|| a["title"].as_str().map(String::from)),
                );
            }
        }
        for n in names {
            let key = normalize(&n);
            if !key.is_empty() {
                idx.simple.entry(key).or_insert_with(|| icon.clone());
            }
        }
    }
    idx
}

fn with_index<T>(f: impl FnOnce(&Index) -> T) -> T {
    let mut guard = index_cell().lock().unwrap();
    let current = stamp();
    if guard.as_ref().map(|i| i.stamp != current).unwrap_or(true) {
        *guard = Some(build_index());
    }
    f(guard.as_ref().unwrap())
}

fn lookup(issuer: &str, icon_src: &str, icon_id: &str) -> Option<Icon> {
    with_index(|idx| {
        let find = |key: &str| idx.ente.get(key).or_else(|| idx.simple.get(key)).cloned();
        // An icon chosen by hand in Ente Auth wins.
        if !icon_id.is_empty() {
            let hit = if icon_src == "simpleIcon" {
                idx.simple.get(&normalize(icon_id)).cloned()
            } else {
                idx.ente_file
                    .get(icon_id)
                    .or_else(|| idx.ente.get(&normalize(icon_id)))
                    .cloned()
            };
            if hit.is_some() {
                return hit;
            }
        }
        let key = normalize(issuer);
        if key.is_empty() {
            return None;
        }
        if let Some(hit) = find(&key) {
            return Some(hit);
        }
        // "Google Workspace", "github.com": the first word, then without a TLD.
        let first = issuer
            .trim()
            .split(|c: char| c.is_whitespace() || "(:@/-".contains(c))
            .next()
            .unwrap_or("");
        let tld = Regex::new(r"\.(com|net|org|io|dev|app|co)$").unwrap();
        let bare = tld.replace(&issuer.to_lowercase(), "").into_owned();
        [first.to_string(), bare]
            .iter()
            .map(|c| normalize(c))
            .filter(|k| k.len() > 2)
            .find_map(|k| find(&k))
    })
}

/// Paint every fill and stroke of a one-colour mark in `color`.
pub fn tint_svg(text: &str, color: &str) -> String {
    let attr = Regex::new(r#"(fill|stroke)="([^"]*)""#).unwrap();
    let text = attr.replace_all(text, |c: &regex::Captures| {
        if &c[2] == "none" {
            c[0].to_string()
        } else {
            format!("{}=\"{color}\"", &c[1])
        }
    });
    let style = Regex::new(r#"(fill|stroke)\s*:\s*([^;"']+)"#).unwrap();
    let text = style.replace_all(&text, |c: &regex::Captures| {
        if c[2].trim() == "none" {
            c[0].to_string()
        } else {
            format!("{}:{color}", &c[1])
        }
    });
    let root = Regex::new(r"<svg\b[^>]*>").unwrap();
    if let Some(m) = root.find(&text) {
        let tag = m.as_str();
        if !tag.contains("fill=") {
            let closing = if tag.ends_with("/>") { "/>" } else { ">" };
            let head = tag[..tag.len() - closing.len()].trim_end();
            return text.replacen(tag, &format!("{head} fill=\"{color}\"{closing}"), 1);
        }
    }
    text.into_owned()
}

fn tinted(path: &str, color: &str) -> std::io::Result<String> {
    let key: String = sha2::Sha256::digest(format!("{path}|{color}").as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect();
    let out = icons_dir().join("tinted").join(format!("{key}.svg"));
    if !out.exists() {
        fs::create_dir_all(out.parent().unwrap())?;
        fs::write(&out, tint_svg(&fs::read_to_string(path)?, color))?;
    }
    Ok(out.to_string_lossy().into_owned())
}

/// The logo for an issuer. With a tint, `path` is already painted in it.
pub fn resolve(issuer: &str, icon_src: &str, icon_id: &str) -> Option<Icon> {
    let hit = lookup(issuer, icon_src, icon_id)?;
    if hit.tint.is_empty() {
        return Some(hit);
    }
    Some(match tinted(&hit.path, &hit.tint) {
        Ok(path) => Icon {
            path,
            tint: hit.tint,
        },
        Err(_) => hit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_tint() {
        assert_eq!(simple_slug("Dot.net"), "dotdotnet");
        assert_eq!(simple_slug("C++"), "cplusplus");
        assert_eq!(simple_slug("Déjà Vu"), "dejavu");
        assert_eq!(normalize("Proton Mail"), "protonmail");
        assert_eq!(
            tint_svg(r#"<svg viewBox="0 0 1 1"><path d="M0"/></svg>"#, "#123456"),
            r##"<svg viewBox="0 0 1 1" fill="#123456"><path d="M0"/></svg>"##
        );
        let out = tint_svg(
            r#"<svg fill="none"><path fill="white" style="stroke:#000"/></svg>"#,
            "#abcdef",
        );
        assert!(
            out.contains(r#"fill="none""#)
                && out.contains(r##"fill="#abcdef""##)
                && out.contains("stroke:#abcdef")
        );
    }

    pub fn build_fixture(home: &Path) {
        let ente = home.join("cache/icons/ente");
        fs::create_dir_all(ente.join("icons")).unwrap();
        fs::create_dir_all(ente.join("_data")).unwrap();
        fs::write(ente.join("icons/github.svg"), "<svg/>").unwrap();
        fs::write(ente.join("icons/proton_mail.svg"), "<svg/>").unwrap();
        fs::write(
            ente.join("_data/custom-icons.json"),
            r#"{"icons":[{"title":"GitHub","hex":"181717"},{"title":"Proton Mail","slug":"proton_mail","altNames":["ProtonMail"]}]}"#,
        )
        .unwrap();
        fs::write(ente.join(".complete"), "1").unwrap();
        let simple = home.join("cache/icons/simple");
        fs::create_dir_all(simple.join("icons")).unwrap();
        fs::write(simple.join("icons/discord.svg"), "<svg/>").unwrap();
        fs::write(
            simple.join("simple-icons.json"),
            r#"[{"title":"Discord","hex":"5865F2"}]"#,
        )
        .unwrap();
        fs::write(simple.join(".complete"), "1").unwrap();
    }

    #[test]
    fn matching() {
        let _lock = crate::paths::TEST_ENV
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let dir = tempfile::tempdir().unwrap();
        build_fixture(dir.path());
        std::env::set_var("ENTE_CODES_HOME", dir.path());
        let gh = resolve("GitHub", "", "").unwrap();
        assert_eq!(gh.tint, "#181717");
        assert!(fs::read_to_string(&gh.path)
            .unwrap()
            .contains(r##"fill="#181717""##));
        assert_eq!(resolve("protonmail", "", "").unwrap().tint, "");
        assert!(lookup("Proton Mail", "", "")
            .unwrap()
            .path
            .ends_with("proton_mail.svg"));
        assert!(lookup("discord.com", "", "")
            .unwrap()
            .path
            .ends_with("discord.svg"));
        assert!(resolve("Unknown Bank", "", "").is_none());
        assert!(lookup("Anything", "customIcon", "github")
            .unwrap()
            .path
            .ends_with("github.svg"));
    }
}
