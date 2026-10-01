// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! The ente-codes command.

use crate::otp::Entry;
use crate::setup::t;
use crate::{client, daemon, icons, settings, setup, sources, vault};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;
use zeroize::Zeroizing;

pub const EXIT_ERROR: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_LOCKED: i32 = 3;
pub const EXIT_EMPTY: i32 = 4;
pub const EXIT_REFUSED: i32 = 5;

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub code: i32,
    pub extra: Value,
}

fn failure(message: impl Into<String>, code: i32) -> Failure {
    Failure {
        message: message.into(),
        code,
        extra: json!({}),
    }
}

type Result<T> = std::result::Result<T, Failure>;

#[derive(Parser)]
#[command(
    name = "ente-codes",
    version,
    about = "Your Ente Auth codes on the desktop, locked in memory."
)]
struct Args {
    /// Answer with one line of JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// First-time setup wizard (signs in to Ente, downloads the codes)
    Setup,
    /// Locked, unlocked or empty
    Status,
    /// Unlock with your passphrase
    Unlock {
        /// Ask in a desktop dialog
        #[arg(long)]
        gui: bool,
    },
    /// Forget the codes until the next unlock
    Lock,
    /// Entry names (no codes)
    List,
    /// Current codes (terminal or widget only)
    Codes,
    /// Copy a code to the clipboard
    Copy {
        /// Entry id, or part of its issuer or account name
        entry: String,
    },
    /// Import a plain-text export from the Ente Auth app
    Import {
        file: Option<PathBuf>,
        /// Wipe the export file afterwards
        #[arg(long)]
        delete_source: bool,
        #[arg(long)]
        gui: bool,
    },
    /// Sync with Ente now
    Sync {
        #[arg(long)]
        gui: bool,
    },
    /// The Ente account sync uses
    EnteAccount { email: String },
    /// Change the passphrase
    Passwd {
        #[arg(long)]
        gui: bool,
    },
    /// Show or change settings
    Settings {
        /// Clear a copied code after N seconds (0: never)
        #[arg(long, value_name = "N")]
        clear_seconds: Option<u64>,
        /// Lock after N idle minutes (0: never)
        #[arg(long, value_name = "N")]
        lock_minutes: Option<u64>,
        /// Fetch changes from Ente every N minutes while unlocked (0: only on unlock)
        #[arg(long, value_name = "N")]
        sync_minutes: Option<u64>,
    },
    /// The logo file for a service name (for previews and debugging)
    #[command(hide = true)]
    Icon { name: String },
    /// Download the service logos again
    Icons {
        #[arg(long)]
        force: bool,
    },
    /// Run the background process (systemd starts it)
    Daemon,
}

// ---------------------------------------------------------------- prompts

fn kdialog(args: &[&str]) -> Result<Option<Zeroizing<String>>> {
    let out = Command::new("kdialog")
        .args(["--title", "Ente Auth"])
        .args(args)
        .output()
        .map_err(|_| {
            failure(
                "kdialog not found; run this in a terminal instead",
                EXIT_ERROR,
            )
        })?;
    if !out.status.success() {
        return Ok(None);
    }
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    while text.ends_with('\n') {
        text.pop();
    }
    Ok(Some(Zeroizing::new(text)))
}

fn tty_prompt(text: &str) -> Result<Zeroizing<String>> {
    if !std::io::stdin().is_terminal() {
        return Err(failure(
            "no terminal to ask for the passphrase (use --gui)",
            EXIT_USAGE,
        ));
    }
    rpassword::prompt_password(format!("{text} "))
        .map(Zeroizing::new)
        .map_err(|e| failure(e.to_string(), EXIT_ERROR))
}

fn ask_passphrase(gui: bool, text: &str) -> Result<Option<Zeroizing<String>>> {
    if gui {
        kdialog(&["--password", text])
    } else {
        tty_prompt(text).map(Some)
    }
}

fn ask_new_passphrase(gui: bool) -> Result<Option<Zeroizing<String>>> {
    let text = t(
        "Choose a passphrase for your codes on this computer (at least 8 characters):",
        "Kodlarınız için bu bilgisayarda bir parola seçin (en az 8 karakter):",
    );
    if gui {
        return kdialog(&["--newpassword", &text]);
    }
    let first = tty_prompt(&text)?;
    let again = tty_prompt(&t("Again:", "Tekrar:"))?;
    if *first != *again {
        return Err(failure(
            t("the passphrases do not match", "parolalar eşleşmiyor"),
            EXIT_USAGE,
        ));
    }
    Ok(Some(first))
}

// ---------------------------------------------------------------- helpers

fn call(op: &str, fields: Value) -> Result<Value> {
    let reply = client::request(op, fields, true).map_err(|e| failure(e, EXIT_ERROR))?;
    if reply["ok"] == true {
        return Ok(reply);
    }
    let error = reply["error"].as_str().unwrap_or("failed").to_string();
    if reply["refused"] == true {
        return Err(failure(error, EXIT_REFUSED));
    }
    if reply["state"] == "empty" {
        return Err(Failure {
            message: "no codes imported yet".into(),
            code: EXIT_EMPTY,
            extra: json!({"state": "empty"}),
        });
    }
    if reply["state"] == "locked" || error == "locked" {
        return Err(Failure {
            message: "locked".into(),
            code: EXIT_LOCKED,
            extra: json!({"state": "locked"}),
        });
    }
    Err(failure(error, EXIT_ERROR))
}

/// Seal entries with a passphrase (asked for) and unlock the daemon with them.
pub fn store(entries: &[Entry], gui: bool) -> Result<Value> {
    let passphrase = if vault::exists() {
        let p = ask_passphrase(
            gui,
            &t(
                "Passphrase for your codes on this computer:",
                "Bu bilgisayardaki kodlarınızın parolası:",
            ),
        )?
        .ok_or_else(|| failure("cancelled", EXIT_USAGE))?;
        match vault::open(&p) {
            Ok(_) => p,
            Err(vault::VaultError::WrongPassphrase) => {
                return Err(failure("wrong passphrase", EXIT_ERROR))
            }
            Err(e) => return Err(failure(e.to_string(), EXIT_ERROR)),
        }
    } else {
        ask_new_passphrase(gui)?.ok_or_else(|| failure("cancelled", EXIT_USAGE))?
    };
    vault::save(entries, &passphrase).map_err(|e| failure(e.to_string(), EXIT_USAGE))?;
    call("unlock", json!({"passphrase": passphrase.as_str()}))
}

fn find<'a>(entries: &'a [Value], query: &str) -> Result<&'a Value> {
    if let Some(e) = entries.iter().find(|e| e["id"] == query) {
        return Ok(e);
    }
    let q = query.to_lowercase();
    let hits: Vec<&Value> = entries
        .iter()
        .filter(|e| {
            e["issuer"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains(&q)
                || e["account"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
        })
        .collect();
    match hits.len() {
        1 => Ok(hits[0]),
        0 => Err(failure(format!("nothing matches {query:?}"), EXIT_ERROR)),
        n => {
            let names: Vec<String> = hits
                .iter()
                .take(8)
                .map(|e| format!("{} ({})", label(e), e["id"].as_str().unwrap_or("")))
                .collect();
            Err(failure(
                format!("{n} entries match {query:?}: {}", names.join(", ")),
                EXIT_USAGE,
            ))
        }
    }
}

fn label(e: &Value) -> String {
    let issuer = e["issuer"].as_str().unwrap_or("");
    let account = e["account"].as_str().unwrap_or("");
    match (issuer.is_empty(), account.is_empty()) {
        (false, false) => format!("{issuer} ({account})"),
        (false, true) => issuer.into(),
        (true, false) => account.into(),
        _ => e["id"].as_str().unwrap_or("").into(),
    }
}

fn with(mut a: Value, b: Value) -> Value {
    if let (Some(a), Value::Object(b)) = (a.as_object_mut(), b) {
        a.extend(b);
    }
    a
}

// ---------------------------------------------------------------- commands

fn run_command(cmd: &Cmd) -> Result<Value> {
    match cmd {
        Cmd::Status => call("status", json!({})),
        Cmd::Unlock { gui } => {
            let state = call("status", json!({}))?;
            match state["state"].as_str() {
                Some("empty") => {
                    return Err(Failure {
                        message: "no codes imported yet".into(),
                        code: EXIT_EMPTY,
                        extra: json!({"state": "empty"}),
                    })
                }
                Some("unlocked") => return Ok(state),
                _ => {}
            }
            loop {
                let p = ask_passphrase(
                    *gui,
                    &t(
                        "Passphrase for your Ente Auth codes:",
                        "Ente Auth kodlarınızın parolası:",
                    ),
                )?
                .ok_or(Failure {
                    message: "cancelled".into(),
                    code: EXIT_LOCKED,
                    extra: json!({"state": "locked"}),
                })?;
                match call("unlock", json!({"passphrase": p.as_str()})) {
                    Err(f) if *gui && f.message.contains("wrong passphrase") => continue, // ask again
                    other => return other,
                }
            }
        }
        Cmd::Lock => client::request("lock", json!({}), false).map_err(|e| failure(e, EXIT_ERROR)),
        Cmd::List => call("list", json!({})),
        Cmd::Codes => call("codes", json!({})),
        Cmd::Copy { entry } => {
            let listed = call("list", json!({}))?;
            let entries = listed["entries"].as_array().cloned().unwrap_or_default();
            let hit = find(&entries, entry)?;
            call("copy", json!({"id": hit["id"]}))?;
            Ok(json!({"ok": true, "copied": hit["id"], "label": label(hit)}))
        }
        Cmd::Import {
            file,
            delete_source,
            gui,
        } => {
            let mut wipe = *delete_source;
            let path = match (file, gui) {
                (Some(f), _) => f.clone(),
                (None, true) => {
                    let home = std::env::var("HOME").unwrap_or_default();
                    let picked = kdialog(&[
                        "--getopenfilename",
                        &home,
                        "*.txt|Ente Auth plain-text export (*.txt)",
                    ])?
                    .ok_or_else(|| failure("cancelled", EXIT_USAGE))?;
                    // A desktop user cannot pass --delete-source; offer it.
                    let question = t(
                        "Wipe the plain-text export file after importing? (Recommended)",
                        "Düz metin dışa aktarım dosyası içe aktarıldıktan sonra silinsin mi? (Önerilir)",
                    );
                    wipe = wipe || kdialog(&["--yesno", &question])?.is_some();
                    PathBuf::from(picked.as_str())
                }
                (None, false) => {
                    return Err(failure(
                        "give the export file, or use --gui to pick it",
                        EXIT_USAGE,
                    ))
                }
            };
            let (entries, skipped) =
                sources::from_file(&path).map_err(|e| failure(e.0, EXIT_ERROR))?;
            let reply = store(&entries, *gui)?;
            if wipe {
                sources::wipe(&path);
            }
            Ok(with(
                reply,
                json!({"imported": entries.len(), "skipped": skipped, "source_deleted": wipe}),
            ))
        }
        Cmd::Sync { gui } => {
            // The first sync stores the codes under a new passphrase; after that the
            // daemon syncs by itself and this only asks it to do so now.
            let state = call("status", json!({}))?;
            match state["state"].as_str() {
                Some("unlocked") => return call("sync", json!({})),
                Some("locked") => {
                    return Err(Failure {
                        message: "locked: unlock first, the daemon then syncs by itself".into(),
                        code: EXIT_LOCKED,
                        extra: json!({"state": "locked"}),
                    })
                }
                _ => {}
            }
            let (entries, skipped) = sources::from_ente_cli(&settings::load().ente_email)
                .map_err(|e| failure(e.0, EXIT_ERROR))?;
            let reply = store(&entries, *gui)?;
            Ok(with(
                reply,
                json!({"imported": entries.len(), "skipped": skipped}),
            ))
        }
        Cmd::EnteAccount { email } => {
            let mut s = settings::load();
            s.ente_email = email.clone();
            settings::save(&s).map_err(|e| failure(e.to_string(), EXIT_ERROR))?;
            Ok(
                json!({"ok": true, "ente_email": email, "cli_installed": sources::ente_cli().is_some()}),
            )
        }
        Cmd::Passwd { gui } => {
            let old = ask_passphrase(*gui, &t("Current passphrase:", "Şu anki parola:"))?
                .ok_or_else(|| failure("cancelled", EXIT_USAGE))?;
            let (entries, _) = vault::open(&old).map_err(|e| match e {
                vault::VaultError::Empty => failure(e.to_string(), EXIT_EMPTY),
                _ => failure(e.to_string(), EXIT_ERROR),
            })?;
            let new = ask_new_passphrase(*gui)?.ok_or_else(|| failure("cancelled", EXIT_USAGE))?;
            vault::save(&entries, &new).map_err(|e| failure(e.to_string(), EXIT_USAGE))?;
            Ok(json!({"ok": true}))
        }
        Cmd::Settings {
            clear_seconds,
            lock_minutes,
            sync_minutes,
        } => {
            let mut s = settings::load();
            if clear_seconds.is_some() || lock_minutes.is_some() || sync_minutes.is_some() {
                s.clear_clipboard_seconds = clear_seconds.unwrap_or(s.clear_clipboard_seconds);
                s.lock_after_minutes = lock_minutes.unwrap_or(s.lock_after_minutes);
                s.sync_minutes = sync_minutes.unwrap_or(s.sync_minutes);
                settings::save(&s).map_err(|e| failure(e.to_string(), EXIT_ERROR))?;
            }
            Ok(with(json!({"ok": true}), serde_json::to_value(&s).unwrap()))
        }
        Cmd::Icon { name } => {
            let hit = icons::resolve(name, "", "");
            Ok(json!({
                "ok": true,
                "icon": hit.as_ref().map(|h| h.path.as_str()).unwrap_or(""),
                "tint": hit.as_ref().map(|h| h.tint.as_str()).unwrap_or(""),
            }))
        }
        Cmd::Icons { force } => {
            let mut out = json!({"ok": true});
            for (name, result) in icons::update(*force) {
                out[name] = json!(result);
            }
            Ok(out)
        }
        Cmd::Setup | Cmd::Daemon => unreachable!(),
    }
}

// ---------------------------------------------------------------- output

fn print_human(cmd: &Cmd, r: &Value) {
    match cmd {
        Cmd::Status => match r["state"].as_str() {
            Some("unlocked") => println!("unlocked, {} codes", r["count"]),
            Some("locked") => println!("locked: run `ente-codes unlock`"),
            _ => println!("no codes yet: `ente-codes setup`"),
        },
        Cmd::List => {
            for e in r["entries"].as_array().into_iter().flatten() {
                println!("{}  {}", e["id"].as_str().unwrap_or(""), label(e));
            }
        }
        Cmd::Codes => {
            for e in r["entries"].as_array().into_iter().flatten() {
                let mut code = e["code"].as_str().unwrap_or("").to_string();
                if code.is_empty() {
                    code = "-".into();
                } else if code.len() == 6 {
                    code.insert(3, ' ');
                }
                println!(
                    "{code:>9}  {:>2}s  {}",
                    e["remaining"].as_u64().unwrap_or(0),
                    label(e)
                );
            }
        }
        Cmd::Copy { .. } => println!("copied {}", r["label"].as_str().unwrap_or("")),
        Cmd::Sync { .. } if r.get("imported").is_none() => {
            println!(
                "{}",
                if r["sync"]["running"] == true {
                    "syncing in the background"
                } else {
                    "sync requested"
                }
            )
        }
        Cmd::Import { .. } | Cmd::Sync { .. } => {
            let mut line = format!("{} codes stored", r["imported"]);
            if r["skipped"].as_u64().unwrap_or(0) > 0 {
                line += &format!(", {} unreadable lines skipped", r["skipped"]);
            }
            if r["source_deleted"] == true {
                line += ", source file wiped";
            }
            println!("{line}");
        }
        Cmd::EnteAccount { .. } => {
            println!(
                "Ente account: {}",
                r["ente_email"].as_str().unwrap_or("(none)")
            );
            if r["cli_installed"] != true {
                println!("The Ente CLI (`ente`) is not installed yet.");
            }
        }
        Cmd::Settings { .. } | Cmd::Icons { .. } => {
            for (k, v) in r
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(k, _)| *k != "ok")
            {
                println!(
                    "{k} = {}",
                    v.as_str()
                        .map(String::from)
                        .unwrap_or_else(|| v.to_string())
                );
            }
        }
        _ => println!("{}", r["state"].as_str().unwrap_or("done")),
    }
}

pub fn main() -> i32 {
    let args = Args::parse();
    match args.command {
        Cmd::Daemon => return daemon::serve(),
        Cmd::Setup => return setup::run(),
        _ => {}
    }
    match run_command(&args.command) {
        Ok(result) => {
            if args.json {
                println!("{result}");
            } else {
                print_human(&args.command, &result);
            }
            0
        }
        Err(f) => {
            if args.json {
                println!(
                    "{}",
                    with(
                        json!({"ok": false, "error": f.message, "code": f.code}),
                        f.extra
                    )
                );
            } else {
                eprintln!("ente-codes: {}", f.message);
            }
            f.code
        }
    }
}
