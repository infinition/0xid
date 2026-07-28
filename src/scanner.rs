/// Ultra-fast network scanner.
/// Phase 1: ARP-style host discovery via parallel ICMP/TCP ping (50ms timeout)
/// Phase 2: Port scan only alive hosts with aggressive parallelism
/// Results stream in real-time.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const DEFAULT_PORTS: &[u16] = &[
    20, 21, 22, 23, 25, 26, 53, 67, 68, 69, 80, 81, 88, 110, 111,
    119, 123, 135, 137, 138, 139, 143, 161, 162, 179, 389, 427,
    443, 445, 464, 465, 500, 515, 520, 548, 554, 587, 593, 631,
    636, 873, 902, 993, 995, 1080, 1194, 1433, 1434, 1521, 1701,
    1723, 1812, 1813, 1883, 1900, 2049, 2082, 2083, 2181, 2222,
    3000, 3128, 3268, 3306, 3389, 3690, 4000, 4333, 4443, 4444,
    4500, 4567, 4662, 4672, 5000, 5001, 5004, 5005, 5050, 5060,
    5222, 5269, 5353, 5357, 5432, 5555, 5631, 5632, 5800, 5900,
    5938, 5984, 5985, 5986, 6000, 6379, 6443, 6666, 6667, 7000,
    7070, 7443, 7777, 7878, 8000, 8008, 8080, 8081, 8088, 8181,
    8443, 8444, 8880, 8888, 8983, 9000, 9001, 9090, 9091, 9100,
    9200, 9300, 9418, 9443, 9999, 10000, 10250, 11211, 15672,
    17500, 19132, 25565, 27017, 27018, 28017, 32400, 49152,
];

/// Quick-check ports for host discovery (most likely to be open)
const DISCOVERY_PORTS: &[u16] = &[80, 443, 22, 445, 139, 3389, 8080, 135];

#[derive(Debug, Clone)]
pub struct ScanHost {
    pub ip: Ipv4Addr,
    pub hostname: String,
    pub mac: String,
    pub open_ports: Vec<u16>,
    pub vendor: String,
    pub online: bool,
}

#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub subnet: String,
    pub start: u8,
    pub end: u8,
    pub ports: Vec<u16>,
    pub timeout_ms: u64,
    pub threads: usize,
}

impl Default for ScanConfig {
    fn default() -> Self {
        ScanConfig {
            subnet: "192.168.1".to_string(),
            start: 1,
            end: 254,
            ports: DEFAULT_PORTS.to_vec(),
            timeout_ms: 250,
            threads: 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub hosts: Vec<ScanHost>,
    pub scanned: usize,
    pub total: usize,
    pub done: bool,
    pub phase: String,
}

pub fn scan_async(config: ScanConfig, progress: Arc<Mutex<ScanProgress>>) {
    std::thread::spawn(move || {
        run_scan(config, progress);
    });
}

fn run_scan(config: ScanConfig, progress: Arc<Mutex<ScanProgress>>) {
    let octets: Vec<u8> = config
        .subnet
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let (o1, o2, o3) = (
        *octets.first().unwrap_or(&192),
        *octets.get(1).unwrap_or(&168),
        *octets.get(2).unwrap_or(&1),
    );

    let all_ips: Vec<Ipv4Addr> = (config.start..=config.end)
        .map(|i| Ipv4Addr::new(o1, o2, o3, i))
        .collect();

    let total_ips = all_ips.len();

    // ── Phase 1: Host discovery (fast parallel ping on common ports) ─────
    if let Ok(mut p) = progress.lock() {
        p.phase = "DISCOVERY (pass 1)".to_string();
        p.total = total_ips * 2; // 2 passes
        p.scanned = 0;
        p.done = false;
        p.hosts.clear();
    }

    let alive_ips = discover_hosts(&all_ips, &progress);

    // ── Phase 2: Full port scan on alive hosts ──────────────────────────
    let total_scans = alive_ips.len() * config.ports.len();
    if let Ok(mut p) = progress.lock() {
        p.phase = "PORT SCAN".to_string();
        p.total = total_scans;
        p.scanned = 0;
    }

    if alive_ips.is_empty() {
        if let Ok(mut p) = progress.lock() {
            p.done = true;
            p.phase = "DONE".to_string();
        }
        return;
    }

    // Build port scan tasks
    let tasks: Vec<(Ipv4Addr, u16)> = alive_ips
        .iter()
        .flat_map(|&ip| config.ports.iter().map(move |&port| (ip, port)))
        .collect();

    let (tx, rx) = std::sync::mpsc::channel::<(Ipv4Addr, u16)>();
    let rx = Arc::new(Mutex::new(rx));

    for task in &tasks {
        let _ = tx.send(*task);
    }
    drop(tx);

    let found: Arc<Mutex<HashMap<Ipv4Addr, Vec<u16>>>> = Arc::new(Mutex::new(HashMap::new()));
    let scanned = Arc::new(AtomicUsize::new(0));
    let timeout = Duration::from_millis(config.timeout_ms);
    let thread_count = config.threads.min(tasks.len()).max(1);

    let mut handles = Vec::new();
    for _ in 0..thread_count {
        let rx = rx.clone();
        let found = found.clone();
        let progress = progress.clone();
        let scanned = scanned.clone();
        let total_scans = total_scans;

        handles.push(std::thread::spawn(move || {
            loop {
                let task = match rx.lock() {
                    Ok(rx) => rx.recv().ok(),
                    Err(_) => break,
                };
                let (ip, port) = match task {
                    Some(t) => t,
                    None => break,
                };

                let addr = SocketAddr::new(IpAddr::V4(ip), port);
                if TcpStream::connect_timeout(&addr, timeout).is_ok() {
                    if let Ok(mut f) = found.lock() {
                        f.entry(ip).or_default().push(port);
                    }
                    // Real-time update
                    rebuild_hosts(&found, &progress);
                }

                let count = scanned.fetch_add(1, Ordering::Relaxed) + 1;
                if count % 200 == 0 || count == total_scans {
                    if let Ok(mut p) = progress.lock() {
                        p.scanned = count;
                    }
                }
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    // Final rebuild
    rebuild_hosts(&found, &progress);

    // Phase 3: Resolve hostnames (async, non-blocking per host)
    if let Ok(mut p) = progress.lock() {
        p.phase = "RESOLVING".to_string();
        p.scanned = total_scans;
    }
    resolve_hostnames(&progress);

    // Phase 4: Resolve MAC addresses (arp -a) and look up OUI vendors
    if let Ok(mut p) = progress.lock() {
        p.phase = "ARP / OUI".to_string();
    }
    resolve_macs(&progress);

    if let Ok(mut p) = progress.lock() {
        p.scanned = total_scans;
        p.done = true;
        p.phase = "DONE".to_string();
    }
}

/// Resolve hostnames for all discovered hosts using parallel DNS/NetBIOS lookups.
fn resolve_hostnames(progress: &Arc<Mutex<ScanProgress>>) {
    let ips: Vec<Ipv4Addr> = match progress.lock() {
        Ok(p) => p.hosts.iter().map(|h| h.ip).collect(),
        Err(_) => return,
    };

    if ips.is_empty() {
        return;
    }

    let results: Arc<Mutex<HashMap<Ipv4Addr, String>>> = Arc::new(Mutex::new(HashMap::new()));

    let (tx, rx) = std::sync::mpsc::channel::<Ipv4Addr>();
    let rx = Arc::new(Mutex::new(rx));

    for &ip in &ips {
        let _ = tx.send(ip);
    }
    drop(tx);

    let thread_count = ips.len().min(32);
    let mut handles = Vec::new();

    for _ in 0..thread_count {
        let rx = rx.clone();
        let results = results.clone();

        handles.push(std::thread::spawn(move || {
            loop {
                let ip = match rx.lock() {
                    Ok(rx) => rx.recv().ok(),
                    Err(_) => break,
                };
                let ip = match ip {
                    Some(ip) => ip,
                    None => break,
                };

                let hostname = resolve_hostname_fast(ip);
                if !hostname.is_empty() {
                    if let Ok(mut r) = results.lock() {
                        r.insert(ip, hostname);
                    }
                }
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    // Apply hostnames to hosts
    let resolved = Arc::try_unwrap(results).unwrap().into_inner().unwrap();
    if let Ok(mut p) = progress.lock() {
        for host in &mut p.hosts {
            if let Some(name) = resolved.get(&host.ip) {
                host.hostname = name.clone();
            }
        }
    }
}

/// Resolve MAC addresses via `arp -a` and look up OUI vendor for each host.
fn resolve_macs(progress: &Arc<Mutex<ScanProgress>>) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let output = std::process::Command::new("arp")
            .args(["-a"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let arp_table = match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(_) => return,
        };

        // Parse lines like:
        //   192.168.1.1       00-11-22-33-44-55     dynamic
        //   192.168.1.1       00:11:22:33:44:55     dynamic
        let mut mac_map: std::collections::HashMap<Ipv4Addr, String> = std::collections::HashMap::new();
        for line in arp_table.lines() {
            let line = line.trim();
            // Look for a line with an IP at the start
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            if let Ok(ip) = parts[0].parse::<Ipv4Addr>() {
                let mac = parts[1].replace('-', ":").to_ascii_uppercase();
                // Validate MAC-like format
                if mac.len() == 17 && mac.as_bytes().iter().filter(|&&b| b == b':').count() == 5 {
                    mac_map.insert(ip, mac);
                }
            }
        }

        if mac_map.is_empty() {
            return;
        }

        if let Ok(mut p) = progress.lock() {
            for host in &mut p.hosts {
                if let Some(mac) = mac_map.get(&host.ip) {
                    host.mac = mac.clone();
                    let oui = lookup_oui_vendor(mac);
                    if !oui.is_empty() && host.vendor.is_empty() {
                        host.vendor = oui.to_string();
                    }
                }
            }
        }
    }
}

/// Simple OUI-based vendor lookup for common manufacturers.
fn lookup_oui_vendor(mac: &str) -> &'static str {
    const OUI_TABLE: &[(&str, &str)] = &[
        // Virtual / container
        ("08:00:27", "Oracle/VirtualBox"),
        ("00:0C:29", "VMware"),
        ("00:50:56", "VMware"),
        ("00:05:69", "VMware"),
        ("00:1C:14", "VMware"),
        ("00:0E:C6", "Linux"),
        // Raspberry Pi
        ("B8:27:EB", "Raspberry Pi"),
        ("DC:A6:32", "Raspberry Pi"),
        ("E4:5F:01", "Raspberry Pi"),
        // Apple
        ("00:03:93", "Apple"), ("00:0D:93", "Apple"), ("00:0E:4B", "Apple"),
        ("00:0F:4B", "Apple"), ("00:11:24", "Apple"), ("00:13:E9", "Apple"),
        ("00:14:51", "Apple"), ("00:15:53", "Apple"), ("00:16:CB", "Apple"),
        ("00:17:F2", "Apple"), ("00:19:E3", "Apple"), ("00:1A:92", "Apple"),
        ("00:1B:63", "Apple"), ("00:1C:B3", "Apple"), ("00:1D:4F", "Apple"),
        ("00:1E:52", "Apple"), ("00:1F:5B", "Apple"), ("00:1F:F3", "Apple"),
        ("00:21:E9", "Apple"), ("00:22:41", "Apple"), ("00:23:32", "Apple"),
        ("00:23:6C", "Apple"), ("00:24:36", "Apple"), ("00:25:00", "Apple"),
        ("00:25:4B", "Apple"), ("00:26:08", "Apple"), ("00:26:B0", "Apple"),
        ("00:27:24", "Apple"), ("04:0C:CE", "Apple"), ("04:D4:C4", "Apple"),
        ("08:66:98", "Apple"), ("0C:74:C2", "Apple"), ("10:40:F3", "Apple"),
        ("14:10:9F", "Apple"), ("14:7D:DA", "Apple"), ("18:65:90", "Apple"),
        ("1C:36:BB", "Apple"), ("20:C9:D0", "Apple"), ("24:F0:94", "Apple"),
        ("28:E0:2C", "Apple"), ("2C:20:0B", "Apple"), ("30:F7:0D", "Apple"),
        ("34:12:98", "Apple"), ("34:2E:B7", "Apple"), ("3C:07:54", "Apple"),
        ("3C:15:C2", "Apple"), ("40:6C:8F", "Apple"), ("44:D8:84", "Apple"),
        ("48:43:7C", "Apple"), ("48:60:BC", "Apple"), ("48:E1:E9", "Apple"),
        ("50:46:5D", "Apple"), ("58:55:CA", "Apple"), ("5C:59:48", "Apple"),
        ("60:33:4B", "Apple"), ("60:92:17", "Apple"), ("68:AB:1E", "Apple"),
        ("6C:70:9F", "Apple"), ("70:14:A6", "Apple"), ("74:3A:EF", "Apple"),
        ("74:E2:F5", "Apple"), ("78:31:C1", "Apple"), ("80:BE:05", "Apple"),
        ("84:38:35", "Apple"), ("84:7B:CD", "Apple"), ("88:53:2E", "Apple"),
        ("8C:85:80", "Apple"), ("8C:7B:9D", "Apple"), ("90:84:0D", "Apple"),
        ("94:56:65", "Apple"), ("94:B8:6D", "Apple"), ("98:01:A7", "Apple"),
        ("98:FE:94", "Apple"), ("A0:99:9B", "Apple"), ("A4:D1:D2", "Apple"),
        ("A8:51:AB", "Apple"), ("B0:65:BD", "Apple"), ("B4:86:55", "Apple"),
        ("BC:4C:C4", "Apple"), ("C0:65:27", "Apple"), ("C4:B3:01", "Apple"),
        ("C8:5B:76", "Apple"), ("CC:5D:4E", "Apple"), ("D0:23:DB", "Apple"),
        ("D4:61:9D", "Apple"), ("DC:2B:2A", "Apple"), ("E0:2E:4A", "Apple"),
        ("E0:7C:13", "Apple"), ("E4:E4:AB", "Apple"), ("EC:35:86", "Apple"),
        ("F0:18:98", "Apple"), ("F0:DC:E2", "Apple"), ("F4:5C:89", "Apple"),
        ("F8:1E:DF", "Apple"), ("FC:FC:48", "Apple"),
        // Cisco
        ("00:00:0C", "Cisco"), ("00:01:42", "Cisco"), ("00:0C:85", "Cisco"),
        ("00:1A:A1", "Cisco"), ("00:1D:45", "Cisco"), ("00:1E:14", "Cisco"),
        ("00:1E:7A", "Cisco"), ("00:1F:26", "Cisco"), ("00:1F:CA", "Cisco"),
        ("00:21:55", "Cisco"), ("00:21:D8", "Cisco"), ("00:22:BD", "Cisco"),
        ("00:23:04", "Cisco"), ("00:23:8B", "Cisco"), ("00:23:AC", "Cisco"),
        ("00:24:14", "Cisco"), ("00:24:97", "Cisco"), ("00:24:C4", "Cisco"),
        ("00:26:0B", "Cisco"), ("00:26:CB", "Cisco"), ("00:3A:98", "Cisco"),
        ("70:72:3C", "Cisco"), ("BC:16:65", "Cisco"), ("00:1A:2B", "Cisco"),
        ("C8:1E:E7", "Cisco"), ("00:1B:D5", "Cisco"), ("00:1E:13", "Cisco"),
        ("00:1E:4F", "Cisco"), ("00:1E:49", "Cisco"), ("00:1E:12", "Cisco"),
        ("00:16:47", "Cisco"), ("00:0F:F7", "Cisco"), ("EC:BD:1D", "Cisco"),
        ("1C:DF:0F", "Cisco"),
        // Intel
        ("00:02:B3", "Intel"), ("00:04:23", "Intel"), ("00:07:E9", "Intel"),
        ("00:0D:65", "Intel"), ("00:0E:35", "Intel"), ("00:0E:7B", "Intel"),
        ("00:12:F0", "Intel"), ("00:13:02", "Intel"), ("00:13:C3", "Intel"),
        ("00:13:E8", "Intel"), ("00:15:00", "Intel"), ("00:15:17", "Intel"),
        ("00:16:76", "Intel"), ("00:16:EA", "Intel"), ("00:18:DE", "Intel"),
        ("00:19:D1", "Intel"), ("00:1B:21", "Intel"), ("00:1B:77", "Intel"),
        ("00:1C:BF", "Intel"), ("00:1D:0E", "Intel"), ("00:1D:60", "Intel"),
        ("00:1D:72", "Intel"), ("00:1E:67", "Intel"), ("00:1E:68", "Intel"),
        ("00:1F:3C", "Intel"), ("00:1F:5B", "Intel"), ("00:1F:C6", "Intel"),
        ("00:21:6B", "Intel"), ("00:21:CC", "Intel"), ("00:22:FA", "Intel"),
        ("00:24:D6", "Intel"), ("00:26:C6", "Intel"), ("00:27:13", "Intel"),
        ("00:30:4F", "Intel"), ("C8:F7:50", "Intel"), ("F0:4D:A2", "Intel"),
        ("F4:8E:38", "Intel"), ("E8:9A:8F", "Intel"), ("9C:B6:54", "Intel"),
        ("D8:FE:E3", "Intel"), ("40:16:9E", "Intel"), ("60:6C:66", "Intel"),
        ("10:98:36", "Intel"), ("00:24:D7", "Intel"), ("A8:66:7F", "Intel"),
        // Realtek
        ("00:E0:4C", "Realtek"), ("00:E0:18", "Realtek"), ("00:E0:A0", "Realtek"),
        // Broadcom
        ("00:10:18", "Broadcom"), ("00:0F:20", "Broadcom"), ("00:0A:F7", "Broadcom"),
        ("00:1A:5F", "Broadcom"), ("00:23:68", "Broadcom"), ("00:26:5D", "Broadcom"),
        ("30:05:5C", "Broadcom"), ("A0:04:60", "Broadcom"), ("20:CF:30", "Broadcom"),
        ("10:60:4B", "Broadcom"), ("DC:9B:5E", "Broadcom"), ("00:14:BF", "Broadcom"),
        ("E0:2B:E9", "Broadcom"),
        // Samsung
        ("00:01:5E", "Samsung"), ("00:02:78", "Samsung"), ("00:0F:B5", "Samsung"),
        ("00:12:47", "Samsung"), ("00:15:99", "Samsung"), ("00:16:E9", "Samsung"),
        ("00:1B:A0", "Samsung"), ("00:1C:43", "Samsung"), ("00:1E:48", "Samsung"),
        ("00:1F:49", "Samsung"), ("00:21:19", "Samsung"), ("00:21:D2", "Samsung"),
        ("00:22:1B", "Samsung"), ("00:23:D4", "Samsung"), ("00:24:54", "Samsung"),
        ("00:26:37", "Samsung"), ("00:26:5E", "Samsung"), ("00:27:B2", "Samsung"),
        ("00:2B:2B", "Samsung"), ("64:6B:F0", "Samsung"), ("E0:2C:3E", "Samsung"),
        ("08:FC:88", "Samsung"), ("00:24:90", "Samsung"),
        // Dell
        ("00:01:1A", "Dell"), ("00:06:5B", "Dell"), ("00:08:74", "Dell"),
        ("00:0B:DB", "Dell"), ("00:0F:1F", "Dell"), ("00:11:43", "Dell"),
        ("00:11:D8", "Dell"), ("00:12:3F", "Dell"), ("00:13:20", "Dell"),
        ("00:14:22", "Dell"), ("00:14:C1", "Dell"), ("00:15:C5", "Dell"),
        ("00:15:5D", "Dell"), ("00:16:36", "Dell"), ("00:16:CF", "Dell"),
        ("00:17:43", "Dell"), ("00:18:8B", "Dell"), ("00:19:5B", "Dell"),
        ("00:19:B9", "Dell"), ("00:1A:1E", "Dell"), ("00:1A:F1", "Dell"),
        ("00:1B:FC", "Dell"), ("00:1C:23", "Dell"), ("00:1C:9F", "Dell"),
        ("00:1D:09", "Dell"), ("00:1D:8B", "Dell"), ("00:1E:4C", "Dell"),
        ("00:1E:C9", "Dell"), ("00:1F:1A", "Dell"), ("00:1F:C1", "Dell"),
        ("00:21:9B", "Dell"), ("00:21:DB", "Dell"), ("00:22:19", "Dell"),
        ("00:22:6B", "Dell"), ("00:23:AE", "Dell"), ("00:23:B2", "Dell"),
        ("00:24:1D", "Dell"), ("00:24:E8", "Dell"), ("00:26:2D", "Dell"),
        ("00:26:55", "Dell"), ("B8:AC:6F", "Dell"), ("F0:1D:BC", "Dell"),
        ("14:58:D0", "Dell"), ("14:FE:B5", "Dell"), ("34:17:EB", "Dell"),
        // HP
        ("00:01:E6", "HP"), ("00:02:A5", "HP"), ("00:04:EA", "HP"),
        ("00:08:C7", "HP"), ("00:0B:CD", "HP"), ("00:0E:7F", "HP"),
        ("00:10:83", "HP"), ("00:10:E4", "HP"), ("00:11:0A", "HP"),
        ("00:11:85", "HP"), ("00:12:79", "HP"), ("00:13:21", "HP"),
        ("00:13:95", "HP"), ("00:15:60", "HP"), ("00:16:35", "HP"),
        ("00:17:A4", "HP"), ("00:18:71", "HP"), ("00:1A:4B", "HP"),
        ("00:1B:78", "HP"), ("00:1C:C4", "HP"), ("00:1E:0B", "HP"),
        ("00:1E:8C", "HP"), ("00:1F:28", "HP"), ("00:1F:29", "HP"),
        ("00:21:5A", "HP"), ("00:21:5D", "HP"), ("00:21:F7", "HP"),
        ("00:22:64", "HP"), ("00:23:47", "HP"), ("00:23:7D", "HP"),
        ("00:23:7E", "HP"), ("00:24:81", "HP"), ("00:25:36", "HP"),
        ("00:25:4E", "HP"), ("00:26:73", "HP"), ("00:26:B8", "HP"),
        ("00:27:18", "HP"), ("00:27:19", "HP"), ("1C:1B:0D", "HP"),
        ("1C:4B:87", "HP"), ("24:05:0F", "HP"), ("2C:27:D7", "HP"),
        ("3C:D7:DA", "HP"), ("9C:D3:5B", "HP"), ("A0:1D:48", "HP"),
        ("A4:1B:8B", "HP"), ("D4:85:64", "HP"), ("48:F7:5B", "HP"),
        ("00:17:3B", "HP"), ("00:17:6C", "HP"),
        // TP-Link
        ("00:26:2C", "TP-Link"), ("0C:9D:92", "TP-Link"), ("14:CF:92", "TP-Link"),
        ("1C:3B:F3", "TP-Link"), ("20:DC:E6", "TP-Link"), ("3C:21:9B", "TP-Link"),
        ("50:C7:BF", "TP-Link"), ("54:A0:50", "TP-Link"), ("60:4B:3B", "TP-Link"),
        ("68:72:51", "TP-Link"), ("74:DA:38", "TP-Link"), ("84:5C:92", "TP-Link"),
        ("90:F6:52", "TP-Link"), ("94:D9:C3", "TP-Link"), ("A0:2B:1F", "TP-Link"),
        ("A0:F3:C1", "TP-Link"), ("B0:BE:76", "TP-Link"), ("BC:F6:85", "TP-Link"),
        ("C0:3E:0F", "TP-Link"), ("C8:3A:35", "TP-Link"), ("CC:5A:F9", "TP-Link"),
        ("D4:6E:0E", "TP-Link"), ("E0:0D:F8", "TP-Link"), ("E4:F8:8E", "TP-Link"),
        ("EC:17:3F", "TP-Link"), ("F0:A2:27", "TP-Link"), ("F4:A1:13", "TP-Link"),
        ("F8:8F:8A", "TP-Link"), ("FC:BE:EB", "TP-Link"),
        // Netgear
        ("00:14:6C", "Netgear"), ("00:1B:2F", "Netgear"), ("00:1E:2A", "Netgear"),
        ("00:22:3F", "Netgear"), ("00:23:DF", "Netgear"), ("00:24:1C", "Netgear"),
        ("00:26:F2", "Netgear"), ("00:27:99", "Netgear"),
        ("0C:3C:65", "Netgear"), ("1C:3E:84", "Netgear"), ("20:E8:23", "Netgear"),
        ("2C:33:11", "Netgear"), ("2C:B0:5A", "Netgear"), ("30:46:9A", "Netgear"),
        ("38:94:2D", "Netgear"), ("3C:37:86", "Netgear"), ("44:55:33", "Netgear"),
        ("50:1C:26", "Netgear"), ("50:4F:94", "Netgear"), ("5C:49:7D", "Netgear"),
        ("68:3E:34", "Netgear"), ("6C:B0:CE", "Netgear"), ("70:EE:50", "Netgear"),
        ("78:D6:F0", "Netgear"), ("84:1B:5E", "Netgear"), ("88:3A:30", "Netgear"),
        ("8C:3B:AD", "Netgear"), ("94:DE:80", "Netgear"), ("98:90:96", "Netgear"),
        ("A0:21:B7", "Netgear"), ("A4:2B:B0", "Netgear"), ("A8:5E:40", "Netgear"),
        ("AC:22:0B", "Netgear"), ("B0:48:7A", "Netgear"), ("B4:75:0E", "Netgear"),
        ("BC:9C:31", "Netgear"), ("C0:34:40", "Netgear"), ("C4:3C:EA", "Netgear"),
        ("C8:3E:0F", "Netgear"), ("CC:CE:D0", "Netgear"), ("D0:37:61", "Netgear"),
        ("D4:4C:24", "Netgear"), ("E0:91:53", "Netgear"), ("E4:6B:4D", "Netgear"),
        ("E8:FC:60", "Netgear"), ("EC:1A:59", "Netgear"), ("F0:7D:68", "Netgear"),
        ("F4:1C:D9", "Netgear"), ("F8:1A:67", "Netgear"), ("FC:F5:28", "Netgear"),
        // Asus
        ("00:01:C0", "Asus"), ("00:0A:F5", "Asus"), ("00:0E:A6", "Asus"),
        ("00:10:DC", "Asus"), ("00:12:17", "Asus"), ("00:13:74", "Asus"),
        ("00:15:F2", "Asus"), ("00:16:E3", "Asus"), ("00:18:F3", "Asus"),
        ("00:1A:92", "Asus"), ("00:1B:FC", "Asus"), ("00:1C:B3", "Asus"),
        ("00:1E:2A", "Asus"), ("00:1F:C6", "Asus"), ("00:22:15", "Asus"),
        ("00:23:8A", "Asus"), ("00:24:8C", "Asus"), ("00:25:9A", "Asus"),
        ("00:26:18", "Asus"), ("08:60:6E", "Asus"), ("10:2E:AF", "Asus"),
        ("10:BF:48", "Asus"), ("14:D6:4D", "Asus"), ("20:CF:30", "Asus"),
        ("28:CF:DA", "Asus"), ("2C:56:DC", "Asus"), ("44:D9:E7", "Asus"),
        ("54:04:A6", "Asus"), ("68:FF:7B", "Asus"), ("70:4D:7B", "Asus"),
        ("80:A5:89", "Asus"), ("94:C6:91", "Asus"), ("D0:06:DD", "Asus"),
        ("D4:07:CA", "Asus"), ("D8:5E:D3", "Asus"), ("E8:28:C1", "Asus"),
        ("F4:6D:04", "Asus"),
        // Ubiquiti
        ("00:15:6D", "Ubiquiti"), ("00:1A:8C", "Ubiquiti"), ("00:27:22", "Ubiquiti"),
        ("04:18:D6", "Ubiquiti"), ("10:56:CA", "Ubiquiti"), ("10:74:94", "Ubiquiti"),
        ("18:E8:29", "Ubiquiti"), ("24:5A:4C", "Ubiquiti"), ("2C:3B:70", "Ubiquiti"),
        ("74:83:C2", "Ubiquiti"), ("78:8A:20", "Ubiquiti"), ("7C:0B:C6", "Ubiquiti"),
        ("80:2A:A8", "Ubiquiti"), ("84:46:FE", "Ubiquiti"), ("94:A6:7E", "Ubiquiti"),
        ("9C:51:81", "Ubiquiti"), ("C0:4A:00", "Ubiquiti"), ("C8:94:02", "Ubiquiti"),
        ("D0:6F:4B", "Ubiquiti"), ("D4:6A:91", "Ubiquiti"), ("E0:63:DA", "Ubiquiti"),
        ("E4:8D:8C", "Ubiquiti"), ("F0:9F:C2", "Ubiquiti"), ("F8:E7:1E", "Ubiquiti"),
        // Huawei
        ("00:0E:ED", "Huawei"), ("00:18:82", "Huawei"), ("00:19:ED", "Huawei"),
        ("00:1B:93", "Huawei"), ("00:1E:10", "Huawei"), ("00:1F:CB", "Huawei"),
        ("00:22:2D", "Huawei"), ("00:23:E3", "Huawei"), ("00:24:46", "Huawei"),
        ("00:25:48", "Huawei"), ("00:25:9E", "Huawei"), ("04:1E:64", "Huawei"),
        ("0C:1D:AF", "Huawei"), ("10:1F:74", "Huawei"), ("14:2D:27", "Huawei"),
        ("18:3E:3E", "Huawei"), ("1C:4A:A8", "Huawei"), ("20:1A:06", "Huawei"),
        ("24:46:C8", "Huawei"), ("2C:54:CF", "Huawei"), ("30:1B:97", "Huawei"),
        ("34:3E:2C", "Huawei"), ("3C:0C:48", "Huawei"), ("40:6A:AB", "Huawei"),
        ("44:FB:5A", "Huawei"), ("48:8C:32", "Huawei"), ("54:0B:A0", "Huawei"),
        ("54:2C:59", "Huawei"), ("58:CB:52", "Huawei"), ("5C:5B:6E", "Huawei"),
        ("60:1E:F0", "Huawei"), ("64:16:8F", "Huawei"), ("68:8C:E8", "Huawei"),
        ("6C:63:F5", "Huawei"), ("70:B8:4E", "Huawei"), ("74:85:2F", "Huawei"),
        ("78:11:76", "Huawei"), ("7C:2E:0C", "Huawei"), ("80:71:1A", "Huawei"),
        ("84:89:AD", "Huawei"), ("8C:2D:AA", "Huawei"), ("8C:71:F8", "Huawei"),
        ("90:17:AC", "Huawei"), ("94:8C:B5", "Huawei"), ("98:07:6E", "Huawei"),
        ("A0:8C:15", "Huawei"), ("A4:2C:2B", "Huawei"), ("A8:55:76", "Huawei"),
        ("AC:CF:5C", "Huawei"), ("B0:5C:E5", "Huawei"), ("B4:1E:7F", "Huawei"),
        ("B8:BC:1B", "Huawei"), ("C0:A9:0C", "Huawei"), ("C4:AE:0C", "Huawei"),
        ("C8:4C:75", "Huawei"), ("CC:72:BF", "Huawei"), ("D0:9A:03", "Huawei"),
        ("D4:3B:2C", "Huawei"), ("D8:57:EF", "Huawei"), ("DC:0E:A1", "Huawei"),
        ("E4:60:5E", "Huawei"), ("E8:3A:12", "Huawei"), ("EC:23:3E", "Huawei"),
        ("F0:22:1A", "Huawei"), ("F4:0F:9B", "Huawei"), ("F4:AA:45", "Huawei"),
        ("F8:6F:AE", "Huawei"), ("FC:A4:B6", "Huawei"),
        // MikroTik
        ("00:0B:6B", "MikroTik"), ("10:0E:7E", "MikroTik"), ("14:CB:65", "MikroTik"),
        ("1C:7E:E5", "MikroTik"), ("20:0C:E4", "MikroTik"), ("24:0A:C4", "MikroTik"),
        ("28:1B:AA", "MikroTik"), ("2C:C8:1B", "MikroTik"), ("30:88:F1", "MikroTik"),
        ("34:08:BC", "MikroTik"), ("38:EA:A7", "MikroTik"), ("4C:5F:70", "MikroTik"),
        ("64:70:02", "MikroTik"), ("6C:3B:6B", "MikroTik"), ("74:4D:28", "MikroTik"),
        ("7C:61:93", "MikroTik"), ("80:7A:BF", "MikroTik"), ("94:D4:69", "MikroTik"),
        ("A4:50:46", "MikroTik"), ("B8:69:F4", "MikroTik"), ("C0:17:EE", "MikroTik"),
        ("CC:2D:E0", "MikroTik"), ("D0:2B:E2", "MikroTik"), ("D4:01:6D", "MikroTik"),
        ("D4:CA:6D", "MikroTik"), ("E0:3F:13", "MikroTik"), ("EC:08:6B", "MikroTik"),
        // Aruba
        ("00:0F:66", "Aruba"), ("00:24:6C", "Aruba"), ("20:4C:03", "Aruba"),
        ("24:DE:C6", "Aruba"), ("28:6C:07", "Aruba"), ("34:2B:3F", "Aruba"),
        ("38:17:C3", "Aruba"), ("3C:7D:0A", "Aruba"), ("44:38:39", "Aruba"),
        ("48:0F:CF", "Aruba"), ("4C:ED:DE", "Aruba"), ("58:AC:78", "Aruba"),
        ("6C:64:15", "Aruba"), ("70:2E:4F", "Aruba"), ("78:44:FD", "Aruba"),
        ("7C:11:4B", "Aruba"), ("80:C6:AB", "Aruba"), ("84:D4:7E", "Aruba"),
        ("8C:9F:F0", "Aruba"), ("94:57:A5", "Aruba"), ("A0:70:28", "Aruba"),
        ("AC:A3:1E", "Aruba"), ("B0:34:95", "Aruba"), ("BC:4B:2F", "Aruba"),
        ("C4:6E:8F", "Aruba"), ("CC:46:D7", "Aruba"), ("D0:BF:9C", "Aruba"),
        ("D4:0C:1F", "Aruba"), ("D8:C4:E9", "Aruba"), ("E0:45:95", "Aruba"),
        ("E4:F0:7B", "Aruba"), ("F0:2F:74", "Aruba"), ("F4:6B:8C", "Aruba"),
        ("F8:2C:18", "Aruba"),
        // Xiaomi
        ("08:D8:2C", "Xiaomi"), ("18:FE:34", "Xiaomi"), ("2C:F0:EE", "Xiaomi"),
        ("40:02:43", "Xiaomi"), ("54:60:09", "Xiaomi"), ("60:80:80", "Xiaomi"),
        ("64:9D:99", "Xiaomi"), ("88:53:95", "Xiaomi"), ("8C:78:E5", "Xiaomi"),
        ("F0:FE:5F", "Xiaomi"), ("D8:FB:5E", "Xiaomi"), ("AC:72:89", "Xiaomi"),
        // Google
        ("00:1A:11", "Google"), ("04:8D:38", "Google"), ("18:8B:9D", "Google"),
        ("1C:ED:61", "Google"), ("24:A4:3C", "Google"), ("28:C6:8E", "Google"),
        ("38:0F:4A", "Google"), ("3C:5A:37", "Google"), ("50:6A:03", "Google"),
        ("68:54:ED", "Google"), ("8C:8B:83", "Google"), ("A4:77:33", "Google"),
        ("B0:75:D5", "Google"), ("C8:94:BB", "Google"), ("D8:0D:17", "Google"),
        ("E0:67:B3", "Google"), ("F8:A4:5F", "Google"),
        // Amazon
        ("00:1B:67", "Amazon"), ("20:68:9D", "Amazon"), ("68:B6:E8", "Amazon"),
        ("8C:0F:6F", "Amazon"), ("9C:5C:8E", "Amazon"), ("A8:FD:0E", "Amazon"),
        ("AC:63:BE", "Amazon"), ("B0:4E:26", "Amazon"), ("BC:14:01", "Amazon"),
        ("C0:A0:BB", "Amazon"), ("D4:70:F9", "Amazon"), ("E8:7A:04", "Amazon"),
        ("F0:1B:03", "Amazon"), ("F0:27:2B", "Amazon"), ("F8:0D:A9", "Amazon"),
    ];

    let prefix = &mac[..8];
    for &(oui, vendor) in OUI_TABLE {
        if oui == prefix {
            return vendor;
        }
    }
    ""
}

/// Fast hostname resolution: try DNS reverse lookup via `nslookup` (primary)
/// then `ping -a` (fallback), then `nbtstat -A` (NetBIOS fallback).
fn resolve_hostname_fast(ip: Ipv4Addr) -> String {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let ip_str = ip.to_string();

        // Try nslookup first (most reliable for DNS PTR records)
        let output = std::process::Command::new("nslookup")
            .args([&ip_str])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if let Some(name_part) = trimmed.strip_prefix("Name:") {
                    let name = name_part.trim().trim_matches('"').trim_matches('\'');
                    if !name.is_empty() && name != ip_str {
                        if let Some(host) = name.split('.').next() {
                            if !host.is_empty() {
                                return host.to_string();
                            }
                        }
                        return name.to_string();
                    }
                }
            }
        }

        // Fallback: ping -a for NetBIOS / LLMNR names
        let output = std::process::Command::new("ping")
            .args(["-a", "-n", "1", "-w", "500", &ip_str])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(line) = text.lines().next() {
                let line = line.trim();
                if let Some(bracket_pos) = line.find('[') {
                    let before_bracket = line[..bracket_pos].trim();
                    let words: Vec<&str> = before_bracket.split_whitespace().collect();
                    if let Some(&name) = words.last() {
                        let name = name.trim_matches('\'');
                        if !name.is_empty()
                            && name != ip_str
                            && !name.to_lowercase().contains("ping")
                        {
                            return name.to_string();
                        }
                    }
                }
            }
        }

        // Last resort: nbtstat -A for NetBIOS name table
        let output = std::process::Command::new("nbtstat")
            .args(["-A", &ip_str])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            // Format:
            //   NetBIOS Remote Machine Name Table
            //   Name               Type         Status
            //   MY-PC        <00>  UNIQUE      Registered
            for line in text.lines() {
                let trimmed = line.trim();
                // Skip header/separator lines
                if trimmed.is_empty()
                    || trimmed.contains("NetBIOS")
                    || trimmed.contains("Name")
                    || trimmed.contains("---")
                {
                    continue;
                }
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 3 {
                    let name = parts[0];
                    // Skip group names (containing <1B>, <1C>, <1E>) and
                    // domain/workgroup names; prefer unique workstation names.
                    if !name.is_empty()
                        && name != ip_str
                        && !name.contains(".")
                        && !name.ends_with("$")
                    {
                        return name.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

/// Host discovery with 2 passes for reliability.
/// Pass 1: fast (100ms) on common ports.
/// Pass 2: slower (300ms) on remaining IPs to catch slow responders.
fn discover_hosts(ips: &[Ipv4Addr], progress: &Arc<Mutex<ScanProgress>>) -> Vec<Ipv4Addr> {
    // Pass 1: fast sweep
    let found_pass1 = discover_pass(ips, Duration::from_millis(100), progress, 0);

    // Pass 2: retry IPs NOT found in pass 1 with longer timeout
    let remaining: Vec<Ipv4Addr> = ips
        .iter()
        .filter(|ip| !found_pass1.contains(ip))
        .copied()
        .collect();

    if let Ok(mut p) = progress.lock() {
        p.phase = "DISCOVERY (pass 2)".to_string();
    }

    let found_pass2 = discover_pass(&remaining, Duration::from_millis(350), progress, ips.len());

    let mut all: Vec<Ipv4Addr> = found_pass1;
    all.extend(found_pass2);
    all.sort();
    all.dedup();
    all
}

fn discover_pass(
    ips: &[Ipv4Addr],
    timeout: Duration,
    progress: &Arc<Mutex<ScanProgress>>,
    scanned_offset: usize,
) -> Vec<Ipv4Addr> {
    if ips.is_empty() {
        return Vec::new();
    }

    let alive: Arc<Mutex<Vec<Ipv4Addr>>> = Arc::new(Mutex::new(Vec::new()));
    let scanned = Arc::new(AtomicUsize::new(0));
    let total = ips.len();

    let (tx, rx) = std::sync::mpsc::channel::<Ipv4Addr>();
    let rx = Arc::new(Mutex::new(rx));

    for &ip in ips {
        let _ = tx.send(ip);
    }
    drop(tx);

    let thread_count = 256.min(total);
    let mut handles = Vec::new();

    for _ in 0..thread_count {
        let rx = rx.clone();
        let alive = alive.clone();
        let scanned = scanned.clone();
        let progress = progress.clone();
        let timeout = timeout;

        handles.push(std::thread::spawn(move || {
            loop {
                let ip = match rx.lock() {
                    Ok(rx) => rx.recv().ok(),
                    Err(_) => break,
                };
                let ip = match ip {
                    Some(ip) => ip,
                    None => break,
                };

                let mut is_alive = false;

                // Try discovery ports
                for &port in DISCOVERY_PORTS {
                    let addr = SocketAddr::new(IpAddr::V4(ip), port);
                    match TcpStream::connect_timeout(&addr, timeout) {
                        Ok(_) => { is_alive = true; break; }
                        Err(e) => {
                            let msg = format!("{}", e);
                            // Connection refused = host is there, port just closed
                            if msg.contains("refused") || msg.contains("10061") {
                                is_alive = true;
                                break;
                            }
                        }
                    }
                }

                // Fallback: try port 0 (RST = host alive)
                if !is_alive {
                    let addr = SocketAddr::new(IpAddr::V4(ip), 0);
                    if let Err(e) = TcpStream::connect_timeout(&addr, timeout) {
                        let msg = format!("{}", e);
                        if msg.contains("refused") || msg.contains("10061") {
                            is_alive = true;
                        }
                    }
                }

                if is_alive {
                    if let Ok(mut a) = alive.lock() {
                        a.push(ip);
                    }
                }

                let count = scanned.fetch_add(1, Ordering::Relaxed) + 1;
                if count % 20 == 0 || count == total {
                    if let Ok(mut p) = progress.lock() {
                        p.scanned = scanned_offset + count;
                    }
                }
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    Arc::try_unwrap(alive).unwrap().into_inner().unwrap()
}

fn rebuild_hosts(
    found: &Arc<Mutex<HashMap<Ipv4Addr, Vec<u16>>>>,
    progress: &Arc<Mutex<ScanProgress>>,
) {
    let snapshot = match found.lock() {
        Ok(f) => f.clone(),
        Err(_) => return,
    };

    let mut hosts: Vec<ScanHost> = snapshot
        .into_iter()
        .map(|(ip, mut ports)| {
            ports.sort();
            ports.dedup();
            let vendor = guess_vendor(&ports);
            ScanHost {
                ip,
                hostname: String::new(),
                mac: String::new(),
                open_ports: ports,
                vendor,
                online: true,
            }
        })
        .collect();

    hosts.sort_by_key(|h| h.ip);

    if let Ok(mut p) = progress.lock() {
        p.hosts = hosts;
    }
}

fn guess_vendor(ports: &[u16]) -> String {
    if ports.contains(&445) && ports.contains(&135) {
        "Windows".to_string()
    } else if ports.contains(&22) && !ports.contains(&445) {
        "Linux/Unix".to_string()
    } else if ports.contains(&548) {
        "macOS".to_string()
    } else if ports.contains(&80) || ports.contains(&443) {
        if ports.contains(&9090) || ports.contains(&5000) {
            "NAS".to_string()
        } else {
            "Web".to_string()
        }
    } else {
        String::new()
    }
}

pub fn port_service_name(port: u16) -> &'static str {
    match port {
        20 => "FTP-D", 21 => "FTP", 22 => "SSH", 23 => "Telnet",
        25 => "SMTP", 26 => "SMTP", 53 => "DNS", 67 => "DHCP",
        68 => "DHCP", 69 => "TFTP", 80 => "HTTP", 81 => "HTTP",
        88 => "Kerb", 110 => "POP3", 111 => "RPC", 119 => "NNTP",
        123 => "NTP", 135 => "MSRPC", 137 => "NBT", 138 => "NBT",
        139 => "NetBIOS", 143 => "IMAP", 161 => "SNMP", 162 => "SNMP",
        179 => "BGP", 389 => "LDAP", 427 => "SLP", 443 => "HTTPS",
        445 => "SMB", 464 => "Kerb", 465 => "SMTPS", 500 => "IKE",
        515 => "LPD", 520 => "RIP", 548 => "AFP", 554 => "RTSP",
        587 => "SMTP", 593 => "RPC", 631 => "IPP", 636 => "LDAPS",
        873 => "Rsync", 902 => "VMware", 993 => "IMAPS", 995 => "POP3S",
        1080 => "SOCKS", 1194 => "OpenVPN", 1433 => "MSSQL",
        1434 => "MSSQL", 1521 => "Oracle", 1701 => "L2TP",
        1723 => "PPTP", 1812 => "RADIUS", 1883 => "MQTT",
        1900 => "UPnP", 2049 => "NFS", 2082 => "cPanel",
        2083 => "cPanel", 2181 => "ZooKeep", 2222 => "SSH",
        3000 => "Dev", 3128 => "Squid", 3268 => "LDAP",
        3306 => "MySQL", 3389 => "RDP", 3690 => "SVN",
        4443 => "HTTPS", 4500 => "IPSec", 5000 => "UPnP",
        5001 => "Synol", 5060 => "SIP", 5222 => "XMPP",
        5353 => "mDNS", 5357 => "WSDAPI", 5432 => "PgSQL",
        5631 => "PCAnyw", 5800 => "VNC-H", 5900 => "VNC",
        5938 => "TeamV", 5984 => "CouchDB", 5985 => "WinRM",
        5986 => "WinRM", 6000 => "X11", 6379 => "Redis",
        6443 => "K8s", 6667 => "IRC", 7000 => "Cassandra",
        7443 => "HTTPS", 8000 => "HTTP", 8008 => "HTTP",
        8080 => "HTTP-P", 8081 => "HTTP", 8088 => "HTTP",
        8181 => "HTTP", 8443 => "HTTPS", 8880 => "HTTP",
        8888 => "HTTP", 8983 => "Solr", 9000 => "PHP",
        9001 => "Tor", 9090 => "Webmin", 9091 => "Trans",
        9100 => "Print", 9200 => "Elast", 9300 => "Elast",
        9418 => "Git", 9443 => "HTTPS", 9999 => "Aapl",
        10000 => "Webmin", 10250 => "K8s", 11211 => "Memcache",
        15672 => "RabbitMQ", 17500 => "Dropbox", 25565 => "MC",
        27017 => "MongoDB", 27018 => "MongoDB", 32400 => "Plex",
        49152 => "WinRPC",
        _ => "",
    }
}
