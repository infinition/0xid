/// WSL2 distribution management — list, start, stop, open terminal.

use std::sync::mpsc;

#[derive(Debug, Clone)]
pub struct WslDistro {
    pub name: String,
    pub state: WslState,
    pub version: u8,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WslState {
    Running,
    Stopped,
    Unknown,
}

impl WslState {
    pub fn label(self) -> &'static str {
        match self {
            WslState::Running => "RUNNING",
            WslState::Stopped => "STOPPED",
            WslState::Unknown => "???",
        }
    }
}

/// Launch async distro listing. Results arrive via the Arc<Mutex>.
pub fn list_distros_async(result: std::sync::Arc<std::sync::Mutex<Option<Vec<WslDistro>>>>) {
    std::thread::spawn(move || {
        let distros = list_distros_sync();
        if let Ok(mut guard) = result.lock() {
            *guard = Some(distros);
        }
    });
}

/// Parse `wsl --list --verbose` output (UTF-16LE encoded on Windows). Blocking.
fn list_distros_sync() -> Vec<WslDistro> {
    let mut cmd = std::process::Command::new("wsl");
    cmd.args(["--list", "--verbose"]);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    // wsl outputs UTF-16LE on Windows
    let text = decode_wsl_output(&output.stdout);

    let mut distros = Vec::new();
    for line in text.lines().skip(1) {
        // skip header
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let is_default = line.starts_with('*');
        let line = line.trim_start_matches('*').trim();

        // Parse: "Name    State    Version"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let name = parts[0].to_string();
            let state = match parts[1].to_lowercase().as_str() {
                "running" => WslState::Running,
                "stopped" => WslState::Stopped,
                _ => WslState::Unknown,
            };
            let version = parts[2].parse().unwrap_or(2);

            distros.push(WslDistro {
                name,
                state,
                version,
                is_default,
            });
        }
    }

    distros
}

fn decode_wsl_output(raw: &[u8]) -> String {
    // Try UTF-16LE first (Windows default for wsl.exe)
    if raw.len() >= 2 {
        let u16_chars: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16_lossy(&u16_chars);
        // Verify it looks valid (should contain "NAME" or "State")
        if text.contains("NAME") || text.contains("State") || text.contains("Running") || text.contains("Stopped") {
            return text;
        }
    }
    // Fallback: plain UTF-8
    String::from_utf8_lossy(raw).to_string()
}

/// Start a WSL distro in the background (keeps it alive with a sleep process).
pub fn start_distro_async(name: &str, tx: mpsc::Sender<String>) {
    let name = name.to_string();
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            let result = std::process::Command::new("wsl")
                .args(["-d", &name, "--", "sh", "-c", "nohup sleep 2147483647 >/dev/null 2>&1 &"])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();

            match result {
                Ok(mut child) => {
                    let _ = child.wait();
                    let _ = tx.send(format!("> [WSL] {} started", name));
                }
                Err(e) => {
                    let _ = tx.send(format!("> [ERROR] Failed to start {}: {}", name, e));
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = tx.send(format!("> [ERROR] WSL is Windows-only"));
        }
    });
}

/// Stop/terminate a WSL distro.
pub fn stop_distro_async(name: &str, tx: mpsc::Sender<String>) {
    let name = name.to_string();
    std::thread::spawn(move || {
        let mut cmd = std::process::Command::new("wsl");
        cmd.args(["--terminate", &name]);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let result = cmd.output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    let _ = tx.send(format!("> [WSL] {} stopped", name));
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let _ = tx.send(format!("> [FAIL] Stop {}: {}", name, stderr.trim()));
                }
            }
            Err(e) => {
                let _ = tx.send(format!("> [ERROR] Failed to stop {}: {}", name, e));
            }
        }
    });
}

/// Open a terminal in the specified distro.
pub fn open_terminal(name: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        // Try Windows Terminal first, fallback to cmd
        let _ = std::process::Command::new("wt")
            .args(["wsl", "-d", name])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .or_else(|_| {
                std::process::Command::new("cmd")
                    .args(["/C", "start", "cmd.exe", "/c", "wsl", "-d", name])
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
            });
    }
}
