// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! Who may see a code.
//!
//! Codes go to two kinds of caller only: the Plasma widget (a descendant of
//! plasmashell) and a person typing in a terminal (a process with a
//! controlling TTY). Callers on the local block list are refused, however
//! they ask: through this command, a script or the socket directly.
//!
//! The block list lives outside the repository, in
//! `$XDG_CONFIG_HOME/ente-auth-plasmoid/blocked-callers`, one entry per line:
//! `env:NAME` blocks any process tree carrying that environment variable,
//! anything else is a process name. Lines starting with `#` are comments.
//! Without the file only the widget-or-terminal rule applies.
//!
//! This is a guard rail, not a security boundary: a program running as your
//! user can do a lot.

use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

const WIDGET_HOSTS: &[&str] = &["plasmashell", "plasmawindowed", "plasmoidviewer"];

/// Environment variables and process names whose callers are refused.
#[derive(Debug, Default)]
pub struct BlockList {
    env: Vec<String>,
    names: Option<Regex>,
}

impl BlockList {
    pub fn parse(text: &str) -> Self {
        let mut env = Vec::new();
        let mut names = Vec::new();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match line.strip_prefix("env:") {
                Some(name) => env.push(name.trim().to_string()),
                None => names.push(regex::escape(line)),
            }
        }
        let names = (!names.is_empty()).then(|| {
            Regex::new(&format!(
                r"(?i)(?:^|[/\s@])(?:{})(?:$|[\s/.-])",
                names.join("|")
            ))
            .unwrap()
        });
        BlockList { env, names }
    }

    fn env_marker(&self, present: &HashSet<String>) -> Option<&str> {
        self.env
            .iter()
            .find(|n| present.contains(n.as_str()))
            .map(String::as_str)
    }

    fn names_match(&self, text: &str) -> bool {
        self.names.as_ref().is_some_and(|re| re.is_match(text))
    }
}

fn block_list_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("ente-auth-plasmoid").join("blocked-callers"))
}

fn block_list() -> &'static BlockList {
    static LIST: OnceLock<BlockList> = OnceLock::new();
    LIST.get_or_init(|| {
        block_list_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .map(|text| BlockList::parse(&text))
            .unwrap_or_default()
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub allowed: bool,
    pub reason: String,
    pub blocked: bool,
}

impl Verdict {
    fn allow(reason: &str) -> Self {
        Verdict {
            allowed: true,
            reason: reason.into(),
            blocked: false,
        }
    }
    fn refuse(reason: String, blocked: bool) -> Self {
        Verdict {
            allowed: false,
            reason,
            blocked,
        }
    }
}

/// What the guard needs to know about a process; faked in tests.
pub trait Procs {
    fn parent(&self, pid: i32) -> i32;
    fn comm(&self, pid: i32) -> String;
    fn cmdline(&self, pid: i32) -> String;
    fn env_names(&self, pid: i32) -> HashSet<String>;
    fn has_tty(&self, pid: i32) -> bool;
}

pub struct ProcFs;

fn stat_fields(pid: i32) -> Vec<String> {
    let raw = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    // comm sits in parentheses and may contain spaces: split after the last ')'.
    match raw.rfind(')') {
        Some(i) if i + 2 <= raw.len() => {
            raw[i + 2..].split_whitespace().map(String::from).collect()
        }
        _ => Vec::new(),
    }
}

impl Procs for ProcFs {
    fn parent(&self, pid: i32) -> i32 {
        stat_fields(pid)
            .get(1)
            .and_then(|p| p.parse().ok())
            .unwrap_or(0)
    }
    fn comm(&self, pid: i32) -> String {
        fs::read_to_string(format!("/proc/{pid}/comm"))
            .unwrap_or_default()
            .trim()
            .to_string()
    }
    fn cmdline(&self, pid: i32) -> String {
        let raw = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        String::from_utf8_lossy(&raw)
            .replace('\0', " ")
            .trim()
            .to_string()
    }
    fn env_names(&self, pid: i32) -> HashSet<String> {
        let raw = fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
        raw.split(|b| *b == 0)
            .filter(|item| !item.is_empty())
            .map(|item| {
                let name = item.split(|b| *b == b'=').next().unwrap_or(item);
                String::from_utf8_lossy(name).into_owned()
            })
            .collect()
    }
    fn has_tty(&self, pid: i32) -> bool {
        stat_fields(pid).get(4).map(|t| t != "0").unwrap_or(false)
    }
}

pub fn ancestry(procs: &dyn Procs, mut pid: i32) -> Vec<i32> {
    let mut chain = Vec::new();
    while pid > 1 && chain.len() < 64 {
        chain.push(pid);
        pid = procs.parent(pid);
    }
    chain
}

/// May process `pid` receive codes?
pub fn check_with(procs: &dyn Procs, pid: i32, list: &BlockList) -> Verdict {
    if pid <= 0 {
        return Verdict::refuse("unknown caller".into(), false);
    }
    let chain = ancestry(procs, pid);
    for &p in &chain {
        let names = procs.env_names(p);
        if let Some(marker) = list.env_marker(&names) {
            return Verdict::refuse(format!("refused: blocked caller ({marker})"), true);
        }
        let comm = procs.comm(p);
        if list.names_match(&comm) || list.names_match(&procs.cmdline(p)) {
            return Verdict::refuse(format!("refused: blocked caller ({comm})"), true);
        }
    }
    if chain
        .iter()
        .any(|&p| WIDGET_HOSTS.contains(&procs.comm(p).as_str()))
    {
        return Verdict::allow("widget");
    }
    if procs.has_tty(pid) {
        return Verdict::allow("terminal");
    }
    Verdict::refuse(
        "refused: codes go only to the widget or an interactive terminal".into(),
        false,
    )
}

pub fn check(pid: i32) -> Verdict {
    check_with(&ProcFs, pid, block_list())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn check_with_empty(procs: &dyn Procs, pid: i32) -> Verdict {
        check_with(procs, pid, &BlockList::default())
    }

    #[test]
    fn empty_list_blocks_nothing() {
        let list = BlockList::parse("# nothing here\n\n");
        assert!(list
            .env_marker(&HashSet::from(["ANY".to_string()]))
            .is_none());
        assert!(!list.names_match("robot"));
    }

    struct Fake(HashMap<i32, (i32, &'static str, &'static str, Vec<&'static str>, bool)>);

    impl Procs for Fake {
        fn parent(&self, pid: i32) -> i32 {
            self.0.get(&pid).map(|p| p.0).unwrap_or(0)
        }
        fn comm(&self, pid: i32) -> String {
            self.0[&pid].1.into()
        }
        fn cmdline(&self, pid: i32) -> String {
            self.0[&pid].2.into()
        }
        fn env_names(&self, pid: i32) -> HashSet<String> {
            self.0[&pid].3.iter().map(|s| s.to_string()).collect()
        }
        fn has_tty(&self, pid: i32) -> bool {
            self.0[&pid].4
        }
    }

    fn tree(
        mid: (&'static str, &'static str, Vec<&'static str>),
        leaf_tty: bool,
        top: &'static str,
    ) -> Fake {
        Fake(HashMap::from([
            (
                30,
                (20, "ente-codes", "ente-codes codes", vec!["HOME"], leaf_tty),
            ),
            (20, (10, mid.0, mid.1, mid.2, leaf_tty)),
            (10, (1, top, top, vec!["HOME"], false)),
        ]))
    }

    #[test]
    fn widget_and_terminal_allowed() {
        assert!(
            check_with_empty(
                &tree(("sh", "sh -c x", vec!["HOME"]), false, "plasmashell"),
                30
            )
            .allowed
        );
        assert!(
            check_with_empty(
                &tree(("bash", "/bin/bash", vec!["HOME"]), true, "konsole"),
                30
            )
            .allowed
        );
    }

    #[test]
    fn blocked_callers_refused_even_with_tty() {
        let list = BlockList::parse("# test list\nenv:RUNNER_SESSION\nrobot\nhelper-bot\n");
        for mid in [
            ("bash", "/bin/bash", vec!["RUNNER_SESSION"]),
            ("robot", "robot", vec!["HOME"]),
            (
                "node",
                "node /usr/lib/node_modules/helper-bot/bin/helper-bot.js",
                vec!["HOME"],
            ),
            ("robot", "robot run", vec!["HOME"]),
        ] {
            let v = check_with(&tree(mid, true, "plasmashell"), 30, &list);
            assert!(!v.allowed && v.blocked, "{v:?}");
        }
    }

    #[test]
    fn background_scripts_refused() {
        let v = check_with_empty(
            &tree(
                ("python3", "python3 script.py", vec!["HOME"]),
                false,
                "systemd",
            ),
            30,
        );
        assert!(!v.allowed && !v.blocked);
    }
}
