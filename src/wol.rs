/// Wake-on-LAN + ping status for network hosts.

use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct WolHost {
    pub name: String,
    pub mac: String,
    pub broadcast: String,
    pub port: u16,
    pub ip: String,
    pub online: Option<bool>, // None = checking, Some(true/false)
}

impl WolHost {
    pub fn mac_bytes(&self) -> Option<[u8; 6]> {
        // Parse MAC from hex string (with or without separators)
        let clean: String = self.mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if clean.len() != 12 {
            return None;
        }
        let mut bytes = [0u8; 6];
        for i in 0..6 {
            bytes[i] = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(bytes)
    }
}

/// Send a Wake-on-LAN magic packet.
pub fn send_wol(host: &WolHost) -> Result<(), String> {
    let mac = host.mac_bytes().ok_or("Invalid MAC address")?;

    // Build magic packet: 6x 0xFF + 16x MAC
    let mut packet = vec![0xFFu8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac);
    }

    let addr: SocketAddr = format!("{}:{}", host.broadcast, host.port)
        .parse()
        .map_err(|e| format!("Bad broadcast address: {e}"))?;

    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("Socket error: {e}"))?;
    socket
        .set_broadcast(true)
        .map_err(|e| format!("Broadcast error: {e}"))?;
    socket
        .send_to(&packet, addr)
        .map_err(|e| format!("Send error: {e}"))?;

    Ok(())
}

/// Ping a host to check if it's online (async via thread).
pub fn ping_host_async(ip: String, result: Arc<Mutex<Option<bool>>>) {
    std::thread::spawn(move || {
        let online = ping_host(&ip);
        if let Ok(mut guard) = result.lock() {
            *guard = Some(online);
        }
    });
}

/// Ping a host (blocking). Returns true if online.
fn ping_host(ip: &str) -> bool {
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        std::process::Command::new("ping")
            .args(["-n", "1", "-w", "1000", ip])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };

    #[cfg(not(windows))]
    let output = std::process::Command::new("ping")
        .args(["-c", "1", "-W", "1", ip])
        .output();

    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Path to the WOL hosts config file (`~/.0xid/wol_hosts.json`).
pub fn config_file() -> std::path::PathBuf {
    crate::plugins::data_dir().join("wol_hosts.json")
}

/// Load hosts from the JSON config file. Creates an empty file on first run.
pub fn load_hosts() -> Vec<WolHost> {
    let path = config_file();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            let _ = std::fs::write(&path, "[]");
            "[]".to_string()
        }
    };

    #[derive(serde::Deserialize)]
    struct HostConfig {
        name: String,
        mac: String,
        #[serde(default = "default_broadcast")]
        broadcast: String,
        #[serde(default = "default_port")]
        port: u16,
        #[serde(default)]
        ip: String,
    }

    fn default_broadcast() -> String {
        "255.255.255.255".to_string()
    }
    fn default_port() -> u16 {
        9
    }

    let configs: Vec<HostConfig> = serde_json::from_str(&content).unwrap_or_default();

    configs
        .into_iter()
        .map(|c| WolHost {
            name: c.name,
            mac: c.mac,
            broadcast: c.broadcast,
            port: c.port,
            ip: c.ip,
            online: None,
        })
        .collect()
}
