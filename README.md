
<img width="150" height="150" alt="0xid" src="https://github.com/user-attachments/assets/ffcf7d64-e040-4495-a1d1-a96cbaaeb148" />

# 0xID

Retro hacker plugin launcher. Single binary. No installer.

<img width="1591" height="994" alt="image" src="https://github.com/user-attachments/assets/a65d8071-9a9a-49f5-9e72-cfff6bc29cbb" />

## CMD

```
Ctrl + Shift + Space   →   summon
Q / Esc                →   minimize to tray
Ctrl + Q               →   quit
```

## What it does

Tabs:
- **LAUNCHER** — runs anything you drop into `~/.0xid/apps/`
- **SSH** — host manager, AES-256-GCM password vault (4-digit PIN), terminal, SFTP
- **SCANNER** — local network sweep
- **WSL2** — distro list / start / stop
- **WOL** — Wake-on-LAN with ping status

## Apps

Drop into `~/.0xid/apps/`:

```
.exe  .bat  .cmd  .ps1  .py  .lnk
```

Subfolders work. `logs/` is ignored. For metadata (icons, args, descriptions) add a `plugins.json`:

```json
[
  { "name": "scan",  "path": "nmap.exe", "args": ["-sn", "10.0.0.0/24"], "icon": "🛰" },
  { "name": "tail",  "path": "tail.bat", "interactive": true }
]
```

## Config

All in `~/.0xid/` (Windows: `%USERPROFILE%\.0xid\`):

```
.0xid/
├── apps/              ← drop your stuff here
├── ssh_hosts.json     ← passwords encrypted
└── wol_hosts.json
```

## Build

```sh
cargo build --release
```

Binary: `target/release/0xid.exe`.

## Release

Push a tag, GitHub Actions builds + publishes:

```sh
git tag v1.0.0
git push origin v1.0.0
```

## Stack

`eframe` / `egui` · Windows tray + global hotkey via `windows-sys` · `aes-gcm` for the SSH vault · built-in `ssh.exe` / `scp.exe` for sessions.



## Star History

<a href="https://www.star-history.com/?repos=infinition%2F0xid&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/chart?repos=infinition/0xid&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/chart?repos=infinition/0xid&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=infinition/0xid&type=date&legend=top-left" />
 </picture>
</a>
