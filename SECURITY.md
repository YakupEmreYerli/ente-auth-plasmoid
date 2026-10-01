# Security

## Reporting

Please report vulnerabilities privately through
[GitHub security advisories](https://github.com/YakupEmreYerli/ente-auth-plasmoid/security/advisories/new),
not in public issues. Never paste real codes, secrets or export files into a report.

## What this tool promises, and what it does not

It promises:

- Codes are encrypted at rest with a passphrase only you know (scrypt, AES-256-GCM).
- Decrypted codes exist only in the background process's memory while unlocked, and are overwritten when
  locked. The pages holding secrets are pinned in RAM so they never reach swap. The process disables core
  dumps and marks itself non-dumpable, so other programs running as you cannot ptrace it or read its memory
  through `/proc`.
- The Ente CLI export is written to RAM (`$XDG_RUNTIME_DIR`) and wiped right after it is read.
- Codes are handed out only to the Plasma widget and to interactive terminals.

It does not promise:

- Protection from malware running as your user. Such a program can, for example, watch your clipboard, log your
  keystrokes when you type the passphrase, or talk to the socket while pretending to be the widget.
- Protection while suspended to disk: hibernation writes all of RAM, including locked pages, to swap.

Keeping two-factor secrets on the same computer as your passwords weakens the "second factor". This tool
makes that trade-off explicit and as small as it can; decide whether it fits your threat model.
