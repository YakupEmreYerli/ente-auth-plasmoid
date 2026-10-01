// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! The one request that arrives over D-Bus: unlocking from the widget.
//!
//! The widget's passphrase field cannot hand a secret to a command without it
//! showing up in a command line, so it calls this method on the session bus
//! instead. The caller is identified by the bus itself (its process id) and
//! goes through the same check as every socket request.

use crate::daemon;
use zbus::message::Header;
use zbus::Connection;
use zeroize::Zeroizing;

pub const SERVICE: &str = "io.github.yakupemreyerli.EnteCodes";
pub const PATH: &str = "/io/github/yakupemreyerli/EnteCodes";

struct Codes {
    env: &'static daemon::Env,
}

#[zbus::interface(name = "io.github.yakupemreyerli.EnteCodes")]
impl Codes {
    /// Unlocks the codes; answers with the same JSON as the socket.
    async fn unlock(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        passphrase: String,
    ) -> String {
        let passphrase = Zeroizing::new(passphrase);
        let pid = match header.sender() {
            Some(sender) => match zbus::fdo::DBusProxy::new(conn).await {
                Ok(proxy) => proxy
                    .get_connection_unix_process_id(sender.clone().into())
                    .await
                    .map(|p| p as i32)
                    .unwrap_or(0),
                Err(_) => 0,
            },
            None => 0,
        };
        let env = self.env;
        // handle() is synchronous (it derives the key); run it on its own thread.
        let request = serde_json::json!({"op": "unlock", "passphrase": passphrase.as_str()});
        let reply = std::thread::spawn(move || daemon::handle(&request, pid, env)).join();
        match reply {
            Ok(v) => v.to_string(),
            Err(_) => serde_json::json!({"ok": false, "error": "internal error"}).to_string(),
        }
    }
}

/// Serves the interface for as long as the process lives. Errors are not
/// fatal: without a session bus the widget falls back to a dialog.
pub fn serve(env: &'static daemon::Env) -> Result<zbus::blocking::Connection, zbus::Error> {
    zbus::blocking::connection::Builder::session()?
        .name(SERVICE)?
        .serve_at(PATH, Codes { env })?
        .build()
}
