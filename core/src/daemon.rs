// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! The background process that holds the unlocked entries in memory.
//!
//! It listens on a Unix socket in $XDG_RUNTIME_DIR (mode 0600) and speaks one
//! JSON object per line. Every request is checked against the caller's real
//! process (SO_PEERCRED), not against anything the caller claims.

use crate::guard::{self, Verdict};
use crate::otp::{self, Entry};
use crate::{icons, paths, settings, sources, vault};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Requests that reveal nothing: anyone may make them, blocked callers included.
const HARMLESS: &[&str] = &["status", "lock"];
/// Handling the passphrase, or asking for a sync: anyone not on the block list.
const OPEN: &[&str] = &["unlock", "sync"];
/// Requests that reveal something from inside the vault: widget or terminal only.
const GUARDED: &[&str] = &["list", "codes", "copy"];

#[derive(Default)]
pub struct State {
    entries: Option<Vec<Entry>>,
    sealer: Option<Arc<vault::Sealer>>,
    last_used: Option<Instant>,
    syncing: bool,
    last_sync: f64,
    last_attempt: f64,
    sync_error: String,
    clipboard_generation: u64,
}

impl State {
    fn unlocked(&self) -> bool {
        self.entries.is_some()
    }
    /// Dropping the entries wipes their secrets (Entry zeroizes on drop).
    fn forget(&mut self) {
        self.entries = None;
        self.sealer = None;
    }
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Everything the request handler reaches outside itself; faked in tests.
pub struct Env {
    pub check: fn(i32) -> Verdict,
    pub sync_configured: fn() -> bool,
    pub fetch: fn(&str) -> sources::Fetched,
    pub spawn: fn(Box<dyn FnOnce() + Send>),
    pub copy: fn(&str) -> Result<(), String>,
    pub update_icons: fn(),
}

pub fn real_env() -> Env {
    Env {
        check: guard::check,
        sync_configured: || {
            !settings::load().ente_email.is_empty() && sources::ente_cli().is_some()
        },
        fetch: sources::from_ente_cli,
        spawn: |job| {
            std::thread::spawn(job);
        },
        copy: copy_to_clipboard,
        update_icons: || {
            icons::update(false);
        },
    }
}

fn status(st: &State, env: &Env) -> Value {
    let sync = json!({
        "configured": (env.sync_configured)(), "running": st.syncing, "last": st.last_sync as u64,
        "error": st.sync_error, "cli": sources::ente_cli().is_some(),
    });
    if !vault::exists() {
        json!({"state": "empty", "sync": sync})
    } else if !st.unlocked() {
        json!({"state": "locked", "sync": sync})
    } else {
        json!({"state": "unlocked", "count": st.entries.as_ref().map_or(0, Vec::len), "sync": sync})
    }
}

fn merge(mut a: Value, b: Value) -> Value {
    if let (Some(a), Value::Object(b)) = (a.as_object_mut(), b) {
        a.extend(b);
    }
    a
}

fn ok(extra: Value) -> Value {
    merge(json!({"ok": true}), extra)
}

fn fail(msg: impl std::fmt::Display, extra: Value) -> Value {
    merge(json!({"ok": false, "error": msg.to_string()}), extra)
}

fn with_icon(e: &Entry, mut item: Value) -> Value {
    let hit = icons::resolve(
        if e.issuer.is_empty() {
            &e.account
        } else {
            &e.issuer
        },
        &e.icon_src,
        &e.icon_id,
    );
    item["icon"] = json!(hit.as_ref().map(|h| h.path.as_str()).unwrap_or(""));
    item["tint"] = json!(hit.as_ref().map(|h| h.tint.as_str()).unwrap_or(""));
    item
}

// ---------------------------------------------------------------- syncing

/// Download the codes again with the Ente CLI, in the background.
pub fn start_sync(env: &'static Env) -> bool {
    let sealer = {
        let mut st = state();
        if st.syncing || !st.unlocked() || !(env.sync_configured)() {
            return false;
        }
        st.syncing = true;
        st.last_attempt = now();
        st.sealer.clone()
    };
    (env.spawn)(Box::new(move || sync_worker(env, sealer)));
    true
}

fn sync_worker(env: &Env, sealer: Option<Arc<vault::Sealer>>) {
    let email = settings::load().ente_email;
    let result = (env.fetch)(&email);
    let mut st = state();
    st.syncing = false;
    match result {
        Err(e) => st.sync_error = e.to_string(),
        Ok((mut fresh, _)) => {
            st.sync_error.clear();
            // Locked meanwhile, or unlocked again with another key: drop it.
            let same = match (&st.sealer, &sealer) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            };
            if !st.unlocked() || !same {
                return;
            }
            otp::sort(&mut fresh);
            lock_in_ram(&fresh, None);
            if let Some(s) = &sealer {
                if let Err(e) = s.seal(&fresh) {
                    st.sync_error = format!("could not save: {e}");
                }
            }
            st.entries = Some(fresh);
            st.last_sync = now();
        }
    }
}

// ---------------------------------------------------------------- clipboard

fn copy_to_clipboard(code: &str) -> Result<(), String> {
    let mut child = Command::new("wl-copy")
        .args(["--sensitive", "--trim-newline"])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("wl-copy not found (install wl-clipboard): {e}"))?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(code.as_bytes())
        .map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("wl-copy failed".into());
    }
    let clear_after = settings::load().clear_clipboard_seconds;
    let generation = {
        let mut st = state();
        st.clipboard_generation += 1;
        st.clipboard_generation
    };
    if clear_after > 0 {
        let code = zeroize::Zeroizing::new(code.to_string());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(clear_after));
            if state().clipboard_generation != generation {
                return; // a newer code was copied meanwhile
            }
            let current = Command::new("wl-paste")
                .arg("--no-newline")
                .output()
                .map(|o| o.stdout)
                .unwrap_or_default();
            if current == code.as_bytes() {
                let _ = Command::new("wl-copy").arg("--clear").status();
            }
        });
    }
    Ok(())
}

// ---------------------------------------------------------------- requests

pub fn handle(req: &Value, pid: i32, env: &'static Env) -> Value {
    let op = req["op"].as_str().unwrap_or("");
    if !HARMLESS.contains(&op) && !OPEN.contains(&op) && !GUARDED.contains(&op) {
        return fail(format!("unknown request {op:?}"), json!({}));
    }
    if !HARMLESS.contains(&op) {
        let v = (env.check)(pid);
        if !v.allowed && (GUARDED.contains(&op) || v.blocked) {
            return fail(v.reason, json!({"refused": true}));
        }
    }

    match op {
        "status" => return ok(status(&state(), env)),
        "lock" => {
            let mut st = state();
            st.forget();
            return ok(status(&st, env));
        }
        "unlock" => {
            let passphrase =
                zeroize::Zeroizing::new(req["passphrase"].as_str().unwrap_or("").to_string());
            // Deriving the key takes a moment; keep the state free meanwhile.
            let opened = vault::open(&passphrase);
            let reply = {
                let mut st = state();
                match opened {
                    Err(vault::VaultError::WrongPassphrase) => {
                        return fail("wrong passphrase", json!({"wrong": true}))
                    }
                    Err(e) => return fail(e, json!({})),
                    Ok((mut entries, sealer)) => {
                        otp::sort(&mut entries);
                        let sealer = Arc::new(sealer);
                        lock_in_ram(&entries, Some(&sealer));
                        st.entries = Some(entries);
                        st.sealer = Some(sealer);
                        st.last_used = Some(Instant::now());
                        ok(status(&st, env))
                    }
                }
            };
            // Fetch changes from Ente and make sure the logos exist.
            start_sync(env);
            (env.spawn)(Box::new(move || (env.update_icons)()));
            return reply;
        }
        "sync" => {
            if !state().unlocked() {
                return fail("locked", status(&state(), env));
            }
            start_sync(env);
            return ok(status(&state(), env));
        }
        _ => {}
    }

    let (code_to_copy, reply) = {
        let mut st = state();
        if !st.unlocked() {
            return fail("locked", status(&st, env));
        }
        st.last_used = Some(Instant::now());
        let entries = st.entries.as_ref().unwrap();
        match op {
            "list" => {
                return ok(
                    json!({"entries": entries.iter().map(Entry::public).collect::<Vec<_>>()}),
                )
            }
            "codes" => {
                let t = now();
                let out: Vec<Value> = entries
                    .iter()
                    .map(|e| {
                        let mut item = with_icon(e, e.public());
                        match otp::code_for(e, t) {
                            Ok((code, remaining)) => {
                                item["code"] = json!(code);
                                item["remaining"] = json!(remaining);
                            }
                            Err(err) => {
                                item["code"] = json!("");
                                item["error"] = json!(err.to_string());
                            }
                        }
                        item
                    })
                    .collect();
                return ok(merge(json!({"entries": out}), status(&st, env)));
            }
            _ => {
                // copy
                let wanted = req["id"].as_str().unwrap_or("");
                let Some(e) = entries.iter().find(|e| e.id == wanted) else {
                    return fail("no such entry", json!({}));
                };
                match otp::code_for(e, now()) {
                    Ok((code, _)) => (zeroize::Zeroizing::new(code), ok(json!({"id": e.id}))),
                    Err(err) => return fail(err, json!({})),
                }
            }
        }
    };
    match (env.copy)(&code_to_copy) {
        Ok(()) => reply,
        Err(e) => fail(e, json!({})),
    }
}

// ---------------------------------------------------------------- serving

fn peer_pid(stream: &UnixStream) -> i32 {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 || cred.uid != unsafe { libc::getuid() } {
        return 0;
    }
    cred.pid
}

/// Pin the pages holding secrets in RAM so they never reach swap. Only those
/// pages: locking the whole process would exceed the usual 8 MB limit.
fn lock_in_ram(entries: &[Entry], sealer: Option<&vault::Sealer>) {
    let lock = |ptr: *const u8, len: usize| {
        if len > 0 {
            unsafe { libc::mlock(ptr as *const libc::c_void, len) };
        }
    };
    for e in entries {
        lock(e.secret.as_ptr(), e.secret.capacity());
    }
    if let Some(s) = sealer {
        let key = s.key_bytes();
        lock(key.as_ptr(), key.len());
    }
}

/// Keep the unlocked codes out of reach: no core dumps, and no ptrace or
/// /proc/PID/mem reads by other programs of the same user. Returns what failed.
pub fn harden() -> Vec<&'static str> {
    let mut missing = Vec::new();
    unsafe {
        let none = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::setrlimit(libc::RLIMIT_CORE, &none) != 0 {
            missing.push("core-limit");
        }
        if libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) != 0 {
            missing.push("non-dumpable");
        }
    }
    missing
}

fn serve_client(stream: UnixStream, env: &'static Env) {
    let pid = peer_pid(&stream);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut line = zeroize::Zeroizing::new(String::new());
    let mut reader = BufReader::new(stream.try_clone().expect("socket clone")).take(64 * 1024);
    let reply = match reader.read_line(&mut line) {
        Ok(_) => match serde_json::from_str::<Value>(&line) {
            Ok(req) if req.is_object() => handle(&req, pid, env),
            _ => fail("bad request", json!({})),
        },
        Err(_) => fail("bad request", json!({})),
    };
    let mut stream = stream;
    let _ = stream.write_all((reply.to_string() + "\n").as_bytes());
}

fn housekeeping(env: &'static Env) {
    loop {
        std::thread::sleep(Duration::from_secs(15));
        let s = settings::load();
        let due = {
            let mut st = state();
            if s.lock_after_minutes > 0
                && st.unlocked()
                && st
                    .last_used
                    .is_some_and(|t| t.elapsed() > Duration::from_secs(s.lock_after_minutes * 60))
            {
                st.forget();
            }
            st.unlocked()
                && s.sync_minutes > 0
                && now() - st.last_sync.max(st.last_attempt) > (s.sync_minutes * 60) as f64
        };
        if due {
            start_sync(env);
        }
    }
}

pub fn serve() -> i32 {
    for item in harden() {
        eprintln!("warning: could not apply {item}");
    }
    let env: &'static Env = Box::leak(Box::new(real_env()));
    if let Err(e) = paths::ensure_private(&paths::runtime_dir()) {
        eprintln!("cannot create {}: {e}", paths::runtime_dir().display());
        return 1;
    }
    let path = paths::socket_path();
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            println!("already running");
            return 0;
        }
        let _ = std::fs::remove_file(&path);
    }
    let old = unsafe { libc::umask(0o177) };
    let listener = UnixListener::bind(&path);
    unsafe { libc::umask(old) };
    let listener = match listener {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot listen on {}: {e}", path.display());
            return 1;
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    std::thread::spawn(move || housekeeping(env));
    // Kept alive for the life of the process; the widget unlocks through it.
    let _bus = match crate::dbus::serve(env) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("warning: no D-Bus unlock ({e}); the widget will ask in a dialog");
            None
        }
    };
    for stream in listener.incoming().flatten() {
        std::thread::spawn(move || serve_client(stream, env));
    }
    0
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::paths::TEST_ENV;

    const FAKE: &str = "JBSWY3DPEHPK3PXP";

    fn verdict(allowed: bool, blocked: bool) -> Verdict {
        Verdict {
            allowed,
            reason: "test".into(),
            blocked,
        }
    }

    fn inline(job: Box<dyn FnOnce() + Send>) {
        job()
    }
    fn skip(_: Box<dyn FnOnce() + Send>) {}

    fn env(
        check: fn(i32) -> Verdict,
        fetch: fn(&str) -> sources::Fetched,
        spawn: fn(Box<dyn FnOnce() + Send>),
    ) -> &'static Env {
        Box::leak(Box::new(Env {
            check,
            sync_configured: || true,
            fetch,
            spawn,
            copy: |_| Ok(()),
            update_icons: || {},
        }))
    }

    fn no_fetch(_: &str) -> Result<(Vec<Entry>, usize), sources::SourceError> {
        Err(sources::SourceError("offline".into()))
    }

    fn setup() -> (MutexGuard<'static, ()>, tempfile::TempDir) {
        let lock = TEST_ENV.lock().unwrap_or_else(|p| p.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ENTE_CODES_HOME", dir.path());
        state().forget();
        *state() = State::default();
        vault::save_to(
            &paths::vault_path(),
            &[Entry::new("a", "GitHub", "alice", FAKE)],
            "correct horse",
            10,
        )
        .unwrap();
        (lock, dir)
    }

    #[test]
    fn flow() {
        let (_l, _d) = setup();
        let env = env(|_| verdict(true, false), no_fetch, skip);
        assert_eq!(handle(&json!({"op": "codes"}), 1, env)["error"], "locked");
        assert_eq!(
            handle(&json!({"op": "unlock", "passphrase": "nope nope"}), 1, env)["wrong"],
            true
        );
        assert_eq!(
            handle(
                &json!({"op": "unlock", "passphrase": "correct horse"}),
                1,
                env
            )["state"],
            "unlocked"
        );
        let listed = handle(&json!({"op": "list"}), 1, env);
        assert_eq!(
            listed["entries"][0],
            json!({"id": "a", "issuer": "GitHub", "account": "alice", "kind": "totp", "digits": 6, "period": 30, "pinned": false})
        );
        let codes = handle(&json!({"op": "codes"}), 1, env);
        assert_eq!(codes["entries"][0]["code"].as_str().unwrap().len(), 6);
        assert!(!codes.to_string().contains(FAKE));
        assert_eq!(
            handle(&json!({"op": "copy", "id": "a"}), 1, env),
            json!({"ok": true, "id": "a"})
        );
        assert_eq!(handle(&json!({"op": "lock"}), 1, env)["state"], "locked");
    }

    #[test]
    fn blocked_callers_get_only_status_and_lock() {
        let (_l, _d) = setup();
        handle(
            &json!({"op": "unlock", "passphrase": "correct horse"}),
            1,
            env(|_| verdict(true, false), no_fetch, skip),
        );
        let blocked = env(|_| verdict(false, true), no_fetch, skip);
        for op in ["list", "codes", "copy", "unlock", "sync"] {
            assert_eq!(
                handle(
                    &json!({"op": op, "id": "a", "passphrase": "correct horse"}),
                    1,
                    blocked
                )["refused"],
                true,
                "{op}"
            );
        }
        assert_eq!(
            handle(&json!({"op": "status"}), 1, blocked)["state"],
            "unlocked"
        );
        assert_eq!(
            handle(&json!({"op": "lock"}), 1, blocked)["state"],
            "locked"
        );
    }

    #[test]
    fn background_unblocked_may_unlock_but_not_read() {
        let (_l, _d) = setup();
        let bg = env(|_| verdict(false, false), no_fetch, skip);
        assert_eq!(
            handle(
                &json!({"op": "unlock", "passphrase": "correct horse"}),
                1,
                bg
            )["ok"],
            true
        );
        assert_eq!(handle(&json!({"op": "codes"}), 1, bg)["refused"], true);
    }

    #[test]
    fn sync_replaces_and_reseals() {
        let (_l, _d) = setup();
        fn fetch(_: &str) -> Result<(Vec<Entry>, usize), sources::SourceError> {
            Ok((
                vec![
                    Entry::new("b", "Proton", "jane", FAKE),
                    Entry::new("a", "GitHub", "alice", FAKE),
                ],
                0,
            ))
        }
        handle(
            &json!({"op": "unlock", "passphrase": "correct horse"}),
            1,
            env(|_| verdict(true, false), fetch, inline),
        );
        let names: Vec<String> = state()
            .entries
            .as_ref()
            .unwrap()
            .iter()
            .map(|e| e.issuer.clone())
            .collect();
        assert_eq!(names, ["GitHub", "Proton"]);
        let (reopened, _) = vault::open("correct horse").unwrap();
        assert_eq!(reopened.len(), 2);
        assert!(state().last_sync > 0.0 && state().sync_error.is_empty());
    }

    #[test]
    fn sync_failure_keeps_codes() {
        let (_l, _d) = setup();
        handle(
            &json!({"op": "unlock", "passphrase": "correct horse"}),
            1,
            env(|_| verdict(true, false), no_fetch, inline),
        );
        assert_eq!(state().entries.as_ref().unwrap().len(), 1);
        assert_eq!(state().sync_error, "offline");
    }
}
