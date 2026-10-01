// SPDX-FileCopyrightText: 2026 Yakup Emre Yerli
// SPDX-License-Identifier: GPL-2.0-or-later
//! `ente-codes setup`: the first-run wizard the widget opens in a terminal.
//!
//! It signs the Ente CLI in (answering the CLI's own questions about the app
//! type, export folder and e-mail, so only the Ente password and verification
//! code are left to type), downloads the codes and seals them under a new
//! passphrase.

use crate::cli::{store, Failure};
use crate::{paths, settings, sources, vault};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::pty::{forkpty, ForkptyResult, Winsize};
use nix::sys::termios::{self, SetArg};
use nix::sys::wait::{waitpid, WaitStatus};
use std::io::{IsTerminal, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::process::Command;

pub fn turkish() -> bool {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                return v.to_lowercase().starts_with("tr");
            }
        }
    }
    false
}

/// English or Turkish, following the system language.
pub fn t(en: &str, tr: &str) -> String {
    if turkish() {
        tr.into()
    } else {
        en.into()
    }
}

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

fn say(text: &str) {
    println!("{text}");
}

fn step(n: u32, text: &str) {
    say(&format!("\n{BOLD}{n}. {text}{RESET}"));
}

fn ask(prompt: &str, default: &str) -> String {
    if default.is_empty() {
        print!("{prompt}: ");
    } else {
        print!("{prompt} [{default}]: ");
    }
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    let line = line.trim();
    if line.is_empty() {
        default.to_string()
    } else {
        line.to_string()
    }
}

fn pause() {
    print!(
        "{DIM}{}{RESET}",
        t(
            "Press Enter to close this window.",
            "Pencereyi kapatmak için Enter'a basın."
        )
    );
    let _ = std::io::stdout().flush();
    let _ = std::io::stdin().read_line(&mut String::new());
}

pub fn auth_emails() -> Vec<String> {
    let Some(ente) = sources::ente_cli() else {
        return vec![];
    };
    let out = Command::new(ente)
        .args(["account", "list"])
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    let (mut emails, mut current) = (Vec::new(), None::<String>);
    for line in String::from_utf8_lossy(&out).lines() {
        let line = line.trim();
        if let Some(e) = line.strip_prefix("Email:") {
            current = Some(e.trim().to_string());
        } else if let Some(app) = line.strip_prefix("App:") {
            if app.trim() == "auth" {
                emails.extend(current.clone());
            }
        }
    }
    emails
}

/// Run `ente account add` in a pseudo-terminal, answering its first three
/// questions and passing everything else between the CLI and the user.
fn add_account(email: &str) -> bool {
    let Some(ente) = sources::ente_cli() else {
        return false;
    };
    let Ok(target) = paths::ensure_private(&paths::export_dir()) else {
        return false;
    };
    let mut answers: Vec<(&[u8], Vec<u8>)> = vec![
        (b"Enter app type", b"auth\n".to_vec()),
        (
            b"Enter export directory",
            format!("{}\n", target.display()).into_bytes(),
        ),
        (b"Enter email address", format!("{email}\n").into_bytes()),
    ];
    answers.reverse();

    let stdin = std::io::stdin();
    let mut size: Winsize = unsafe { std::mem::zeroed() };
    unsafe { libc::ioctl(stdin.as_raw_fd(), libc::TIOCGWINSZ, &mut size) };
    let fork = match unsafe { forkpty(Some(&size), None) } {
        Ok(f) => f,
        Err(_) => return false,
    };
    let (master, child) = match fork {
        ForkptyResult::Child => {
            let err = std::os::unix::process::CommandExt::exec(
                Command::new(&ente).args(["account", "add"]),
            );
            eprintln!("{err}");
            std::process::exit(127);
        }
        ForkptyResult::Parent { child, master } => (master, child),
    };

    // Our terminal goes raw so keystrokes (and hidden password input) pass through untouched.
    let saved = termios::tcgetattr(stdin.as_fd()).ok();
    if let Some(s) = &saved {
        let mut raw = s.clone();
        termios::cfmakeraw(&mut raw);
        let _ = termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &raw);
    }
    let mut seen: Vec<u8> = Vec::new();
    let mut buf = [0u8; 1024];
    let mut stdout = std::io::stdout();
    loop {
        let mut fds = [
            PollFd::new(stdin.as_fd(), PollFlags::POLLIN),
            PollFd::new(master.as_fd(), PollFlags::POLLIN),
        ];
        if poll(&mut fds, PollTimeout::from(200u16)).is_err() {
            break;
        }
        let master_ready = fds[1].revents().is_some_and(|r| !r.is_empty());
        // Hold typed-ahead keys until our own answers are in, so they cannot
        // land in the wrong prompt.
        let stdin_ready = answers.is_empty()
            && fds[0]
                .revents()
                .is_some_and(|r| r.contains(PollFlags::POLLIN));
        if master_ready {
            match nix::unistd::read(master.as_raw_fd(), &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = stdout.write_all(&buf[..n]);
                    let _ = stdout.flush();
                    seen.extend_from_slice(&buf[..n]);
                    if seen.len() > 512 {
                        seen.drain(..seen.len() - 512);
                    }
                    while let Some((prompt, answer)) = answers.last() {
                        if seen.windows(prompt.len()).any(|w| w == *prompt) {
                            let _ = nix::unistd::write(&master, answer);
                            answers.pop();
                            seen.clear();
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        if stdin_ready {
            match nix::unistd::read(stdin.as_raw_fd(), &mut buf) {
                Ok(0) | Err(_) => {}
                Ok(n) => {
                    let _ = nix::unistd::write(&master, &buf[..n]);
                }
            }
        }
        if let Ok(WaitStatus::Exited(..) | WaitStatus::Signaled(..)) =
            waitpid(child, Some(nix::sys::wait::WaitPidFlag::WNOHANG))
        {
            // Drain what is left on screen, then stop.
            while let Ok(n @ 1..) = nix::unistd::read(master.as_raw_fd(), &mut buf) {
                let _ = stdout.write_all(&buf[..n]);
            }
            break;
        }
    }
    if let Some(s) = &saved {
        let _ = termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, s);
    }
    let _ = stdout.flush();
    matches!(waitpid(child, None), Ok(WaitStatus::Exited(_, 0)) | Err(_))
}

pub fn run() -> i32 {
    if !std::io::stdin().is_terminal() {
        eprintln!("ente-codes setup needs a terminal");
        return 2;
    }
    say(&format!(
        "{BOLD}{}{RESET}",
        t(
            "Ente Auth codes: first-time setup",
            "Ente Auth kodları: ilk kurulum"
        )
    ));
    say(&t(
        "Your codes will be downloaded from Ente and kept on this computer, encrypted with a passphrase you choose.",
        "Kodlarınız Ente'den indirilip bu bilgisayarda, seçeceğiniz bir parolayla şifreli tutulacak.",
    ));
    if vault::exists() {
        say(&format!(
            "\n{GREEN}{}{RESET}",
            t(
                "Already set up: open the widget in the panel.",
                "Zaten kurulu: paneldeki bileşeni açın."
            )
        ));
        pause();
        return 0;
    }
    if sources::ente_cli().is_none() {
        say(&format!(
            "\n{RED}{}{RESET}",
            t("The Ente CLI is not installed.", "Ente CLI kurulu değil.")
        ));
        say(&t(
            "Install it (Arch: the ente-cli-bin package; others: github.com/ente-io/ente/releases, cli-v… release), then open this setup again from the widget.",
            "Kurun (Arch: ente-cli-bin paketi; diğerleri: github.com/ente-io/ente/releases, cli-v… sürümü), sonra bu kurulumu bileşenden yeniden açın.",
        ));
        pause();
        return 1;
    }

    step(1, &t("Your Ente account", "Ente hesabınız"));
    let known = auth_emails();
    let mut s = settings::load();
    let default = if s.ente_email.is_empty() {
        known.first().cloned().unwrap_or_default()
    } else {
        s.ente_email.clone()
    };
    let email = ask(
        &t(
            "E-mail address of your Ente account",
            "Ente hesabınızın e-posta adresi",
        ),
        &default,
    );
    if email.is_empty() {
        say(&format!(
            "{RED}{}{RESET}",
            t("No e-mail given.", "E-posta girilmedi.")
        ));
        pause();
        return 1;
    }
    if !known.contains(&email) {
        step(2, &t("Sign in to Ente", "Ente'ye giriş"));
        say(&t(
            "The Ente CLI now asks for your Ente password, then a code from your e-mail or authenticator.",
            "Ente CLI şimdi Ente şifrenizi, ardından e-postanıza gelen ya da doğrulayıcınızdaki kodu soracak.",
        ));
        say(&format!(
            "{DIM}{}{RESET}",
            t(
                "(Its first questions are answered for you.)",
                "(İlk soruları sizin yerinize cevaplanıyor.)"
            )
        ));
        if !add_account(&email) || !auth_emails().contains(&email) {
            say(&format!(
                "\n{RED}{}{RESET}",
                t(
                    "Signing in did not finish. Open the setup again to retry.",
                    "Giriş tamamlanmadı. Yeniden denemek için kurulumu tekrar açın."
                )
            ));
            pause();
            return 1;
        }
    }
    s.ente_email = email.clone();
    let _ = settings::save(&s);

    step(3, &t("Downloading your codes", "Kodlarınız indiriliyor"));
    let (entries, skipped) = match sources::from_ente_cli(&email) {
        Ok(r) => r,
        Err(e) => {
            say(&format!("{RED}{e}{RESET}"));
            pause();
            return 1;
        }
    };
    say(&t(
        &format!("{} codes found.", entries.len()),
        &format!("{} kod bulundu.", entries.len()),
    ));

    step(
        4,
        &t(
            "Choose a passphrase for this computer",
            "Bu bilgisayar için bir parola seçin",
        ),
    );
    say(&t(
        "At least 8 characters. It is separate from your Ente password; you type it once per login.",
        "En az 8 karakter. Ente şifrenizden ayrıdır; her oturum açılışında bir kez yazarsınız.",
    ));
    loop {
        match store(&entries, false) {
            Ok(_) => break,
            Err(Failure { message, .. }) => {
                say(&format!("{RED}{message}{RESET}"));
                if !message.contains("match")
                    && !message.contains("8 characters")
                    && !message.contains("eşleşmiyor")
                {
                    pause();
                    return 1;
                }
            }
        }
    }
    say(&format!(
        "\n{GREEN}{BOLD}{}{RESET} {}",
        t("Done.", "Tamam."),
        t(
            "Your codes are in the panel widget and stay in sync with Ente by themselves.",
            "Kodlarınız paneldeki bileşende ve Ente ile kendiliğinden eşit kalır."
        )
    ));
    if skipped > 0 {
        say(&t(
            &format!("{skipped} entries could not be read and were skipped."),
            &format!("{skipped} kayıt okunamadı, atlandı."),
        ));
    }
    pause();
    0
}
