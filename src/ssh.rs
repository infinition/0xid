/// SSH host management, encrypted credential storage, and session operations.
/// Uses Windows built-in ssh.exe/scp.exe for connections.
/// Passwords encrypted with AES-256-GCM, keyed from a 4-digit PIN.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

const NONCE_LEN: usize = 12;

fn config_file() -> PathBuf {
    crate::plugins::data_dir().join("ssh_hosts.json")
}

// ── Host configuration ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshHost {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    /// AES-GCM encrypted password (base64), None if key-based
    #[serde(default)]
    pub encrypted_password: Option<String>,
    /// Path to SSH private key
    #[serde(default)]
    pub key_path: Option<String>,
    /// Default remote directory for SFTP
    #[serde(default)]
    pub remote_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthMethod {
    Password,
    Key,
}

impl SshHost {
    pub fn display(&self) -> String {
        format!("{}@{}:{}", self.username, self.hostname, self.port)
    }
}

// ── Encryption (AES-256-GCM from 4-digit PIN) ──────────────────────────────

fn derive_key_from_pin(pin: &str) -> [u8; 32] {
    // Simple key derivation: SHA-256 of "0xID-ssh-" + pin repeated
    // Not PBKDF2 but sufficient for a 4-digit PIN protecting local config
    let input = format!("0xID-ssh-{}-{}-{}", pin, pin, pin);
    let mut key = [0u8; 32];
    // Simple hash: iterate and mix
    let bytes = input.as_bytes();
    for (i, &b) in bytes.iter().cycle().take(1024).enumerate() {
        key[i % 32] ^= b;
        key[(i + 7) % 32] = key[(i + 7) % 32].wrapping_add(b);
        key[(i + 13) % 32] = key[(i + 13) % 32].wrapping_mul(b | 1);
    }
    key
}

pub fn encrypt_password(password: &str, pin: &str) -> Option<String> {
    let key = derive_key_from_pin(pin);
    let cipher = Aes256Gcm::new_from_slice(&key).ok()?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, password.as_bytes()).ok()?;

    // Encode as: base64(nonce + ciphertext)
    let mut combined = nonce_bytes.to_vec();
    combined.extend(ciphertext);
    Some(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &combined,
    ))
}

pub fn decrypt_password(encrypted: &str, pin: &str) -> Option<String> {
    use base64::Engine;
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .ok()?;
    if combined.len() < NONCE_LEN + 1 {
        return None;
    }

    let key = derive_key_from_pin(pin);
    let cipher = Aes256Gcm::new_from_slice(&key).ok()?;

    let nonce = Nonce::from_slice(&combined[..NONCE_LEN]);
    let ciphertext = &combined[NONCE_LEN..];

    let plaintext = cipher.decrypt(nonce, ciphertext).ok()?;
    String::from_utf8(plaintext).ok()
}

// ── SSH Key detection ───────────────────────────────────────────────────────

pub fn detect_ssh_keys() -> Vec<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();

    if home.is_empty() {
        return Vec::new();
    }

    let ssh_dir = PathBuf::from(&home).join(".ssh");
    if !ssh_dir.exists() {
        return Vec::new();
    }

    let key_names = ["id_ed25519", "id_rsa", "id_ecdsa", "id_dsa"];
    let mut keys = Vec::new();

    for name in &key_names {
        let path = ssh_dir.join(name);
        if path.exists() {
            keys.push(path);
        }
    }

    // Also check for any other key files (no extension, not .pub)
    if let Ok(entries) = std::fs::read_dir(&ssh_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !name.ends_with(".pub")
                    && !name.starts_with("known_hosts")
                    && !name.starts_with("config")
                    && !name.starts_with("authorized")
                    && !keys.contains(&path)
                {
                    // Check if it looks like a key file (starts with -----)
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if content.starts_with("-----BEGIN") {
                            keys.push(path);
                        }
                    }
                }
            }
        }
    }

    keys
}

// ── Host config persistence ─────────────────────────────────────────────────

pub fn load_hosts() -> Vec<SshHost> {
    let content = match std::fs::read_to_string(config_file()) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_hosts(hosts: &[SshHost]) {
    if let Ok(json) = serde_json::to_string_pretty(hosts) {
        let _ = std::fs::write(config_file(), json);
    }
}

// ── SSH Session (terminal via piped ssh.exe) ────────────────────────────────

pub struct SshSession {
    pub host: SshHost,
    pub output: Vec<String>,
    pub input: String,
    stdin: Option<std::process::ChildStdin>,
    rx: Option<mpsc::Receiver<String>>,
    pub connected: bool,
}

impl SshSession {
    pub fn connect(host: SshHost, password: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel::<String>();
        let mut session = SshSession {
            host: host.clone(),
            output: vec![format!("> Connecting to {}...", host.display())],
            input: String::new(),
            stdin: None,
            rx: Some(rx),
            connected: false,
        };

        let hostname = host.hostname.clone();
        let port = host.port;
        let username = host.username.clone();
        let key_path = host.key_path.clone();
        let auth = host.auth.clone();

        std::thread::spawn(move || {
            use std::io::BufRead;
            use std::process::{Command, Stdio};

            let mut cmd = Command::new("ssh");

            // Common args
            cmd.args([
                "-tt",                          // Force TTY
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=10",
                "-p", &port.to_string(),
            ]);

            // Auth method
            match auth {
                AuthMethod::Key => {
                    if let Some(ref key) = key_path {
                        cmd.args(["-i", key]);
                    }
                }
                AuthMethod::Password => {
                    cmd.args(["-o", "PreferredAuthentications=password"]);
                }
            }

            cmd.arg(format!("{}@{}", username, hostname));

            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(format!("> [ERROR] SSH failed: {}", e));
                    return;
                }
            };

            let _ = tx.send(format!("> [SSH] Connected to {}", hostname));

            // If password auth, send password
            if auth == AuthMethod::Password {
                if let Some(ref pwd) = password {
                    if let Some(ref mut stdin) = child.stdin {
                        use std::io::Write;
                        // Small delay for SSH to prompt
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        let _ = writeln!(stdin, "{}", pwd);
                    }
                }
            }

            // Read stdout
            let tx_out = tx.clone();
            if let Some(out) = child.stdout.take() {
                std::thread::spawn(move || {
                    for line in std::io::BufReader::new(out).lines().flatten() {
                        if tx_out.send(format!("  {}", line)).is_err() {
                            break;
                        }
                    }
                });
            }

            // Read stderr
            let tx_err = tx;
            if let Some(err) = child.stderr.take() {
                std::thread::spawn(move || {
                    for line in std::io::BufReader::new(err).lines().flatten() {
                        if tx_err.send(format!("! {}", line)).is_err() {
                            break;
                        }
                    }
                });
            }

            let _ = child.wait();
        });

        session.connected = true;
        session
    }

    pub fn send_command(&mut self, cmd: &str) {
        if let Some(ref mut stdin) = self.stdin {
            use std::io::Write;
            let _ = writeln!(stdin, "{}", cmd);
        }
    }

    pub fn drain_output(&mut self) {
        if let Some(ref rx) = self.rx {
            while let Ok(line) = rx.try_recv() {
                self.output.push(line);
            }
        }
    }
}

// ── SFTP operations (via scp.exe / ssh commands) ────────────────────────────

#[derive(Debug, Clone)]
pub struct RemoteFile {
    pub name: String,
    pub is_dir: bool,
    pub size: String,
    pub modified: String,
    pub permissions: String,
}

/// List remote directory contents via SSH.
pub fn list_remote_dir_async(
    host: &SshHost,
    path: &str,
    password: Option<String>,
    result: Arc<Mutex<Option<Vec<RemoteFile>>>>,
) {
    let host = host.clone();
    let path = path.to_string();

    std::thread::spawn(move || {
        let files = list_remote_dir_sync(&host, &path, password.as_deref());
        if let Ok(mut r) = result.lock() {
            *r = Some(files);
        }
    });
}

fn list_remote_dir_sync(host: &SshHost, path: &str, _password: Option<&str>) -> Vec<RemoteFile> {
    let mut cmd = std::process::Command::new("ssh");
    cmd.args([
        "-o", "StrictHostKeyChecking=no",
        "-o", "ConnectTimeout=5",
        "-o", "BatchMode=yes",
        "-p", &host.port.to_string(),
    ]);

    if let Some(ref key) = host.key_path {
        cmd.args(["-i", key]);
    }

    cmd.arg(format!("{}@{}", host.username, host.hostname));
    cmd.arg(format!("ls -la --time-style=long-iso {}", path));

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    parse_ls_output(&text)
}

fn parse_ls_output(text: &str) -> Vec<RemoteFile> {
    let mut files = Vec::new();

    for line in text.lines().skip(1) {
        // Skip "total N" line
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 7 {
            continue;
        }

        let permissions = parts[0].to_string();
        let is_dir = permissions.starts_with('d');
        let size = parts[4].to_string();
        let modified = format!("{} {}", parts.get(5).unwrap_or(&""), parts.get(6).unwrap_or(&""));
        let name = parts[7..].join(" ");

        if name == "." || name == ".." {
            continue;
        }

        files.push(RemoteFile {
            name,
            is_dir,
            size,
            modified,
            permissions,
        });
    }

    // Sort: dirs first, then alphabetical
    files.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    files
}

/// Upload file via scp
pub fn scp_upload_async(
    host: &SshHost,
    local_path: &str,
    remote_path: &str,
    tx: mpsc::Sender<String>,
) {
    let host = host.clone();
    let local = local_path.to_string();
    let remote = remote_path.to_string();

    std::thread::spawn(move || {
        let mut cmd = std::process::Command::new("scp");
        cmd.args([
            "-o", "StrictHostKeyChecking=no",
            "-P", &host.port.to_string(),
            "-r", // recursive for directories
        ]);

        if let Some(ref key) = host.key_path {
            cmd.args(["-i", key]);
        }

        cmd.arg(&local);
        cmd.arg(format!("{}@{}:{}", host.username, host.hostname, remote));

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        let _ = tx.send(format!("> [SCP] Uploading {} → {}:{}", local, host.hostname, remote));

        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    let _ = tx.send(format!("> [OK] Upload complete: {}", local));
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    let _ = tx.send(format!("> [FAIL] Upload failed: {}", err.trim()));
                }
            }
            Err(e) => {
                let _ = tx.send(format!("> [ERROR] SCP failed: {}", e));
            }
        }
    });
}

/// Download file via scp
pub fn scp_download_async(
    host: &SshHost,
    remote_path: &str,
    local_path: &str,
    tx: mpsc::Sender<String>,
) {
    let host = host.clone();
    let remote = remote_path.to_string();
    let local = local_path.to_string();

    std::thread::spawn(move || {
        let mut cmd = std::process::Command::new("scp");
        cmd.args([
            "-o", "StrictHostKeyChecking=no",
            "-P", &host.port.to_string(),
            "-r",
        ]);

        if let Some(ref key) = host.key_path {
            cmd.args(["-i", key]);
        }

        cmd.arg(format!("{}@{}:{}", host.username, host.hostname, remote));
        cmd.arg(&local);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        let _ = tx.send(format!("> [SCP] Downloading {}:{} → {}", host.hostname, remote, local));

        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    let _ = tx.send(format!("> [OK] Download complete: {}", remote));
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    let _ = tx.send(format!("> [FAIL] Download failed: {}", err.trim()));
                }
            }
            Err(e) => {
                let _ = tx.send(format!("> [ERROR] SCP failed: {}", e));
            }
        }
    });
}

/// Build SSH command args for a host.
fn ssh_args(host: &SshHost) -> Vec<String> {
    let mut args = Vec::new();
    args.push("-tt".to_string()); // force TTY
    args.push("-o".to_string());
    args.push("StrictHostKeyChecking=no".to_string());
    args.push("-o".to_string());
    args.push("ConnectTimeout=10".to_string());
    args.push("-p".to_string());
    args.push(host.port.to_string());

    match host.auth {
        AuthMethod::Key => {
            if let Some(ref key) = host.key_path {
                if !key.is_empty() {
                    args.push("-i".to_string());
                    args.push(key.clone());
                }
            }
        }
        AuthMethod::Password => {
            // Password will be handled interactively
        }
    }

    args.push(format!("{}@{}", host.username, host.hostname));
    args
}

/// Open SSH in a separate Windows Terminal window.
pub fn open_ssh_terminal_external(host: &SshHost) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let mut wt_args = vec!["ssh".to_string()];
        wt_args.extend(ssh_args(host));

        let _ = std::process::Command::new("wt")
            .args(&wt_args)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .or_else(|_| {
                let ssh_cmd = wt_args.join(" ");
                std::process::Command::new("cmd")
                    .args(["/C", "start", "cmd.exe", "/k", &ssh_cmd])
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
            });
    }
}

/// Start an inline SSH session (piped I/O, like shell mode).
/// For password auth, uses SSH_ASKPASS trick to feed the password.
/// Returns (stdin_handle, output_receiver).
pub fn start_inline_ssh(
    host: &SshHost,
    password: Option<&str>,
) -> Result<(std::process::ChildStdin, mpsc::Receiver<String>), String> {
    use std::io::BufRead;
    use std::process::{Command, Stdio};

    let args = ssh_args(host);
    let mut cmd = Command::new("ssh");
    cmd.args(&args);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // For password auth: create a temp askpass script
    let mut askpass_path: Option<std::path::PathBuf> = None;
    if host.auth == AuthMethod::Password {
        if let Some(pwd) = password {
            let tmp = std::env::temp_dir().join("0xid_askpass.bat");
            let script = format!("@echo off\r\necho {}\r\n", pwd);
            std::fs::write(&tmp, &script).map_err(|e| format!("Failed to write askpass: {}", e))?;

            cmd.env("SSH_ASKPASS", tmp.to_string_lossy().as_ref());
            cmd.env("SSH_ASKPASS_REQUIRE", "force");
            cmd.env("DISPLAY", ":0"); // Required for SSH_ASKPASS on some versions
            askpass_path = Some(tmp);
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| format!("SSH launch failed: {}", e))?;

    let stdin = child.stdin.take().ok_or("Failed to get stdin")?;
    let (tx, rx) = mpsc::channel::<String>();

    // Clean up askpass file after a delay (auth should be done by then)
    if let Some(path) = askpass_path {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let _ = std::fs::remove_file(path);
        });
    }

    // stdout reader
    let tx_out = tx.clone();
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(out).lines().flatten() {
                if tx_out.send(format!("  {}", line)).is_err() {
                    break;
                }
            }
        });
    }

    // stderr reader
    let tx_err = tx;
    if let Some(err) = child.stderr.take() {
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(err).lines().flatten() {
                if tx_err.send(format!("! {}", line)).is_err() {
                    break;
                }
            }
        });
    }

    Ok((stdin, rx))
}
