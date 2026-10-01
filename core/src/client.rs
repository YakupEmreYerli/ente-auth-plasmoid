// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! Talking to the daemon, starting it first if nobody has.

use crate::paths;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn start_daemon() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = Command::new(exe);
    cmd.arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = cmd.spawn();
}

pub fn request(op: &str, fields: Value, autostart: bool) -> Result<Value, String> {
    let mut payload = json!({"op": op});
    if let (Some(p), Value::Object(f)) = (payload.as_object_mut(), fields) {
        p.extend(f);
    }
    let payload = zeroize::Zeroizing::new(payload.to_string() + "\n");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut started = false;
    let mut stream = loop {
        match UnixStream::connect(paths::socket_path()) {
            Ok(s) => break s,
            Err(_) if autostart => {
                if !started {
                    start_daemon();
                    started = true;
                }
                if Instant::now() > deadline {
                    return Err("the ente-codes daemon did not start".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return Err("the ente-codes daemon is not running".into()),
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(60))).ok();
    stream
        .write_all(payload.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|_| "unexpected answer from the daemon".to_string())
}
