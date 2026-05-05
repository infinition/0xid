use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ChildStdin;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::plugins::{apps_dir, discover_entries, Plugin, PluginEntry};
use crate::sounds;
use crate::tray::{TrayEvent, TrayManager};
use crate::ui;
use crate::wol::WolHost;
use crate::wsl::WslDistro;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Launcher,
    Ssh,
    Scanner,
    Wsl,
    Wol,
}

impl Tab {
    pub const ALL: &'static [Tab] = &[Tab::Launcher, Tab::Ssh, Tab::Scanner, Tab::Wsl, Tab::Wol];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Launcher => "LAUNCHER",
            Tab::Ssh => "SSH",
            Tab::Scanner => "SCANNER",
            Tab::Wsl => "WSL2",
            Tab::Wol => "WOL",
        }
    }

    /// Tabs that use full width (no side panel split)
    pub fn is_fullwidth(self) -> bool {
        matches!(self, Tab::Scanner | Tab::Ssh)
    }

    pub fn next(self) -> Tab {
        let idx = Self::ALL.iter().position(|&t| t == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Tab {
        let idx = Self::ALL.iter().position(|&t| t == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshMode {
    HostList,     // Browse/manage saved hosts
    AddHost,      // Adding a new host
    PinEntry,     // Entering PIN to unlock passwords
    Terminal,     // Connected SSH terminal
    Sftp,         // SFTP dual-pane file browser
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SftpFocus {
    Local,
    Remote,
}

pub struct App {
    pub entries: Vec<PluginEntry>,
    pub current_dir: PathBuf,
    pub dir_stack: Vec<(PathBuf, usize)>,
    pub selected: usize,
    pub output: Vec<String>,
    pub status: String,
    pub tick_count: u64,
    start_time: Instant,
    pub shell_mode: bool,
    pub shell_input: String,
    shell_stdin: Option<ChildStdin>,
    running_output: Option<mpsc::Receiver<String>>,
    pub visible: bool,
    should_hide: bool,
    tray: TrayManager,
    tray_ctx_set: bool,
    hwnd_found: bool,
    /// Cached icon textures keyed by plugin path
    pub icon_textures: HashMap<String, egui::TextureHandle>,
    icons_loaded: bool,
    /// Search mode
    pub active_tab: Tab,
    pub search_mode: bool,
    pub search_query: String,
    /// All plugins from all subfolders (flat list for search)
    pub search_all_plugins: Vec<Plugin>,
    /// Filtered results (indices into search_all_plugins)
    pub search_results: Vec<usize>,
    pub search_selected: usize,
    // SSH
    pub ssh_hosts: Vec<crate::ssh::SshHost>,
    pub ssh_selected: usize,
    pub ssh_keys: Vec<std::path::PathBuf>,
    pub ssh_mode: SshMode,
    pub ssh_input: String,
    pub ssh_add_step: u8,    // 0=name, 1=host, 2=port, 3=user, 4=auth, 5=key/pass, 6=pin
    pub ssh_add_buf: Vec<String>, // accumulated add fields
    pub ssh_pin: String,
    pub ssh_pin_unlocked: bool,
    pub ssh_sftp_local_path: std::path::PathBuf,
    pub ssh_sftp_remote_path: String,
    pub ssh_sftp_local_files: Vec<(String, bool)>,  // (name, is_dir)
    pub ssh_sftp_remote_files: Option<Vec<crate::ssh::RemoteFile>>,
    pub ssh_sftp_remote_pending: Option<std::sync::Arc<std::sync::Mutex<Option<Vec<crate::ssh::RemoteFile>>>>>,
    pub ssh_terminal_output: Vec<String>,
    pub ssh_terminal_input: String,
    ssh_stdin: Option<std::process::ChildStdin>,
    ssh_output_rx: Option<mpsc::Receiver<String>>,
    pub ssh_sftp_focus: SftpFocus,
    pub ssh_sftp_local_selected: usize,
    pub ssh_sftp_remote_selected: usize,
    // Scanner
    pub scan_progress: std::sync::Arc<std::sync::Mutex<crate::scanner::ScanProgress>>,
    pub scan_config: crate::scanner::ScanConfig,
    pub scan_selected: usize,
    pub scan_running: bool,
    // WSL
    pub wsl_distros: Vec<WslDistro>,
    pub wsl_selected: usize,
    wsl_last_refresh: u64,
    wsl_pending: Option<std::sync::Arc<std::sync::Mutex<Option<Vec<WslDistro>>>>>,
    // WOL
    pub wol_hosts: Vec<WolHost>,
    pub wol_selected: usize,
    wol_ping_results: Vec<std::sync::Arc<std::sync::Mutex<Option<bool>>>>,
    wol_last_ping: u64,
    // WOL add mode: step 0=name, 1=mac, 2=ip
    pub wol_adding: bool,
    pub wol_add_step: u8,
    pub wol_add_input: String,
    wol_add_name: String,
    wol_add_mac: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, start_hidden: bool) -> Self {
        // Set all text to monospace
        let mut style = (*cc.egui_ctx.style()).clone();
        let mono_14 = egui::FontId::monospace(14.0);
        style.text_styles.insert(egui::TextStyle::Body, mono_14.clone());
        style.text_styles.insert(egui::TextStyle::Button, mono_14.clone());
        style.text_styles.insert(egui::TextStyle::Small, egui::FontId::monospace(12.0));
        style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::monospace(16.0));
        style.text_styles.insert(egui::TextStyle::Monospace, mono_14);
        cc.egui_ctx.set_style(style);

        let tray = TrayManager::new();

        let base_dir = apps_dir();
        let entries = discover_entries(&base_dir, false);
        let plugin_count = entries
            .iter()
            .filter(|e| matches!(e, PluginEntry::Plugin(_)))
            .count();
        let folder_count = entries
            .iter()
            .filter(|e| matches!(e, PluginEntry::Folder { .. }))
            .count();

        let mut output = vec![
            String::new(),
            "> 0xID v3.0 // SYSTEM READY".to_string(),
            String::new(),
        ];

        if plugin_count == 0 && folder_count == 0 {
            output.push(format!("> WARNING: No apps found in {}", base_dir.display()));
            output.push("> Drop .exe / .bat / .ps1 / .py / .lnk files into that folder.".to_string());
            output.push("> Or create plugins.json there for full config.".to_string());
        } else {
            output.push(format!(
                "> {} app(s), {} folder(s) detected. System is armed.",
                plugin_count, folder_count
            ));
        }

        output.push("> Press [?] for keybindings, [ENTER] to launch.".to_string());
        output.push(String::new());
        output.push("> Awaiting orders...".to_string());

        sounds::boot();

        Self {
            entries,
            current_dir: base_dir,
            dir_stack: Vec::new(),
            selected: 0,
            output,
            status: "0xID@ROOT:~/apps$".to_string(),
            tick_count: 0,
            start_time: Instant::now(),
            shell_mode: false,
            shell_input: String::new(),
            shell_stdin: None,
            running_output: None,
            visible: !start_hidden,
            should_hide: false,
            tray,
            tray_ctx_set: false,
            hwnd_found: false,
            icon_textures: HashMap::new(),
            icons_loaded: false,
            active_tab: Tab::Launcher,
            ssh_hosts: crate::ssh::load_hosts(),
            ssh_selected: 0,
            ssh_keys: crate::ssh::detect_ssh_keys(),
            ssh_mode: SshMode::HostList,
            ssh_input: String::new(),
            ssh_add_step: 0,
            ssh_add_buf: Vec::new(),
            ssh_pin: String::new(),
            ssh_pin_unlocked: false,
            ssh_terminal_output: Vec::new(),
            ssh_terminal_input: String::new(),
            ssh_stdin: None,
            ssh_output_rx: None,
            ssh_sftp_local_path: std::env::current_dir().unwrap_or_default(),
            ssh_sftp_remote_path: "/".to_string(),
            ssh_sftp_local_files: Vec::new(),
            ssh_sftp_remote_files: None,
            ssh_sftp_remote_pending: None,
            ssh_sftp_focus: SftpFocus::Local,
            ssh_sftp_local_selected: 0,
            ssh_sftp_remote_selected: 0,
            scan_progress: std::sync::Arc::new(std::sync::Mutex::new(crate::scanner::ScanProgress {
                hosts: Vec::new(),
                scanned: 0,
                total: 0,
                done: true,
                phase: "IDLE".to_string(),
            })),
            scan_config: crate::scanner::ScanConfig::default(),
            scan_selected: 0,
            scan_running: false,
            search_mode: false,
            search_query: String::new(),
            search_all_plugins: Vec::new(),
            search_results: Vec::new(),
            search_selected: 0,
            wsl_distros: Vec::new(),
            wsl_selected: 0,
            wsl_last_refresh: 0,
            wsl_pending: None,
            wol_hosts: crate::wol::load_hosts(),
            wol_selected: 0,
            wol_ping_results: Vec::new(),
            wol_last_ping: 0,
            wol_adding: false,
            wol_add_step: 0,
            wol_add_input: String::new(),
            wol_add_name: String::new(),
            wol_add_mac: String::new(),
        }
    }

    // ── Input handling ──────────────────────────────────────────────────────

    pub fn handle_input(&mut self, ctx: &egui::Context) {
        let mut quit = false;
        let mut hide = false;

        ctx.input(|i| {
            // On Windows, AltGr = Ctrl+Alt. Don't treat as Ctrl shortcut.
            let is_ctrl_only = i.modifiers.ctrl && !i.modifiers.alt;

            // Global: Ctrl+Q = quit
            if is_ctrl_only && i.key_pressed(egui::Key::Q) {
                quit = true;
                return;
            }
            // Global: Ctrl+C = quit (not in shell mode)
            if is_ctrl_only && i.key_pressed(egui::Key::C) && !self.shell_mode {
                quit = true;
                return;
            }

            if self.shell_mode {
                // Shell mode: keys go to shell input
                if i.key_pressed(egui::Key::Escape) {
                    self.exit_shell_mode();
                    return;
                }
                if i.key_pressed(egui::Key::Enter) {
                    self.shell_execute();
                    return;
                }
                if i.key_pressed(egui::Key::Backspace) {
                    self.shell_input.pop();
                    return;
                }

                // Capture typed text
                for event in &i.events {
                    if let egui::Event::Text(text) = event {
                        self.shell_input.push_str(text);
                    }
                }
            } else if self.search_mode {
                // Search mode: filter apps in real-time
                if i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Tab) {
                    self.exit_search_mode();
                    return;
                }
                if i.key_pressed(egui::Key::Enter) {
                    self.activate_search_result();
                    return;
                }
                if i.key_pressed(egui::Key::Backspace) {
                    self.search_query.pop();
                    self.update_search_filter();
                    return;
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    self.search_select_prev();
                    return;
                }
                if i.key_pressed(egui::Key::ArrowDown) {
                    self.search_select_next();
                    return;
                }

                // Capture typed text into search query
                for event in &i.events {
                    if let egui::Event::Text(text) = event {
                        self.search_query.push_str(text);
                        self.update_search_filter();
                    }
                }
            } else {
                // Normal mode — global keys
                // Don't switch tabs when in SSH terminal or SFTP or add modes
                let allow_tab_switch = !matches!(
                    (self.active_tab, &self.ssh_mode),
                    (Tab::Ssh, SshMode::Terminal)
                    | (Tab::Ssh, SshMode::Sftp)
                    | (Tab::Ssh, SshMode::AddHost)
                    | (Tab::Ssh, SshMode::PinEntry)
                );
                if allow_tab_switch && i.key_pressed(egui::Key::Tab) {
                    if i.modifiers.shift {
                        self.active_tab = self.active_tab.prev();
                    } else {
                        self.active_tab = self.active_tab.next();
                    }
                    sounds::nav();
                    return;
                }
                if i.key_pressed(egui::Key::Escape) {
                    hide = true;
                    return;
                }

                // Tab-specific keys
                match self.active_tab {
                    Tab::Launcher => {
                        if i.key_pressed(egui::Key::ArrowUp) { self.select_prev(); return; }
                        if i.key_pressed(egui::Key::ArrowDown) { self.select_next(); return; }
                        if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::Backspace) { self.go_back(); return; }
                        if i.key_pressed(egui::Key::ArrowRight) { self.enter_if_folder(); return; }
                        if i.key_pressed(egui::Key::Enter) { self.activate_selected(); return; }

                        for event in &i.events {
                            if let egui::Event::Text(text) = event {
                                for ch in text.chars() {
                                    match ch {
                                        'q' | 'Q' => hide = true,
                                        'j' | 'J' => self.select_next(),
                                        's' | 'S' => self.enter_search_mode(),
                                        'i' | 'I' => self.toggle_interactive(),
                                        'k' | 'K' => self.kill_selected(),
                                        'r' | 'R' => self.refresh_entries(),
                                        'c' | 'C' => self.clear_output(),
                                        '/' => self.enter_shell_mode(),
                                        '?' => self.show_help(),
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Tab::Ssh => {
                        match self.ssh_mode {
                            SshMode::AddHost | SshMode::PinEntry => {
                                // Text input modes
                                if i.key_pressed(egui::Key::Escape) {
                                    self.ssh_mode = SshMode::HostList;
                                    self.ssh_input.clear();
                                    self.ssh_add_buf.clear();
                                    return;
                                }
                                if i.key_pressed(egui::Key::Enter) {
                                    self.ssh_handle_input_enter();
                                    return;
                                }
                                if i.key_pressed(egui::Key::Backspace) {
                                    self.ssh_input.pop();
                                    return;
                                }
                                for event in &i.events {
                                    if let egui::Event::Text(text) = event {
                                        self.ssh_input.push_str(text);
                                    }
                                }
                            }
                            SshMode::HostList => {
                                if i.key_pressed(egui::Key::ArrowUp) && self.ssh_selected > 0 {
                                    self.ssh_selected -= 1; sounds::nav(); return;
                                }
                                if i.key_pressed(egui::Key::ArrowDown) && self.ssh_selected + 1 < self.ssh_hosts.len() {
                                    self.ssh_selected += 1; sounds::nav(); return;
                                }
                                if i.key_pressed(egui::Key::Enter) {
                                    self.ssh_connect_selected();
                                    return;
                                }
                                for event in &i.events {
                                    if let egui::Event::Text(text) = event {
                                        for ch in text.chars() {
                                            match ch {
                                                'q' | 'Q' => hide = true,
                                                'a' | 'A' => {
                                                    self.ssh_mode = SshMode::AddHost;
                                                    self.ssh_add_step = 0;
                                                    self.ssh_input.clear();
                                                    self.ssh_add_buf.clear();
                                                }
                                                'd' | 'D' => self.ssh_delete_selected(),
                                                't' | 'T' => self.ssh_open_terminal_selected(),
                                                'e' | 'E' => self.ssh_open_terminal_external(),
                                                'f' | 'F' => self.ssh_open_sftp_selected(),
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            SshMode::Sftp => {
                                if i.key_pressed(egui::Key::Escape) {
                                    self.ssh_mode = SshMode::HostList;
                                    return;
                                }
                                // TAB switches focus between local/remote
                                if i.key_pressed(egui::Key::Tab) {
                                    self.ssh_sftp_focus = match self.ssh_sftp_focus {
                                        SftpFocus::Local => SftpFocus::Remote,
                                        SftpFocus::Remote => SftpFocus::Local,
                                    };
                                    sounds::nav();
                                    return;
                                }
                                if i.key_pressed(egui::Key::ArrowUp) {
                                    match self.ssh_sftp_focus {
                                        SftpFocus::Local if self.ssh_sftp_local_selected > 0 => {
                                            self.ssh_sftp_local_selected -= 1; sounds::nav();
                                        }
                                        SftpFocus::Remote if self.ssh_sftp_remote_selected > 0 => {
                                            self.ssh_sftp_remote_selected -= 1; sounds::nav();
                                        }
                                        _ => {}
                                    }
                                    return;
                                }
                                if i.key_pressed(egui::Key::ArrowDown) {
                                    match self.ssh_sftp_focus {
                                        SftpFocus::Local => {
                                            if self.ssh_sftp_local_selected + 1 < self.ssh_sftp_local_files.len() {
                                                self.ssh_sftp_local_selected += 1; sounds::nav();
                                            }
                                        }
                                        SftpFocus::Remote => {
                                            let count = self.ssh_sftp_remote_files.as_ref().map(|f| f.len()).unwrap_or(0);
                                            if self.ssh_sftp_remote_selected + 1 < count {
                                                self.ssh_sftp_remote_selected += 1; sounds::nav();
                                            }
                                        }
                                    }
                                    return;
                                }
                                if i.key_pressed(egui::Key::Enter) {
                                    self.ssh_sftp_enter();
                                    return;
                                }
                                for event in &i.events {
                                    if let egui::Event::Text(text) = event {
                                        for ch in text.chars() {
                                            match ch {
                                                'u' | 'U' => self.ssh_sftp_upload(),
                                                'd' | 'D' => self.ssh_sftp_download(),
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            SshMode::Terminal => {
                                if i.key_pressed(egui::Key::Escape) {
                                    // Disconnect
                                    if let Some(ref mut stdin) = self.ssh_stdin {
                                        use std::io::Write;
                                        let _ = writeln!(stdin, "exit");
                                    }
                                    self.ssh_stdin = None;
                                    self.ssh_output_rx = None;
                                    self.ssh_terminal_output.push("> [SSH] Disconnected.".to_string());
                                    self.ssh_mode = SshMode::HostList;
                                    sounds::back();
                                    return;
                                }
                                if i.key_pressed(egui::Key::Enter) {
                                    let cmd = self.ssh_terminal_input.trim().to_string();
                                    self.ssh_terminal_input.clear();
                                    if !cmd.is_empty() {
                                        self.ssh_terminal_output.push(format!("> $ {}", cmd));
                                        if let Some(ref mut stdin) = self.ssh_stdin {
                                            use std::io::Write;
                                            let _ = writeln!(stdin, "{}", cmd);
                                        }
                                    }
                                    return;
                                }
                                if i.key_pressed(egui::Key::Backspace) {
                                    self.ssh_terminal_input.pop();
                                    return;
                                }
                                for event in &i.events {
                                    if let egui::Event::Text(text) = event {
                                        self.ssh_terminal_input.push_str(text);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Tab::Scanner => {
                        for event in &i.events {
                            if let egui::Event::Text(text) = event {
                                for ch in text.chars() {
                                    match ch {
                                        'q' | 'Q' => hide = true,
                                        's' | 'S' => {
                                            if !self.scan_running {
                                                self.scan_running = true;
                                                self.push(format!("> [SCAN] Scanning {}.{}-{}...",
                                                    self.scan_config.subnet, self.scan_config.start, self.scan_config.end));
                                                crate::scanner::scan_async(
                                                    self.scan_config.clone(),
                                                    self.scan_progress.clone(),
                                                );
                                                sounds::launch();
                                            }
                                        }
                                        'c' | 'C' => self.clear_output(),
                                        _ => {}
                                    }
                                }
                            }
                        }
                        if i.key_pressed(egui::Key::ArrowUp) && self.scan_selected > 0 {
                            self.scan_selected -= 1; sounds::nav();
                        }
                        let host_count = self.scan_progress.lock().map(|p| p.hosts.len()).unwrap_or(0);
                        if i.key_pressed(egui::Key::ArrowDown) && self.scan_selected + 1 < host_count {
                            self.scan_selected += 1; sounds::nav();
                        }
                    }
                    Tab::Wsl => {
                        if i.key_pressed(egui::Key::ArrowUp) && self.wsl_selected > 0 {
                            self.wsl_selected -= 1; sounds::nav(); return;
                        }
                        if i.key_pressed(egui::Key::ArrowDown) && self.wsl_selected + 1 < self.wsl_distros.len() {
                            self.wsl_selected += 1; sounds::nav(); return;
                        }
                        if i.key_pressed(egui::Key::Enter) { self.wsl_open_terminal(); return; }

                        for event in &i.events {
                            if let egui::Event::Text(text) = event {
                                for ch in text.chars() {
                                    match ch {
                                        'q' | 'Q' => hide = true,
                                        's' | 'S' => { self.wsl_start_selected(); sounds::launch(); }
                                        'x' | 'X' => { self.wsl_stop_selected(); }
                                        'r' | 'R' => { self.refresh_wsl(); sounds::nav(); }
                                        't' | 'T' => self.wsl_open_terminal(),
                                        'c' | 'C' => self.clear_output(),
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Tab::Wol => {
                        if self.wol_adding {
                            // WOL add mode: input fields
                            if i.key_pressed(egui::Key::Escape) {
                                self.wol_adding = false;
                                self.wol_add_input.clear();
                                return;
                            }
                            if i.key_pressed(egui::Key::Enter) {
                                match self.wol_add_step {
                                    0 => {
                                        self.wol_add_name = self.wol_add_input.trim().to_string();
                                        self.wol_add_input.clear();
                                        self.wol_add_step = 1;
                                    }
                                    1 => {
                                        self.wol_add_mac = self.wol_add_input.trim().to_string();
                                        self.wol_add_input.clear();
                                        self.wol_add_step = 2;
                                    }
                                    2 => {
                                        let ip = self.wol_add_input.trim().to_string();
                                        let name = self.wol_add_name.clone();
                                        let mac = self.wol_add_mac.clone();
                                        self.wol_add_input.clear();
                                        self.wol_adding = false;
                                        self.wol_add_step = 0;
                                        self.wol_add_host(name, mac, ip);
                                    }
                                    _ => {}
                                }
                                return;
                            }
                            if i.key_pressed(egui::Key::Backspace) {
                                self.wol_add_input.pop();
                                return;
                            }
                            for event in &i.events {
                                if let egui::Event::Text(text) = event {
                                    self.wol_add_input.push_str(text);
                                }
                            }
                            return;
                        }

                        // WOL normal mode
                        if i.key_pressed(egui::Key::ArrowUp) && self.wol_selected > 0 {
                            self.wol_selected -= 1; sounds::nav(); return;
                        }
                        if i.key_pressed(egui::Key::ArrowDown) && self.wol_selected + 1 < self.wol_hosts.len() {
                            self.wol_selected += 1; sounds::nav(); return;
                        }
                        if i.key_pressed(egui::Key::Enter) { self.wol_wake_selected(); return; }

                        for event in &i.events {
                            if let egui::Event::Text(text) = event {
                                for ch in text.chars() {
                                    match ch {
                                        'q' | 'Q' => hide = true,
                                        'w' | 'W' => self.wol_wake_selected(),
                                        'a' | 'A' => {
                                            self.wol_adding = true;
                                            self.wol_add_step = 0;
                                            self.wol_add_input.clear();
                                        }
                                        'd' | 'D' => self.wol_delete_selected(),
                                        'r' | 'R' => { self.refresh_wol_ping(); sounds::nav(); }
                                        'c' | 'C' => self.clear_output(),
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if hide {
            self.should_hide = true;
        }
    }

    // ── Mouse handling for plugin list ───────────────────────────────────────

    pub fn click_entry(&mut self, index: usize) {
        if index < self.entries.len() {
            if self.selected == index {
                self.activate_selected();
            } else {
                self.selected = index;
                sounds::nav();
            }
        }
    }

    // ── Navigation ───────────────────────────────────────────────────────────

    fn select_prev(&mut self) {
        if !self.entries.is_empty() && self.selected > 0 {
            self.selected -= 1;
            sounds::nav();
        }
    }

    fn select_next(&mut self) {
        if !self.entries.is_empty() && self.selected + 1 < self.entries.len() {
            self.selected += 1;
            sounds::nav();
        }
    }

    /// Right arrow: only enter folders or go back, ignore plugins
    fn enter_if_folder(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let entry = self.entries[self.selected].clone();
        match entry {
            PluginEntry::Folder { name, path } => self.enter_folder(&name, path),
            _ => {} // do nothing for plugins and back
        }
    }

    fn activate_selected(&mut self) {
        if self.entries.is_empty() {
            self.push("> ERROR: No entries. Add files to ./plugins/  directory");
            sounds::error();
            return;
        }
        let entry = self.entries[self.selected].clone();
        match entry {
            PluginEntry::Back => self.go_back(),
            PluginEntry::Folder { name, path } => self.enter_folder(&name, path),
            PluginEntry::Plugin(plugin) => {
                self.launch_plugin(plugin);
            }
        }
    }

    fn enter_folder(&mut self, _name: &str, path: PathBuf) {
        self.dir_stack
            .push((self.current_dir.clone(), self.selected));
        self.current_dir = path;
        self.entries = discover_entries(&self.current_dir, true);
        self.selected = if self.entries.len() > 1 { 1 } else { 0 };
        self.status = format!("0xID@ROOT:~/{}", self.relative_path());
        self.icons_loaded = false; // reload icons for new folder
        sounds::enter();
    }

    fn go_back(&mut self) {
        if let Some((prev_dir, prev_selected)) = self.dir_stack.pop() {
            self.current_dir = prev_dir;
            let is_sub = !self.dir_stack.is_empty();
            self.entries = discover_entries(&self.current_dir, is_sub);
            self.selected = prev_selected.min(self.entries.len().saturating_sub(1));
            self.status = format!("0xID@ROOT:~/{}", self.relative_path());
            self.icons_loaded = false;
            sounds::back();
        }
    }

    fn toggle_interactive(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let (name, mode) = {
            if let PluginEntry::Plugin(ref mut plugin) = self.entries[self.selected] {
                plugin.interactive = !plugin.interactive;
                let m = if plugin.interactive {
                    "INTERACTIVE (own window)"
                } else {
                    "INLINE (output here)"
                };
                (plugin.name.clone(), m)
            } else {
                return;
            }
        };
        self.push(format!("> {} -> {}", name, mode));
        sounds::nav();
    }

    fn kill_selected(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let plugin = match &self.entries[self.selected] {
            PluginEntry::Plugin(p) => p,
            _ => return,
        };

        let path_lower = plugin.path.to_lowercase();
        let kill_target = if path_lower.ends_with(".lnk") {
            // .lnk → strip extension, assume target has same base name + .exe
            let base = std::path::Path::new(&plugin.path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if base.is_empty() { return; }
            format!("{}.exe", base)
        } else if path_lower.ends_with(".py") {
            // .py → kill the python process
            "python.exe".to_string()
        } else {
            let f = std::path::Path::new(&plugin.path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if f.is_empty() { return; }
            f
        };

        self.push(String::new());
        self.push(format!("> [KILL] Terminating: {}...", kill_target));
        let filename = kill_target;

        // Run taskkill in a background thread to avoid freezing the UI
        let (tx, rx) = mpsc::channel::<String>();
        self.running_output = Some(rx);

        std::thread::spawn(move || {
            let mut cmd = std::process::Command::new("taskkill");
            cmd.args(["/F", "/IM", &filename]);

            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }

            let result = cmd.output();

            match result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines() {
                        if !line.trim().is_empty() {
                            let _ = tx.send(format!("  {}", line.trim()));
                        }
                    }
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    for line in stderr.lines() {
                        if !line.trim().is_empty() {
                            let _ = tx.send(format!("! {}", line.trim()));
                        }
                    }

                    if output.status.success() {
                        let _ = tx.send(format!("> [OK] {} killed", filename));
                    } else {
                        let _ = tx.send(format!("> [FAIL] Could not kill {}", filename));
                    }
                }
                Err(e) => {
                    let _ = tx.send(format!("> [ERROR] taskkill failed: {e}"));
                }
            }
        });
    }

    fn relative_path(&self) -> String {
        self.current_dir.to_string_lossy().replace('\\', "/")
    }

    // ── Shell mode ──────────────────────────────────────────────────────────

    // ── Search mode ─────────────────────────────────────────────────────

    fn enter_search_mode(&mut self) {
        // Build flat list of ALL plugins recursively
        let base_dir = apps_dir();
        self.search_all_plugins = crate::plugins::discover_all_plugins(&base_dir);
        self.search_mode = true;
        self.search_query.clear();
        self.search_selected = 0;
        self.update_search_filter();
        self.status = "0xID@SEARCH:~$ type to filter...".to_string();
        sounds::enter();
    }

    fn exit_search_mode(&mut self) {
        self.search_mode = false;
        self.search_query.clear();
        self.search_results.clear();
        self.search_all_plugins.clear();
        self.status = format!("0xID@ROOT:~/{}", self.relative_path());
        sounds::back();
    }

    fn update_search_filter(&mut self) {
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            self.search_results = (0..self.search_all_plugins.len()).collect();
        } else {
            self.search_results = self
                .search_all_plugins
                .iter()
                .enumerate()
                .filter(|(_, p)| p.name.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect();
        }
        self.search_selected = 0;
    }

    fn search_select_prev(&mut self) {
        if self.search_selected > 0 {
            self.search_selected -= 1;
            sounds::nav();
        }
    }

    fn search_select_next(&mut self) {
        if self.search_selected + 1 < self.search_results.len() {
            self.search_selected += 1;
            sounds::nav();
        }
    }

    pub fn activate_search_result(&mut self) {
        if self.search_results.is_empty() {
            return;
        }
        let idx = self.search_results[self.search_selected];
        let plugin = self.search_all_plugins[idx].clone();
        self.search_mode = false;
        self.search_query.clear();
        self.search_results.clear();
        self.search_all_plugins.clear();
        self.launch_plugin(plugin);
    }

    // ── Shell mode ──────────────────────────────────────────────────────────

    fn enter_shell_mode(&mut self) {
        use std::io::BufRead;
        use std::process::{Command, Stdio};

        let (tx, rx) = mpsc::channel::<String>();

        let mut cmd_builder = Command::new("cmd");
        cmd_builder
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd_builder.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = match cmd_builder.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.push(format!("> [ERROR] Failed to start shell: {e}"));
                sounds::error();
                return;
            }
        };

        let stdin = child.stdin.take().unwrap();

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

        self.shell_stdin = Some(stdin);
        self.running_output = Some(rx);
        self.shell_mode = true;
        self.shell_input.clear();

        self.push(String::new());
        self.push("> [SHELL] Entering shell mode. Type commands, press ENTER to run.");
        self.push("> [SHELL] Press ESC to exit shell mode.");
        self.status = "0xID@SHELL:~$ _".to_string();
        sounds::enter();
    }

    fn exit_shell_mode(&mut self) {
        use std::io::Write;

        if let Some(ref mut stdin) = self.shell_stdin {
            let _ = writeln!(stdin, "exit");
        }
        self.shell_stdin = None;
        self.shell_mode = false;
        self.shell_input.clear();

        self.push(String::new());
        self.push("> [SHELL] Exited shell mode.");
        self.status = format!("0xID@ROOT:~/{}", self.relative_path());
        sounds::back();
    }

    fn shell_execute(&mut self) {
        use std::io::Write;

        let cmd = self.shell_input.trim().to_string();
        self.shell_input.clear();

        if cmd.is_empty() {
            return;
        }

        self.push(format!("> $ {}", cmd));

        if let Some(ref mut stdin) = self.shell_stdin {
            if writeln!(stdin, "{}", cmd).is_err() {
                self.push("> [ERROR] Shell pipe broken. Exiting shell mode.");
                self.exit_shell_mode();
                sounds::error();
            }
        }
    }

    // ── Plugin launch ────────────────────────────────────────────────────────

    fn launch_plugin(&mut self, plugin: Plugin) {
        sounds::launch();

        let is_python = plugin.path.to_lowercase().ends_with(".py");

        self.push(String::new());
        self.push("\u{2550}".repeat(52));
        self.push(format!("> EXEC  : {}", plugin.name));
        if let Some(ref desc) = plugin.description {
            self.push(format!("> INFO  : {}", desc));
        }
        self.push(format!("> PATH  : {}", plugin.path));
        self.push("\u{2550}".repeat(52));

        if is_python && !plugin.interactive {
            self.launch_python_captured(plugin);
        } else {
            self.launch_external(plugin);
        }
    }

    fn launch_python_captured(&mut self, plugin: Plugin) {
        let (tx, rx) = mpsc::channel::<String>();
        self.running_output = Some(rx);
        self.status = format!("0xID@ROOT:~$ {} [RUNNING...]", plugin.name);

        let path = plugin.path.clone();
        let args = plugin.args.clone();
        let name = plugin.name.clone();

        std::thread::spawn(move || {
            use std::io::BufRead;
            use std::process::Stdio;

            let mut cmd = std::process::Command::new("cmd");
            cmd.args(["/C", "conda", "run", "-n", "0xid", "python", &path]);
            if let Some(ref a) = args {
                cmd.args(a);
            }
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            // Hide the cmd.exe window (no flash)
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(format!("> [ERROR] Launch failed: {e}"));
                    return;
                }
            };

            let tx_out = tx.clone();
            let stdout_handle = child.stdout.take().map(|out| {
                std::thread::spawn(move || {
                    for line in std::io::BufReader::new(out).lines().flatten() {
                        if tx_out.send(format!("  {}", line)).is_err() {
                            break;
                        }
                    }
                })
            });

            let tx_err = tx.clone();
            let stderr_handle = child.stderr.take().map(|err| {
                std::thread::spawn(move || {
                    for line in std::io::BufReader::new(err).lines().flatten() {
                        if tx_err.send(format!("! {}", line)).is_err() {
                            break;
                        }
                    }
                })
            });

            if let Some(h) = stdout_handle {
                let _ = h.join();
            }
            if let Some(h) = stderr_handle {
                let _ = h.join();
            }

            match child.wait() {
                Ok(status) => {
                    let code = status.code().unwrap_or(-1);
                    let tag = if status.success() { "OK" } else { "FAIL" };
                    let _ = tx.send(format!("> [{tag}] {name} exited with code {code}"));
                }
                Err(e) => {
                    let _ = tx.send(format!("> [ERROR] {name}: {e}"));
                }
            }
        });
    }

    fn launch_external(&mut self, plugin: Plugin) {
        let is_lnk = plugin.path.to_lowercase().ends_with(".lnk");

        let mut cmd = if is_lnk {
            // .lnk must be launched via cmd /c start to resolve the shortcut
            let mut c = std::process::Command::new("cmd");
            let abs_path = std::fs::canonicalize(&plugin.path)
                .unwrap_or_else(|_| PathBuf::from(&plugin.path));
            c.args(["/C", "start", "", &abs_path.to_string_lossy()]);
            c
        } else {
            let mut c = std::process::Command::new(&plugin.path);
            if let Some(ref args) = plugin.args {
                c.args(args);
            }
            c
        };

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            if is_lnk {
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            } else {
                const CREATE_NEW_CONSOLE: u32 = 0x00000010;
                cmd.creation_flags(CREATE_NEW_CONSOLE);
            }
        }

        match cmd.spawn() {
            Ok(_) => {
                self.push(format!("> [LAUNCHED] {}", plugin.name));
                self.status = format!("0xID@ROOT:~$ {} [LAUNCHED]", plugin.name);
            }
            Err(e) => {
                self.push(format!("> [ERROR] Launch failed: {e}"));
                self.status = format!("0xID@ROOT:~$ ERROR: {e}");
                sounds::error();
                return;
            }
        }

        // Hide to tray only for non-interactive plugins
        if !plugin.interactive {
            self.should_hide = true;
        }
    }

    // ── Output ───────────────────────────────────────────────────────────────

    fn push(&mut self, line: impl Into<String>) {
        self.output.push(line.into());
    }

    fn clear_output(&mut self) {
        self.output.clear();
        self.push("> Output cleared. Awaiting orders...");
    }

    fn show_help(&mut self) {
        self.push(String::new());
        self.push("  KEYBINDINGS");
        self.push("  \u{2500}".to_string() + &"\u{2500}".repeat(45));
        self.push("  [\u{2191}/k] [\u{2193}/j]       Navigate entries");
        self.push("  [ENTER/\u{2192}/l]       Open folder / Launch plugin");
        self.push("  [\u{2190}/Bksp]          Go back (parent folder)");
        self.push("  [CLICK]             Select  (double = activate)");
        self.push("  [Mouse Wheel]       Scroll output");
        self.push("  [S]                 Search apps (filter)");
        self.push("  [I]                 Toggle interactive mode");
        self.push("  [K]                 Kill selected process");
        self.push("  [/]                 Shell mode (type commands)");
        self.push("  [R]                 Refresh");
        self.push("  [C]                 Clear output");
        self.push("  [?]                 This help");
        self.push("  [Q/ESC]             Minimize to tray");
        self.push("  [Ctrl+Q/Ctrl+C]     Quit completely");
        self.push("  [Ctrl+Shift+Space]  Restore from tray");
        self.push(String::new());
        self.push("> .py scripts run inline (output here). Use [I] for GUI scripts.");
        self.push("> Supports: .exe .bat .cmd .ps1 .py .lnk (conda 0xid for .py)");
    }

    pub fn refresh_entries(&mut self) {
        let is_sub = !self.dir_stack.is_empty();
        self.entries = discover_entries(&self.current_dir, is_sub);
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        let plugin_count = self
            .entries
            .iter()
            .filter(|e| matches!(e, PluginEntry::Plugin(_)))
            .count();
        let folder_count = self
            .entries
            .iter()
            .filter(|e| matches!(e, PluginEntry::Folder { .. }))
            .count();
        self.push(String::new());
        self.push(format!(
            "> [REFRESH] {} app(s), {} folder(s) found.",
            plugin_count, folder_count
        ));
        self.status = "0xID@ROOT:~$ refresh complete".to_string();
        self.icons_loaded = false; // reload icons
    }

    pub fn tick(&mut self) {
        self.tick_count = (self.start_time.elapsed().as_millis() / 50) as u64;
        self.drain_running_output();

        // Drain SSH terminal output
        if let Some(ref rx) = self.ssh_output_rx {
            let mut got_data = false;
            loop {
                match rx.try_recv() {
                    Ok(line) => {
                        self.ssh_terminal_output.push(line);
                        got_data = true;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        // SSH process ended
                        self.ssh_terminal_output.push("> [SSH] Connection closed.".to_string());
                        self.ssh_output_rx = None;
                        self.ssh_stdin = None;
                        break;
                    }
                }
            }
            let _ = got_data;
        }

        // Poll SFTP remote file listing
        if let Some(ref pending) = self.ssh_sftp_remote_pending {
            if let Ok(guard) = pending.lock() {
                if let Some(files) = guard.as_ref() {
                    self.ssh_sftp_remote_files = Some(files.clone());
                    self.ssh_sftp_remote_selected = 0;
                }
            }
            if self.ssh_sftp_remote_files.is_some() {
                self.ssh_sftp_remote_pending = None;
            }
        }

        // Check scan progress/completion
        if self.scan_running {
            let scan_info = self.scan_progress.lock().ok().map(|p| {
                (p.done, p.hosts.len(), p.scanned, p.total)
            });

            if let Some((done, host_count, scanned, total)) = scan_info {
                if done {
                    self.scan_running = false;
                    self.push(format!("> [SCAN] Complete. {} host(s) found.", host_count));
                    self.status = "0xID@SCANNER:~$ scan complete".to_string();
                } else if total > 0 {
                    let pct = scanned * 100 / total;
                    self.status = format!("0xID@SCANNER:~$ {}% ({} hosts)", pct, host_count);
                }
            }
        }

        // Poll async WSL results
        let mut wsl_done = false;
        if let Some(pending) = &self.wsl_pending {
            if let Ok(guard) = pending.lock() {
                if let Some(distros) = guard.as_ref() {
                    self.wsl_distros = distros.clone();
                    self.wsl_selected = self.wsl_selected.min(self.wsl_distros.len().saturating_sub(1));
                    wsl_done = true;
                }
            }
        }
        if wsl_done {
            self.wsl_pending = None;
        }

        // Auto-refresh WSL every ~5s when on WSL tab
        if self.active_tab == Tab::Wsl
            && self.wsl_pending.is_none()
            && self.tick_count.wrapping_sub(self.wsl_last_refresh) > 100
        {
            self.wsl_last_refresh = self.tick_count;
            self.refresh_wsl();
        }

        // Auto-ping WOL hosts every ~10s when on WOL tab
        if self.active_tab == Tab::Wol {
            self.update_wol_status();
            if self.tick_count.wrapping_sub(self.wol_last_ping) > 200 {
                self.wol_last_ping = self.tick_count;
                self.refresh_wol_ping();
            }
        }
    }

    fn drain_running_output(&mut self) {
        let rx = match self.running_output.take() {
            Some(rx) => rx,
            None => return,
        };

        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(line) => {
                    self.output.push(line);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        if disconnected {
            while let Ok(line) = rx.try_recv() {
                self.output.push(line);
            }
            self.status = "0xID@ROOT:~$ process complete".to_string();
        } else {
            self.running_output = Some(rx);
        }
    }

    /// Load icons for all plugin entries (call when entries change)
    // ── SSH operations ────────────────────────────────────────────────────

    fn ssh_handle_input_enter(&mut self) {
        match self.ssh_mode {
            SshMode::PinEntry => {
                self.ssh_pin = self.ssh_input.clone();
                self.ssh_input.clear();
                self.ssh_pin_unlocked = true;
                // Now retry the connection
                self.ssh_mode = SshMode::HostList;
                self.ssh_open_terminal_selected();
            }
            SshMode::AddHost => {
                let val = self.ssh_input.trim().to_string();
                self.ssh_input.clear();

                match self.ssh_add_step {
                    0 => { // Name
                        self.ssh_add_buf.push(val);
                        self.ssh_add_step = 1;
                    }
                    1 => { // Hostname
                        self.ssh_add_buf.push(val);
                        self.ssh_add_step = 2;
                    }
                    2 => { // Port (default 22)
                        let port = if val.is_empty() { "22".to_string() } else { val };
                        self.ssh_add_buf.push(port);
                        self.ssh_add_step = 3;
                    }
                    3 => { // Username
                        self.ssh_add_buf.push(val);
                        self.ssh_add_step = 4;
                    }
                    4 => { // Auth method: k=key, p=password
                        let auth = if val.to_lowercase().starts_with('k') { "key" } else { "password" };
                        self.ssh_add_buf.push(auth.to_string());
                        if auth == "key" {
                            self.ssh_add_step = 5; // key path
                        } else {
                            self.ssh_add_step = 6; // password
                        }
                    }
                    5 => { // Key path
                        self.ssh_add_buf.push(val);
                        self.ssh_finalize_add_host();
                    }
                    6 => { // Password → needs PIN first
                        self.ssh_add_buf.push(val);
                        if !self.ssh_pin_unlocked {
                            self.ssh_add_step = 7; // ask for PIN
                        } else {
                            self.ssh_finalize_add_host();
                        }
                    }
                    7 => { // PIN for encryption
                        self.ssh_pin = val;
                        self.ssh_pin_unlocked = true;
                        self.ssh_finalize_add_host();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn ssh_finalize_add_host(&mut self) {
        let buf = &self.ssh_add_buf;
        if buf.len() < 5 {
            return;
        }

        let name = buf[0].clone();
        let hostname = buf[1].clone();
        let port: u16 = buf[2].parse().unwrap_or(22);
        let username = buf[3].clone();
        let auth_str = &buf[4];

        let (auth, key_path, encrypted_password) = if auth_str == "key" {
            let key = buf.get(5).cloned().or_else(|| {
                self.ssh_keys.first().map(|p| p.to_string_lossy().to_string())
            });
            (crate::ssh::AuthMethod::Key, key, None)
        } else {
            let password = buf.get(5).cloned().unwrap_or_default();
            let encrypted = crate::ssh::encrypt_password(&password, &self.ssh_pin);
            (crate::ssh::AuthMethod::Password, None, encrypted)
        };

        let id = format!("{}_{}", hostname, port);
        let host = crate::ssh::SshHost {
            id,
            name: name.clone(),
            hostname,
            port,
            username,
            auth,
            encrypted_password,
            key_path,
            remote_dir: None,
        };

        self.ssh_hosts.push(host);
        crate::ssh::save_hosts(&self.ssh_hosts);
        self.push(format!("> [SSH] Host added: {}", name));
        self.ssh_mode = SshMode::HostList;
        self.ssh_add_buf.clear();
        sounds::enter();
    }

    fn ssh_delete_selected(&mut self) {
        if self.ssh_hosts.is_empty() {
            return;
        }
        let name = self.ssh_hosts[self.ssh_selected].name.clone();
        self.ssh_hosts.remove(self.ssh_selected);
        self.ssh_selected = self.ssh_selected.min(self.ssh_hosts.len().saturating_sub(1));
        crate::ssh::save_hosts(&self.ssh_hosts);
        self.push(format!("> [SSH] Removed: {}", name));
    }

    fn ssh_connect_selected(&mut self) {
        self.ssh_open_terminal_selected();
    }

    pub fn ssh_open_terminal_selected(&mut self) {
        if let Some(host) = self.ssh_hosts.get(self.ssh_selected).cloned() {
            // Decrypt password if needed
            let password = if host.auth == crate::ssh::AuthMethod::Password {
                host.encrypted_password.as_ref().and_then(|enc| {
                    if self.ssh_pin_unlocked {
                        crate::ssh::decrypt_password(enc, &self.ssh_pin)
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            // If password auth but no PIN unlocked, ask for PIN first
            if host.auth == crate::ssh::AuthMethod::Password && password.is_none() && !self.ssh_pin_unlocked {
                self.ssh_mode = SshMode::PinEntry;
                self.ssh_input.clear();
                return;
            }

            match crate::ssh::start_inline_ssh(&host, password.as_deref()) {
                Ok((stdin, rx)) => {
                    self.ssh_stdin = Some(stdin);
                    self.ssh_output_rx = Some(rx);
                    self.ssh_terminal_output.clear();
                    self.ssh_terminal_output.push(format!("> [SSH] Connecting to {}...", host.display()));
                    self.ssh_terminal_input.clear();
                    self.ssh_mode = SshMode::Terminal;
                    self.status = format!("0xID@SSH:{}$ _", host.name);
                    sounds::enter();
                }
                Err(e) => {
                    self.push(format!("> [ERROR] {}", e));
                    sounds::error();
                }
            }
        }
    }

    /// Open SSH in external Windows Terminal (fallback)
    pub fn ssh_open_terminal_external(&mut self) {
        if let Some(host) = self.ssh_hosts.get(self.ssh_selected) {
            crate::ssh::open_ssh_terminal_external(host);
            self.push(format!("> [SSH] External terminal: {}", host.display()));
        }
    }

    fn ssh_open_sftp_selected(&mut self) {
        if self.ssh_hosts.is_empty() {
            return;
        }
        self.ssh_mode = SshMode::Sftp;
        self.ssh_sftp_local_path = std::env::current_dir().unwrap_or_default();
        self.ssh_sftp_remote_path = self.ssh_hosts[self.ssh_selected]
            .remote_dir
            .clone()
            .unwrap_or_else(|| "/home".to_string());
        self.ssh_refresh_local_files();
        self.ssh_refresh_remote_files();
        sounds::enter();
    }

    fn ssh_refresh_local_files(&mut self) {
        let mut files = Vec::new();
        // Add parent dir entry
        files.push(("..".to_string(), true));

        if let Ok(entries) = std::fs::read_dir(&self.ssh_sftp_local_path) {
            let mut items: Vec<_> = entries.flatten().collect();
            items.sort_by_key(|e| e.file_name());
            for entry in items {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.path().is_dir();
                files.push((name, is_dir));
            }
        }
        self.ssh_sftp_local_files = files;
        self.ssh_sftp_local_selected = 0;
    }

    fn ssh_refresh_remote_files(&mut self) {
        if let Some(host) = self.ssh_hosts.get(self.ssh_selected) {
            let result = std::sync::Arc::new(std::sync::Mutex::new(None));
            self.ssh_sftp_remote_pending = Some(result.clone());
            self.ssh_sftp_remote_files = None;
            let password = host.encrypted_password.as_ref().and_then(|enc| {
                if self.ssh_pin_unlocked {
                    crate::ssh::decrypt_password(enc, &self.ssh_pin)
                } else {
                    None
                }
            });
            crate::ssh::list_remote_dir_async(host, &self.ssh_sftp_remote_path, password, result);
        }
    }

    fn ssh_sftp_enter(&mut self) {
        match self.ssh_sftp_focus {
            SftpFocus::Local => {
                if let Some((name, is_dir)) = self.ssh_sftp_local_files.get(self.ssh_sftp_local_selected).cloned() {
                    if is_dir {
                        if name == ".." {
                            self.ssh_sftp_local_path.pop();
                        } else {
                            self.ssh_sftp_local_path.push(&name);
                        }
                        self.ssh_refresh_local_files();
                    }
                }
            }
            SftpFocus::Remote => {
                if let Some(files) = &self.ssh_sftp_remote_files {
                    if let Some(file) = files.get(self.ssh_sftp_remote_selected) {
                        if file.is_dir {
                            if file.name == ".." {
                                // Go up
                                let p = std::path::PathBuf::from(&self.ssh_sftp_remote_path);
                                self.ssh_sftp_remote_path = p.parent()
                                    .unwrap_or(std::path::Path::new("/"))
                                    .to_string_lossy()
                                    .to_string();
                            } else {
                                self.ssh_sftp_remote_path = format!("{}/{}", self.ssh_sftp_remote_path.trim_end_matches('/'), file.name);
                            }
                            self.ssh_refresh_remote_files();
                        }
                    }
                }
            }
        }
    }

    fn ssh_sftp_upload(&mut self) {
        if let Some((name, _)) = self.ssh_sftp_local_files.get(self.ssh_sftp_local_selected).cloned() {
            if name == ".." { return; }
            let local = self.ssh_sftp_local_path.join(&name).to_string_lossy().to_string();
            let remote = format!("{}/{}", self.ssh_sftp_remote_path.trim_end_matches('/'), name);
            if let Some(host) = self.ssh_hosts.get(self.ssh_selected) {
                let (tx, rx) = mpsc::channel();
                self.running_output = Some(rx);
                crate::ssh::scp_upload_async(host, &local, &remote, tx);
            }
        }
    }

    fn ssh_sftp_download(&mut self) {
        if let Some(files) = &self.ssh_sftp_remote_files {
            if let Some(file) = files.get(self.ssh_sftp_remote_selected) {
                let remote = format!("{}/{}", self.ssh_sftp_remote_path.trim_end_matches('/'), file.name);
                let local = self.ssh_sftp_local_path.to_string_lossy().to_string();
                if let Some(host) = self.ssh_hosts.get(self.ssh_selected) {
                    let (tx, rx) = mpsc::channel();
                    self.running_output = Some(rx);
                    crate::ssh::scp_download_async(host, &remote, &local, tx);
                }
            }
        }
    }

    // ── WSL operations ───────────────────────────────────────────────────

    pub fn refresh_wsl(&mut self) {
        if self.wsl_pending.is_some() {
            return; // already refreshing
        }
        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        self.wsl_pending = Some(result.clone());
        crate::wsl::list_distros_async(result);
    }

    pub fn wsl_start_selected(&mut self) {
        if let Some(distro) = self.wsl_distros.get(self.wsl_selected) {
            let name = distro.name.clone();
            self.push(format!("> [WSL] Starting {}...", name));
            let (tx, rx) = mpsc::channel();
            self.running_output = Some(rx);
            crate::wsl::start_distro_async(&name, tx);
        }
    }

    pub fn wsl_stop_selected(&mut self) {
        if let Some(distro) = self.wsl_distros.get(self.wsl_selected) {
            let name = distro.name.clone();
            self.push(format!("> [WSL] Stopping {}...", name));
            let (tx, rx) = mpsc::channel();
            self.running_output = Some(rx);
            crate::wsl::stop_distro_async(&name, tx);
        }
    }

    pub fn wsl_open_terminal(&mut self) {
        if let Some(distro) = self.wsl_distros.get(self.wsl_selected) {
            crate::wsl::open_terminal(&distro.name);
            self.push(format!("> [WSL] Terminal opened for {}", distro.name));
        }
    }

    // ── WOL operations ──────────────────────────────────────────────────

    pub fn refresh_wol_ping(&mut self) {
        self.wol_ping_results.clear();
        for host in &mut self.wol_hosts {
            host.online = None;
            let result = std::sync::Arc::new(std::sync::Mutex::new(None));
            self.wol_ping_results.push(result.clone());
            crate::wol::ping_host_async(host.ip.clone(), result);
        }
    }

    pub fn wol_wake_selected(&mut self) {
        if let Some(host) = self.wol_hosts.get(self.wol_selected) {
            let name = host.name.clone();
            match crate::wol::send_wol(host) {
                Ok(()) => self.push(format!("> [WOL] Magic packet sent to {}", name)),
                Err(e) => self.push(format!("> [ERROR] WOL failed for {}: {}", name, e)),
            }
            sounds::launch();
        }
    }

    pub fn wol_delete_selected(&mut self) {
        if self.wol_hosts.is_empty() {
            return;
        }
        let name = self.wol_hosts[self.wol_selected].name.clone();
        self.wol_hosts.remove(self.wol_selected);
        self.wol_selected = self.wol_selected.min(self.wol_hosts.len().saturating_sub(1));
        self.save_wol_hosts();
        self.push(format!("> [WOL] Removed host: {}", name));
    }

    pub fn wol_add_host(&mut self, name: String, mac: String, ip: String) {
        self.wol_hosts.push(WolHost {
            name: name.clone(),
            mac,
            broadcast: "255.255.255.255".to_string(),
            port: 9,
            ip,
            online: None,
        });
        self.save_wol_hosts();
        self.push(format!("> [WOL] Added host: {}", name));
        self.refresh_wol_ping();
    }

    fn save_wol_hosts(&self) {
        #[derive(serde::Serialize)]
        struct HostOut {
            name: String,
            mac: String,
            broadcast: String,
            port: u16,
            ip: String,
        }
        let out: Vec<HostOut> = self
            .wol_hosts
            .iter()
            .map(|h| HostOut {
                name: h.name.clone(),
                mac: h.mac.clone(),
                broadcast: h.broadcast.clone(),
                port: h.port,
                ip: h.ip.clone(),
            })
            .collect();
        if let Ok(json) = serde_json::to_string_pretty(&out) {
            let _ = std::fs::write(crate::wol::config_file(), json);
        }
    }

    pub fn update_wol_status(&mut self) {
        for (i, result) in self.wol_ping_results.iter().enumerate() {
            if let Ok(guard) = result.lock() {
                if let Some(online) = *guard {
                    if i < self.wol_hosts.len() {
                        self.wol_hosts[i].online = Some(online);
                    }
                }
            }
        }
    }

    pub fn load_icons(&mut self, ctx: &egui::Context) {
        for entry in &self.entries {
            if let PluginEntry::Plugin(plugin) = entry {
                if !self.icon_textures.contains_key(&plugin.path) {
                    // For .lnk, resolve target path for icon extraction
                    let icon_path = if plugin.path.to_lowercase().ends_with(".lnk") {
                        crate::plugins::resolve_lnk_target(std::path::Path::new(&plugin.path))
                            .unwrap_or_else(|| plugin.path.clone())
                    } else {
                        plugin.path.clone()
                    };
                    if let Some(image) = crate::icons::extract_icon(&icon_path, 64) {
                        let texture = ctx.load_texture(
                            format!("icon_{}", plugin.path),
                            image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.icon_textures.insert(plugin.path.clone(), texture);
                    }
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // First frame: give ctx to tray for wake-up
        if !self.tray_ctx_set {
            self.tray.set_egui_ctx(ctx.clone());
            self.tray_ctx_set = true;
        }

        // Find our HWND by window title (retry every frame until found)
        #[cfg(windows)]
        if !self.hwnd_found {
            let title: Vec<u16> = "0xID".encode_utf16().chain(std::iter::once(0)).collect();
            let hwnd = unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(
                    std::ptr::null(),
                    title.as_ptr(),
                )
            };
            if hwnd != 0 {
                self.tray.set_app_hwnd(hwnd);
                self.hwnd_found = true;
            }
        }

        // Always request repaint for animations + tray polling
        ctx.request_repaint_after(Duration::from_millis(50));

        // Poll tray events
        while let Some(event) = self.tray.poll() {
            match event {
                TrayEvent::Show => {
                    // tray thread already called ShowWindow — just update flag
                    self.visible = true;
                }
                TrayEvent::Quit => {
                    // Force quit — ViewportCommand::Close may not work when hidden
                    std::process::exit(0);
                }
            }
        }

        // Handle hide-to-tray (only if HWND is known)
        if self.should_hide && self.hwnd_found {
            self.should_hide = false;
            self.visible = false;
            self.tray.hide();
        }

        // Load icons lazily (once)
        if !self.icons_loaded {
            self.load_icons(ctx);
            self.icons_loaded = true;
        }

        // Handle input
        self.handle_input(ctx);

        // Render UI
        if self.visible {
            ui::render(ctx, self);
        }

        // Tick (animations + drain output)
        self.tick();
    }
}
