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

/// Fast hostname resolution: try DNS reverse lookup via `ping -a` (1s timeout).
fn resolve_hostname_fast(ip: Ipv4Addr) -> String {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        // Use ping -a -n 1 -w 500 for reverse DNS
        let output = std::process::Command::new("ping")
            .args(["-a", "-n", "1", "-w", "500", &ip.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            // Parse "Pinging HOSTNAME [ip]" from first line
            if let Some(line) = text.lines().next() {
                let line = line.trim();
                // "Envoi d'une requête 'ping' sur HOSTNAME [192.168.1.1]"
                // or "Pinging HOSTNAME [192.168.1.1]"
                if let Some(bracket_pos) = line.find('[') {
                    let before_bracket = line[..bracket_pos].trim();
                    // Extract last word before [
                    let words: Vec<&str> = before_bracket.split_whitespace().collect();
                    if let Some(&name) = words.last() {
                        let name = name.trim_matches('\'');
                        if !name.is_empty()
                            && name != ip.to_string()
                            && !name.to_lowercase().contains("ping")
                        {
                            return name.to_string();
                        }
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
