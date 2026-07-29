use eframe::egui::{self, Color32, RichText, Sense};

use crate::app::App;
use crate::plugins::PluginEntry;


// ──────────────────────────────────────────────────────────────────────────────
// Palette  (phosphor green + neon accents on black)
// ──────────────────────────────────────────────────────────────────────────────
const GREEN: Color32 = Color32::from_rgb(0, 255, 70);
const GREEN_DIM: Color32 = Color32::from_rgb(0, 140, 40);
const CYAN: Color32 = Color32::from_rgb(0, 255, 220);
const YELLOW: Color32 = Color32::from_rgb(255, 230, 0);
const RED: Color32 = Color32::from_rgb(255, 60, 60);
const DARK: Color32 = Color32::from_rgb(20, 20, 20);
const SELECTED_BG: Color32 = Color32::from_rgb(0, 60, 20);

// ──────────────────────────────────────────────────────────────────────────────
// ASCII banner — "0xID"
// ──────────────────────────────────────────────────────────────────────────────
const BANNER: &[&str] = &[
    r"   ██████╗ ██╗  ██╗██╗██████╗ ",
    r"  ██╔═████╗╚██╗██╔╝██║██╔══██╗",
    r"  ██║██╔██║ ╚███╔╝ ██║██║  ██║",
    r"  ████╔╝██║ ██╔██╗ ██║██║  ██║",
    r"  ╚██████╔╝██╔╝ ██╗██║██████╔╝",
    r"   ╚═════╝ ╚═╝  ╚═╝╚═╝╚═════╝",
];

const TAGLINES: &[&str] = &[
    "HACK THE PLANET",
    "ZERO DAY EDITION",
    "ACCESS GRANTED",
    "BREACH SUCCESSFUL",
    "FIREWALL: MELTING",
    "ROOT ACQUIRED",
    "DAEMON: UNLEASHED",
    "SYSTEM COMPROMISED",
];

// ──────────────────────────────────────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────────────────────────────────────
pub fn render(ctx: &egui::Context, app: &mut App) {
    // Set dark visuals
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = DARK;
    visuals.window_fill = DARK;
    visuals.extreme_bg_color = DARK;
    visuals.faint_bg_color = DARK;
    ctx.set_visuals(visuals);

    // Banner (top)
    egui::TopBottomPanel::top("banner")
        .frame(egui::Frame::default().fill(DARK).inner_margin(egui::Margin::symmetric(8.0, 4.0)))
        .show(ctx, |ui| {
            render_banner(ui, app, ctx);
        });

    // Tab bar
    egui::TopBottomPanel::top("tabs")
        .frame(egui::Frame::default().fill(DARK).inner_margin(egui::Margin::symmetric(8.0, 2.0)))
        .show(ctx, |ui| {
            render_tab_bar(ui, app);
        });

    // Status bar (bottom)
    egui::TopBottomPanel::bottom("status")
        .frame(egui::Frame::default().fill(DARK).inner_margin(egui::Margin::symmetric(4.0, 2.0)))
        .show(ctx, |ui| {
            render_statusbar(ui, app, ctx);
        });

    if app.active_tab.is_fullwidth() {
        // Full-width tabs (Scanner, SSH)
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(DARK)
                    .stroke(egui::Stroke::new(1.0, GREEN))
                    .inner_margin(egui::Margin::same(6.0)),
            )
            .show(ctx, |ui| {
                match app.active_tab {
                    crate::app::Tab::Scanner => render_scanner(ui, app),
                    crate::app::Tab::Ssh => render_ssh(ui, app),
                    _ => {}
                }
            });
    } else {
        // Other tabs: left panel + output
        egui::SidePanel::left("plugins")
            .frame(
                egui::Frame::default()
                    .fill(DARK)
                    .stroke(egui::Stroke::new(1.0, GREEN))
                    .inner_margin(egui::Margin::same(4.0)),
            )
            .default_width(300.0)
            .min_width(200.0)
            .show(ctx, |ui| {
                match app.active_tab {
                    crate::app::Tab::Launcher => render_plugin_list(ui, app),
                    crate::app::Tab::Wsl => render_wsl(ui, app),
                    crate::app::Tab::Wol => render_wol(ui, app),
                    _ => {}
                }
            });

        let border_color = if app.settings_open {
            YELLOW
        } else if app.shell_mode {
            CYAN
        } else {
            GREEN
        };
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(DARK)
                    .stroke(egui::Stroke::new(1.0, border_color))
                    .inner_margin(egui::Margin::same(4.0)),
            )
            .show(ctx, |ui| {
                if app.settings_open && app.active_tab == crate::app::Tab::Launcher {
                    render_settings(ui, app);
                } else {
                    render_output(ui, app);
                }
            });
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Banner
// ──────────────────────────────────────────────────────────────────────────────
fn render_banner(ui: &mut egui::Ui, app: &App, ctx: &egui::Context) {
    let tick = app.tick_count;
    let glitch = tick % 60 < 2;

    // Make banner area draggable (move window)
    let response = ui.interact(ui.max_rect(), egui::Id::new("banner_drag"), Sense::drag());
    if response.drag_started() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    for (i, &row) in BANNER.iter().enumerate() {
        let color = if glitch {
            CYAN
        } else if i % 2 == 0 {
            GREEN
        } else {
            Color32::from_rgb(0, 210, 55)
        };
        ui.label(RichText::new(row).monospace().color(color).strong());
    }

    // Tagline
    let tagline = TAGLINES[(tick / 120) as usize % TAGLINES.len()];
    let cursor = if tick % 20 < 10 { "\u{2588}" } else { " " };

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("   \u{27e6} {} \u{27e7}  {}", tagline, cursor))
                .monospace()
                .color(YELLOW)
                .strong(),
        );
        ui.add_space(40.0);
        ui.label(RichText::new("v3.0").monospace().color(GREEN_DIM).weak());
    });
}

// ──────────────────────────────────────────────────────────────────────────────
// Plugin / folder list (left panel)
// ──────────────────────────────────────────────────────────────────────────────
fn render_plugin_list(ui: &mut egui::Ui, app: &mut App) {
    // Title
    let title = if app.dir_stack.is_empty() {
        "\u{27e6} LAUNCHER \u{27e7}".to_string()
    } else {
        let folder = app.current_dir.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        format!("\u{27e6} {} \u{27e7}", folder)
    };
    ui.label(RichText::new(title).monospace().color(CYAN).strong());

    // Search bar
    if app.search_mode {
        let cursor = if app.tick_count % 20 < 10 { "\u{2588}" } else { " " };
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("\u{1f50d} ").monospace().color(YELLOW));
            ui.label(
                RichText::new(format!("{}{}", app.search_query, cursor))
                    .monospace()
                    .color(GREEN)
                    .strong(),
            );
        });
        let count = app.search_results.len();
        ui.label(
            RichText::new(format!("  {} result(s)", count))
                .monospace()
                .color(GREEN_DIM)
                .weak(),
        );
        ui.add_space(2.0);
    } else {
        ui.add_space(4.0);
    }

    if app.entries.is_empty() {
        ui.label(RichText::new("  [ NO APPS ]").monospace().color(RED).strong());
        ui.add_space(8.0);
        ui.label(RichText::new("  Drop .exe/.bat/.py").monospace().color(GREEN_DIM));
        ui.label(RichText::new("  into ./plugins/").monospace().color(GREEN_DIM));
        ui.add_space(8.0);
        ui.label(RichText::new("  Press [R] to refresh").monospace().color(YELLOW));
        return;
    }

    // In search mode, show flat search results; otherwise show current folder entries
    if app.search_mode {
        render_search_results(ui, app);
        return;
    }

    // Scroll selected into view only when selection changes (avoids fighting
    // mouse-wheel scroll via scroll_to_rect every frame).
    let scroll_anchor = egui::Id::new("plugin_scroll_idx");
    let scroll_selected = ui.ctx().data_mut(|d| {
        let prev = d.get_persisted::<usize>(scroll_anchor).unwrap_or(usize::MAX);
        d.insert_persisted(scroll_anchor, app.selected);
        app.selected != prev
    });

    // Scrollable list
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let entries_snapshot: Vec<(usize, PluginEntry)> = app.entries
                .iter()
                .enumerate()
                .map(|(i, e)| (i, e.clone()))
                .collect();

            for (i, entry) in &entries_snapshot {
                let selected = *i == app.selected;
                let arrow = if selected { "\u{25b6}" } else { " " };

                let (label, entry_color, hint) = match entry {
                    PluginEntry::Back => (
                        format!(" {} \u{25c0} ..", arrow),
                        YELLOW,
                        "[\u{2190}/Bksp] back",
                    ),
                    PluginEntry::Folder { name, .. } => (
                        format!(" {} \u{25b8} {}", arrow, name),
                        CYAN,
                        "[ENTER/\u{2192}] open",
                    ),
                    PluginEntry::Plugin(plugin) => {
                        let itag = if plugin.interactive { " [I]" } else { "" };
                        let ext_icon = if plugin.path.to_lowercase().ends_with(".py") {
                            "\u{1f40d} "
                        } else {
                            ""
                        };
                        (
                            format!(" {} {}{}{}", arrow, ext_icon, plugin.name, itag),
                            GREEN,
                            "[ENTER] launch",
                        )
                    }
                };

                let bg = if selected { SELECTED_BG } else { Color32::TRANSPARENT };

                let frame = egui::Frame::default()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(2.0, 1.0));

                let inner_resp = frame.show(ui, |ui: &mut egui::Ui| {
                    ui.set_width(ui.available_width());

                    ui.horizontal(|ui| {
                        // Show extracted icon if available
                        if let PluginEntry::Plugin(plugin) = entry {
                            if let Some(texture) = app.icon_textures.get(&plugin.path) {
                                let img = egui::Image::new(texture)
                                    .fit_to_exact_size(egui::vec2(48.0, 48.0));
                                ui.add(img);
                            }
                        }

                        let text = RichText::new(&label).monospace().color(entry_color);
                        let text = if selected { text.strong() } else { text };
                        ui.label(text);
                    });

                    if selected {
                        // Description
                        if let PluginEntry::Plugin(plugin) = entry {
                            if let Some(ref d) = plugin.description {
                                ui.label(
                                    RichText::new(format!("   \u{21b3} {}", d))
                                        .monospace()
                                        .color(CYAN)
                                        .weak(),
                                );
                            }
                        }
                        // Hint
                        ui.label(
                            RichText::new(format!("   {}", hint))
                                .monospace()
                                .color(YELLOW)
                                .weak()
                                .italics(),
                        );
                    }
                });

                // Handle click on this entry
                let click_resp = ui.interact(
                    inner_resp.response.rect,
                    egui::Id::new(("entry_click", i)),
                    Sense::click(),
                );
                if click_resp.clicked() {
                    app.click_entry(*i);
                }

                // Auto-scroll to keep selected entry visible (only on selection change)
                if selected && scroll_selected {
                    ui.scroll_to_rect(inner_resp.response.rect, Some(egui::Align::Center));
                }
            }
        });

    // Bottom hint
    ui.add_space(4.0);
    ui.label(
        RichText::new(" [\u{2191}\u{2193}] [ENTER] [\u{2190}]back [R] ")
            .monospace()
            .color(GREEN_DIM),
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Search results (flat plugin list)
// ──────────────────────────────────────────────────────────────────────────────
fn render_search_results(ui: &mut egui::Ui, app: &mut App) {
    if app.search_results.is_empty() && !app.search_query.is_empty() {
        ui.label(RichText::new("  No matches found.").monospace().color(RED));
        return;
    }

    // Snapshot data to avoid borrow conflicts
    let items: Vec<(usize, String, String, bool)> = app
        .search_results
        .iter()
        .enumerate()
        .filter_map(|(list_idx, &plugin_idx)| {
            let p = app.search_all_plugins.get(plugin_idx)?;
            Some((list_idx, p.name.clone(), p.path.clone(), list_idx == app.search_selected))
        })
        .collect();

    // Only scroll to selected on *change* (not every frame), so mouse wheel
    // scrolling works without fighting the programmatic scroll-to.
    let search_scroll_anchor = egui::Id::new("search_scroll_idx");
    let search_scroll_selected = ui.ctx().data_mut(|d| {
        let prev = d.get_persisted::<usize>(search_scroll_anchor).unwrap_or(usize::MAX);
        d.insert_persisted(search_scroll_anchor, app.search_selected);
        app.search_selected != prev
    });

    let mut clicked: Option<usize> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (list_idx, name, path, selected) in &items {
                let arrow = if *selected { "\u{25b6}" } else { " " };
                let ext_icon = if path.to_lowercase().ends_with(".py") { "\u{1f40d} " } else { "" };
                let label = format!(" {} {}{}", arrow, ext_icon, name);
                let bg = if *selected { SELECTED_BG } else { Color32::TRANSPARENT };

                let frame = egui::Frame::default()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(2.0, 1.0));

                let inner_resp = frame.show(ui, |ui: &mut egui::Ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if let Some(texture) = app.icon_textures.get(path.as_str()) {
                            let img = egui::Image::new(texture)
                                .fit_to_exact_size(egui::vec2(24.0, 24.0));
                            ui.add(img);
                        }
                        let text = RichText::new(&label).monospace().color(GREEN);
                        ui.label(if *selected { text.strong() } else { text });
                    });
                    if *selected {
                        ui.label(RichText::new(format!("   {}", path)).monospace().color(GREEN_DIM).weak());
                    }
                });

                if inner_resp.response.interact(Sense::click()).clicked() {
                    clicked = Some(*list_idx);
                }

                // Auto-scroll to keep selected entry visible (only on selection change)
                if *selected && search_scroll_selected {
                    ui.scroll_to_rect(inner_resp.response.rect, Some(egui::Align::Center));
                }
            }
        });

    if let Some(idx) = clicked {
        if app.search_selected == idx {
            app.activate_search_result();
        } else {
            app.search_selected = idx;
            crate::sounds::nav();
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Output stream (right panel)
// ──────────────────────────────────────────────────────────────────────────────
fn render_output(ui: &mut egui::Ui, app: &mut App) {
    // Title
    let title = if app.shell_mode {
        "\u{27e6} SHELL \u{27e7}"
    } else {
        "\u{27e6} OUTPUT STREAM \u{27e7}"
    };
    ui.label(RichText::new(title).monospace().color(CYAN).strong());
    ui.add_space(2.0);

    // Reserve space at bottom for prompt
    let prompt_h = 36.0;
    let scroll_h = ui.available_height() - prompt_h;

    // ScrollArea with auto-scroll to bottom
    egui::ScrollArea::vertical()
        .id_salt("output_scroll")
        .max_height(scroll_h)
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui: &mut egui::Ui| {
            for line in &app.output {
                let (color, bold) = output_style(line);
                let text = RichText::new(line).monospace().color(color);
                ui.label(if bold { text.strong() } else { text });
            }
        });

    // Prompt / cursor (always visible at bottom)
    if app.shell_mode {
        let cursor_char = if app.tick_count % 20 < 10 {
            "\u{2588}"
        } else {
            " "
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new("> $ ").monospace().color(CYAN).strong());
            ui.label(
                RichText::new(format!("{}{}", app.shell_input, cursor_char))
                    .monospace()
                    .color(GREEN)
                    .strong(),
            );
        });
    } else {
        let cursor_char = if app.tick_count % 20 < 10 {
            "\u{258a}"
        } else {
            " "
        };
        ui.label(
            RichText::new(format!("> {}", cursor_char))
                .monospace()
                .color(GREEN)
                .strong(),
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Settings menu ([P])
// ──────────────────────────────────────────────────────────────────────────────
fn render_settings(ui: &mut egui::Ui, app: &mut App) {
    use crate::app::SettingRow;

    let s = crate::settings::get();
    let startup = app.startup_enabled();
    let muted = crate::sounds::is_muted();

    ui.label(RichText::new("\u{27e6} SETTINGS \u{27e7}").monospace().color(CYAN).strong());
    ui.add_space(6.0);

    for (idx, &row) in SettingRow::ALL.iter().enumerate() {
        let selected = idx == app.settings_selected;
        let editing = selected && app.settings_editing;
        let capturing = selected && app.settings_capturing;

        let value: String = if editing {
            let cursor = if app.tick_count % 20 < 10 { "\u{2588}" } else { " " };
            format!("{}{}", app.settings_input, cursor)
        } else if capturing {
            "<press a key combo\u{2026}>".to_string()
        } else {
            match row {
                SettingRow::CondaEnv => {
                    if s.conda_env.trim().is_empty() {
                        "(none \u{2014} plain python)".to_string()
                    } else {
                        s.conda_env.clone()
                    }
                }
                SettingRow::Hotkey => s.hotkey.clone(),
                SettingRow::AppsDir => {
                    if s.apps_dir.trim().is_empty() {
                        "(default)".to_string()
                    } else {
                        s.apps_dir.clone()
                    }
                }
                SettingRow::EnvVars => {
                    if s.env_vars.is_empty() {
                        "(none)".to_string()
                    } else {
                        s.env_vars.join(" ; ")
                    }
                }
                SettingRow::Mute => if muted { "ON".to_string() } else { "OFF".to_string() },
                SettingRow::Startup => if startup { "ON".to_string() } else { "OFF".to_string() },
            }
        };

        let marker = if selected { "\u{25b6} " } else { "  " };
        let label_col = if selected { GREEN } else { GREEN_DIM };
        let val_col = if editing || capturing { YELLOW } else { CYAN };

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{}{:<20}", marker, row.label()))
                    .monospace()
                    .color(label_col)
                    .strong(),
            );
            ui.label(RichText::new(value).monospace().color(val_col).strong());
        });
        ui.add_space(2.0);
    }

    ui.add_space(10.0);
    let hint = if app.settings_editing {
        "[ENTER] save   [ESC] cancel    (env vars: KEY=VALUE separated by  ;)"
    } else if app.settings_capturing {
        "Press the combo (e.g. Ctrl+Shift+Space).   [ESC] cancel"
    } else {
        "[\u{2191}/\u{2193}] move    [ENTER] edit / toggle    [ESC] close"
    };
    ui.label(RichText::new(hint).monospace().color(YELLOW));
    ui.add_space(2.0);
    ui.label(
        RichText::new("Saved automatically. Conda env empty = use plain python.")
            .monospace()
            .color(GREEN_DIM)
            .weak(),
    );
}

fn output_style(line: &str) -> (Color32, bool) {
    if line.starts_with("> [ERROR]") {
        (RED, true)
    } else if line.starts_with("> [FAIL]") {
        (Color32::from_rgb(255, 140, 0), true)
    } else if line.starts_with("> [OK]") || line.starts_with("> [REFRESH]") {
        (GREEN, true)
    } else if line.starts_with("> [INTERACTIVE]")
        || line.starts_with("> EXEC")
        || line.starts_with("> [LAUNCHED]")
        || line.starts_with("> [SHELL]")
        || line.starts_with("> [KILL]")
    {
        (CYAN, true)
    } else if line.starts_with("> WARNING") {
        (YELLOW, true)
    } else if line.starts_with("> $") {
        (CYAN, true)
    } else if line.starts_with("> ") {
        (CYAN, false)
    } else if line.starts_with("! ") {
        (RED, false)
    } else if line.starts_with('\u{2550}') {
        (GREEN_DIM, false)
    } else {
        (GREEN, false)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tab bar
// ──────────────────────────────────────────────────────────────────────────────
fn render_tab_bar(ui: &mut egui::Ui, app: &mut App) {
    use crate::app::Tab;

    ui.horizontal(|ui| {
        for &tab in Tab::ALL {
            let active = app.active_tab == tab;
            let (fg, bg) = if active {
                (DARK, GREEN)
            } else {
                (GREEN_DIM, Color32::TRANSPARENT)
            };

            let frame = egui::Frame::default()
                .fill(bg)
                .inner_margin(egui::Margin::symmetric(8.0, 2.0));

            let resp = frame.show(ui, |ui: &mut egui::Ui| {
                ui.label(RichText::new(tab.label()).monospace().color(fg).strong());
            });

            if resp.response.interact(Sense::click()).clicked() {
                app.active_tab = tab;
                crate::sounds::nav();
            }
        }

        ui.add_space(16.0);
        ui.label(RichText::new("[TAB] switch").monospace().color(GREEN_DIM).weak());
    });
}

// ──────────────────────────────────────────────────────────────────────────────
// SSH tab (full width)
// ──────────────────────────────────────────────────────────────────────────────
fn render_ssh(ui: &mut egui::Ui, app: &mut App) {
    match app.ssh_mode {
        crate::app::SshMode::HostList => render_ssh_host_list(ui, app),
        crate::app::SshMode::AddHost => render_ssh_add_host(ui, app),
        crate::app::SshMode::PinEntry => render_ssh_pin_entry(ui, app),
        crate::app::SshMode::Terminal => render_ssh_terminal(ui, app),
        crate::app::SshMode::Sftp => render_ssh_sftp(ui, app),
    }
}

fn render_ssh_terminal(ui: &mut egui::Ui, app: &mut App) {
    let host_name = app.ssh_hosts.get(app.ssh_selected)
        .map(|h| h.display())
        .unwrap_or_default();

    ui.horizontal(|ui| {
        ui.label(RichText::new("\u{27e6} SSH TERMINAL \u{27e7}").monospace().color(CYAN).strong());
        ui.add_space(8.0);
        ui.label(RichText::new(&host_name).monospace().color(GREEN));
        ui.add_space(16.0);
        ui.label(RichText::new("[ESC] disconnect").monospace().color(YELLOW).weak());
    });
    ui.add_space(2.0);

    // Output with scroll
    let prompt_h = 36.0;
    let scroll_h = ui.available_height() - prompt_h;

    egui::ScrollArea::vertical()
        .id_salt("ssh_terminal_scroll")
        .max_height(scroll_h)
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui: &mut egui::Ui| {
            for line in &app.ssh_terminal_output {
                let (color, bold) = if line.starts_with("> [ERROR]") || line.starts_with("! ") {
                    (RED, line.starts_with("> ["))
                } else if line.starts_with("> [SSH]") || line.starts_with("> $") {
                    (CYAN, true)
                } else if line.starts_with("> ") {
                    (CYAN, false)
                } else {
                    (GREEN, false)
                };
                let text = RichText::new(line).monospace().color(color);
                ui.label(if bold { text.strong() } else { text });
            }
        });

    // Input prompt
    let cursor = if app.tick_count % 20 < 10 { "\u{2588}" } else { " " };
    ui.horizontal(|ui| {
        ui.label(RichText::new("$ ").monospace().color(CYAN).strong());
        ui.label(
            RichText::new(format!("{}{}", app.ssh_terminal_input, cursor))
                .monospace()
                .color(GREEN)
                .strong(),
        );
    });
}

fn render_ssh_host_list(ui: &mut egui::Ui, app: &mut App) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("\u{27e6} SSH HOSTS \u{27e7}").monospace().color(CYAN).strong());
        ui.add_space(16.0);
        let key_count = app.ssh_keys.len();
        if key_count > 0 {
            ui.label(RichText::new(format!("\u{1f511} {} key(s) detected", key_count)).monospace().color(GREEN_DIM));
        }
        if app.ssh_pin_unlocked {
            ui.label(RichText::new("\u{1f513} PIN unlocked").monospace().color(GREEN));
        } else {
            ui.label(RichText::new("\u{1f512} PIN locked").monospace().color(YELLOW));
        }
    });
    ui.add_space(4.0);

    if app.ssh_hosts.is_empty() {
        ui.label(RichText::new("  No SSH hosts configured.").monospace().color(RED));
        ui.label(RichText::new("  Press [A] to add a host.").monospace().color(YELLOW));
        ui.add_space(8.0);
        // Show detected keys
        if !app.ssh_keys.is_empty() {
            ui.label(RichText::new("  Detected SSH keys:").monospace().color(CYAN));
            for key in &app.ssh_keys {
                ui.label(RichText::new(format!("    \u{1f511} {}", key.display())).monospace().color(GREEN_DIM));
            }
        }
        return;
    }

    // Table header
    let hdr = format!(" {:<3} {:<15} {:<25} {:<8} {:<10} {}", "#", "NAME", "HOST", "PORT", "AUTH", "USER");
    ui.label(RichText::new(&hdr).monospace().color(CYAN).strong());
    ui.label(RichText::new(" \u{2500}".to_string() + &"\u{2500}".repeat(80)).monospace().color(GREEN_DIM));

    // Snapshot host data
    let hosts: Vec<(usize, String)> = app.ssh_hosts.iter().enumerate().map(|(i, host)| {
        let selected = i == app.ssh_selected;
        let arrow = if selected { "\u{25b6}" } else { " " };
        let auth_icon = match host.auth {
            crate::ssh::AuthMethod::Key => "\u{1f511} Key",
            crate::ssh::AuthMethod::Password => "\u{1f512} Pass",
        };
        (i, format!(
            " {:<3} {} {:<15} {:<25} {:<8} {:<10} {}",
            i + 1, arrow, host.name, host.hostname, host.port, auth_icon, host.username
        ))
    }).collect();

    let mut clicked: Option<usize> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, row) in &hosts {
                let selected = *i == app.ssh_selected;
                let bg = if selected { SELECTED_BG } else { Color32::TRANSPARENT };
                let frame = egui::Frame::default().fill(bg).inner_margin(egui::Margin::symmetric(0.0, 1.0));

                let resp = frame.show(ui, |ui: &mut egui::Ui| {
                    ui.set_width(ui.available_width());
                    let color = if selected { GREEN } else { Color32::from_rgb(0, 200, 55) };
                    ui.label(RichText::new(row).monospace().color(color));
                    if selected {
                        ui.label(
                            RichText::new("   [ENTER/T] terminal  [E] external  [F] SFTP  [D] delete")
                                .monospace().color(YELLOW).weak().italics(),
                        );
                    }
                });

                if resp.response.interact(Sense::click()).clicked() {
                    clicked = Some(*i);
                }
            }
        });

    if let Some(idx) = clicked {
        if app.ssh_selected == idx {
            app.ssh_open_terminal_selected();
        } else {
            app.ssh_selected = idx;
            crate::sounds::nav();
        }
    }
}

fn render_ssh_add_host(ui: &mut egui::Ui, app: &mut App) {
    ui.label(RichText::new("\u{27e6} ADD SSH HOST \u{27e7}").monospace().color(CYAN).strong());
    ui.add_space(4.0);

    let steps = ["Name", "Hostname/IP", "Port (22)", "Username", "Auth (K=key/P=pass)", "Key path", "Password", "PIN (4 digits)"];
    let step = app.ssh_add_step as usize;

    // Show completed fields
    for (i, val) in app.ssh_add_buf.iter().enumerate() {
        let display = if i == 6 { "****" } else { val }; // hide password
        ui.label(RichText::new(format!("  {} {}: {}", "\u{2713}", steps[i], display)).monospace().color(GREEN));
    }

    // Current prompt
    if step < steps.len() {
        let cursor = if app.tick_count % 20 < 10 { "\u{2588}" } else { " " };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("  {} ", steps[step])).monospace().color(YELLOW).strong());
            let display = if step == 6 || step == 7 {
                "*".repeat(app.ssh_input.len())
            } else {
                app.ssh_input.clone()
            };
            ui.label(RichText::new(format!("{}{}", display, cursor)).monospace().color(GREEN).strong());
        });

        // Show available keys for key path step
        if step == 5 && !app.ssh_keys.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new("  Available keys (or type path):").monospace().color(GREEN_DIM));
            for key in &app.ssh_keys {
                ui.label(RichText::new(format!("    {}", key.display())).monospace().color(CYAN));
            }
        }
    }

    ui.add_space(8.0);
    ui.label(RichText::new("  [ENTER] next  [ESC] cancel").monospace().color(GREEN_DIM));
}

fn render_ssh_pin_entry(ui: &mut egui::Ui, app: &mut App) {
    ui.label(RichText::new("\u{27e6} ENTER PIN \u{27e7}").monospace().color(CYAN).strong());
    ui.add_space(8.0);
    ui.label(RichText::new("  Enter your 4-digit PIN to unlock passwords:").monospace().color(YELLOW));
    ui.add_space(4.0);

    let dots = "\u{25cf} ".repeat(app.ssh_input.len().min(4));
    let remaining = "_ ".repeat(4_usize.saturating_sub(app.ssh_input.len()));
    ui.label(RichText::new(format!("       {}{}", dots, remaining)).monospace().color(GREEN).strong().size(20.0));
    ui.add_space(8.0);
    ui.label(RichText::new("  [ENTER] confirm  [ESC] cancel").monospace().color(GREEN_DIM));
}

fn render_ssh_sftp(ui: &mut egui::Ui, app: &mut App) {
    let host_name = app.ssh_hosts.get(app.ssh_selected)
        .map(|h| h.display())
        .unwrap_or_default();

    ui.horizontal(|ui| {
        ui.label(RichText::new("\u{27e6} SFTP \u{27e7}").monospace().color(CYAN).strong());
        ui.add_space(8.0);
        ui.label(RichText::new(&host_name).monospace().color(GREEN));
    });
    ui.add_space(2.0);

    let available_width = ui.available_width();
    let panel_width = (available_width - 16.0) / 2.0;

    ui.horizontal(|ui| {
        // ── Left panel: LOCAL ──
        let local_active = matches!(app.ssh_sftp_focus, crate::app::SftpFocus::Local);
        let local_border = if local_active { GREEN } else { GREEN_DIM };

        let frame = egui::Frame::default()
            .stroke(egui::Stroke::new(1.0, local_border))
            .inner_margin(egui::Margin::same(4.0));

        frame.show(ui, |ui: &mut egui::Ui| {
            ui.set_width(panel_width);
            ui.set_min_height(ui.available_height());

            ui.label(RichText::new("LOCAL").monospace().color(if local_active { GREEN } else { GREEN_DIM }).strong());
            ui.label(RichText::new(app.ssh_sftp_local_path.to_string_lossy().as_ref()).monospace().color(CYAN).weak());

            egui::ScrollArea::vertical().id_salt("sftp_local").show(ui, |ui| {
                for (i, (name, is_dir)) in app.ssh_sftp_local_files.iter().enumerate() {
                    let selected = local_active && i == app.ssh_sftp_local_selected;
                    let icon = if *is_dir { "\u{1f4c1}" } else { "\u{1f4c4}" };
                    let color = if *is_dir { CYAN } else { GREEN };
                    let bg = if selected { SELECTED_BG } else { Color32::TRANSPARENT };

                    let f = egui::Frame::default().fill(bg);
                    f.show(ui, |ui: &mut egui::Ui| {
                        ui.label(RichText::new(format!("{} {}", icon, name)).monospace().color(color));
                    });
                }
            });
        });

        ui.add_space(8.0);

        // ── Right panel: REMOTE ──
        let remote_active = matches!(app.ssh_sftp_focus, crate::app::SftpFocus::Remote);
        let remote_border = if remote_active { GREEN } else { GREEN_DIM };

        let frame = egui::Frame::default()
            .stroke(egui::Stroke::new(1.0, remote_border))
            .inner_margin(egui::Margin::same(4.0));

        frame.show(ui, |ui: &mut egui::Ui| {
            ui.set_width(panel_width);
            ui.set_min_height(ui.available_height());

            ui.label(RichText::new("REMOTE").monospace().color(if remote_active { GREEN } else { GREEN_DIM }).strong());
            ui.label(RichText::new(&app.ssh_sftp_remote_path).monospace().color(CYAN).weak());

            if app.ssh_sftp_remote_pending.is_some() {
                ui.label(RichText::new("  Loading...").monospace().color(YELLOW));
            } else if let Some(ref files) = app.ssh_sftp_remote_files {
                egui::ScrollArea::vertical().id_salt("sftp_remote").show(ui, |ui| {
                    for (i, file) in files.iter().enumerate() {
                        let selected = remote_active && i == app.ssh_sftp_remote_selected;
                        let icon = if file.is_dir { "\u{1f4c1}" } else { "\u{1f4c4}" };
                        let color = if file.is_dir { CYAN } else { GREEN };
                        let bg = if selected { SELECTED_BG } else { Color32::TRANSPARENT };

                        let f = egui::Frame::default().fill(bg);
                        f.show(ui, |ui: &mut egui::Ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{} {}", icon, file.name)).monospace().color(color));
                                if !file.is_dir {
                                    ui.label(RichText::new(&file.size).monospace().color(GREEN_DIM).weak());
                                }
                            });
                        });
                    }
                });
            } else {
                ui.label(RichText::new("  Not connected").monospace().color(RED));
            }
        });
    });
}

// ──────────────────────────────────────────────────────────────────────────────
// Scanner tab (full width, table layout)
// ──────────────────────────────────────────────────────────────────────────────
fn render_scanner(ui: &mut egui::Ui, app: &mut App) {
    // Header
    ui.horizontal(|ui| {
        ui.label(RichText::new("\u{27e6} NETWORK SCANNER \u{27e7}").monospace().color(CYAN).strong());
        ui.add_space(16.0);
        ui.label(
            RichText::new(format!(
                "Subnet: {}.{}-{} | {} ports | {} threads",
                app.scan_config.subnet,
                app.scan_config.start,
                app.scan_config.end,
                app.scan_config.ports.len(),
                app.scan_config.threads,
            ))
            .monospace()
            .color(GREEN_DIM),
        );
    });

    // Progress bar
    if let Ok(progress) = app.scan_progress.lock() {
        if progress.total > 0 {
            let pct = (progress.scanned as f32 / progress.total as f32 * 100.0) as u32;
            let bar_width = 50;
            let filled = (pct as usize * bar_width / 100).min(bar_width);
            let bar: String = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(bar_width - filled);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("[{}] {}%", bar, pct))
                        .monospace()
                        .color(if progress.done { GREEN } else { YELLOW }),
                );
                ui.label(
                    RichText::new(&progress.phase)
                        .monospace()
                        .color(CYAN)
                        .strong(),
                );
                ui.label(
                    RichText::new(format!("  {} host(s)", progress.hosts.len()))
                        .monospace()
                        .color(GREEN),
                );
            });
        } else {
            ui.label(RichText::new("  Press [S] to start scan").monospace().color(YELLOW));
        }

        let hosts = progress.hosts.clone();
        drop(progress);

        if hosts.is_empty() {
            return;
        }

        ui.add_space(4.0);

        // Table header
        let hdr = format!(
            " {:<3} {:<17} {:<18} {:<18} {:<10} {:<6} {}",
            "#", "MAC", "IP ADDRESS", "HOSTNAME", "VENDOR", "PORTS", "OPEN SERVICES"
        );
        ui.label(RichText::new(&hdr).monospace().color(CYAN).strong());
        ui.label(
            RichText::new(
                " \u{2500}".to_string() + &"\u{2500}".repeat(90),
            )
            .monospace()
            .color(GREEN_DIM),
        );

        // Table rows
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, host) in hosts.iter().enumerate() {
                    let selected = i == app.scan_selected;
                    let bg = if selected { SELECTED_BG } else { Color32::TRANSPARENT };

                    let status_dot = "\u{25cf}"; // ●
                    let vendor = if host.vendor.is_empty() { "-" } else { &host.vendor };
                    let hostname = if host.hostname.is_empty() { "-" } else { &host.hostname };
                    let mac_display = if host.mac.is_empty() { "-" } else { &*host.mac };

                    // Build port tokens — web ports are clickable links
                    let port_tokens: Vec<(String, bool)> = host
                        .open_ports
                        .iter()
                        .map(|&p| {
                            let svc = crate::scanner::port_service_name(p);
                            let label = if svc.is_empty() {
                                format!("{}", p)
                            } else {
                                format!("{}:{}", p, svc)
                            };
                            let is_web = matches!(p, 80 | 443 | 8080 | 8443 | 3000 | 5000 | 8000 | 8888 | 9443 | 8081 | 8008 | 3001);
                            (label, is_web)
                        })
                        .collect();

                    let frame = egui::Frame::default()
                        .fill(bg)
                        .inner_margin(egui::Margin::symmetric(0.0, 1.0));

                    let resp = frame.show(ui, |ui: &mut egui::Ui| {
                        ui.horizontal(|ui| {
                            let color = if selected { GREEN } else { Color32::from_rgb(0, 200, 55) };

                            // Fixed columns: MAC, #, dot, IP, hostname, vendor, port count
                            ui.label(
                                RichText::new(format!(
                                    " {:<17} {:<3} {} {:<16} {:<18} {:<10} {:<6}",
                                    mac_display, i + 1, status_dot, host.ip, hostname, vendor, host.open_ports.len(),
                                ))
                                .monospace()
                                .color(color),
                            );

                            // Port tokens — web ports are clickable links
                            for (pi, (port_label, is_web)) in port_tokens.iter().enumerate() {
                                if pi > 0 {
                                    ui.label(RichText::new("  ").monospace().color(color));
                                }
                                if *is_web {
                                    let scheme = if host.open_ports[pi] == 443 || host.open_ports[pi] == 8443 { "https" } else { "http" };
                                    let url = format!("{}://{}:{}", scheme, host.ip, host.open_ports[pi]);
                                    let url_clone = url.clone();
                                    // Use egui's built-in hyperlink which properly handles
                                    // open-in-browser on every platform.
                                    if ui.link(port_label).clicked() {
                                        let u = url_clone;
                                        std::thread::spawn(move || {
                                            let _ = std::process::Command::new("explorer")
                                                .arg(&u)
                                                .spawn();
                                        });
                                    }
                                } else {
                                    ui.label(RichText::new(port_label).monospace().color(color));
                                }
                            }
                        });
                    });

                    // Row click for selection (handled separately so port-link clicks
                    // above are not swallowed by the row's click sense).
                    if resp.response.clicked() {
                        app.scan_selected = i;
                        crate::sounds::nav();
                    }

                }
            });
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// WSL2 tab
// ──────────────────────────────────────────────────────────────────────────────
fn render_wsl(ui: &mut egui::Ui, app: &mut App) {
    ui.label(RichText::new("\u{27e6} WSL2 DISTROS \u{27e7}").monospace().color(CYAN).strong());
    ui.add_space(4.0);

    if app.wsl_distros.is_empty() {
        ui.label(RichText::new("  No WSL2 distributions found.").monospace().color(RED));
        ui.label(RichText::new("  Press [R] to refresh.").monospace().color(YELLOW));
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, distro) in app.wsl_distros.iter().enumerate() {
                let selected = i == app.wsl_selected;
                let arrow = if selected { "\u{25b6}" } else { " " };

                let (status_icon, status_color) = match distro.state {
                    crate::wsl::WslState::Running => ("\u{25cf}", GREEN),   // ●
                    crate::wsl::WslState::Stopped => ("\u{25cb}", RED),     // ○
                    crate::wsl::WslState::Unknown => ("?", YELLOW),
                };

                let default_tag = if distro.is_default { " *" } else { "" };
                let label = format!(
                    " {} {} {} {} v{}{}",
                    arrow, status_icon, distro.name, distro.state.label(), distro.version, default_tag
                );

                let bg = if selected { SELECTED_BG } else { Color32::TRANSPARENT };
                let frame = egui::Frame::default()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(2.0, 2.0));

                let resp = frame.show(ui, |ui: &mut egui::Ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&label).monospace().color(status_color).strong());

                    if selected {
                        let hint = match distro.state {
                            crate::wsl::WslState::Running => "[X]stop [T]erminal [ENTER]terminal",
                            crate::wsl::WslState::Stopped => "[S]tart [ENTER]terminal",
                            _ => "[R]efresh",
                        };
                        ui.label(RichText::new(format!("   {}", hint)).monospace().color(YELLOW).weak().italics());
                    }
                });

                if resp.response.interact(Sense::click()).clicked() {
                    app.wsl_selected = i;
                    crate::sounds::nav();
                }
            }
        });

    ui.add_space(4.0);
    ui.label(RichText::new(" [S]tart [X]stop [T]erminal [R]efresh").monospace().color(GREEN_DIM));
}

// ──────────────────────────────────────────────────────────────────────────────
// WOL tab
// ──────────────────────────────────────────────────────────────────────────────
fn render_wol(ui: &mut egui::Ui, app: &mut App) {
    ui.label(RichText::new("\u{27e6} WAKE-ON-LAN \u{27e7}").monospace().color(CYAN).strong());
    ui.add_space(4.0);

    // Add mode input
    if app.wol_adding {
        let prompt = match app.wol_add_step {
            0 => "Name:",
            1 => "MAC:",
            2 => "IP:",
            _ => "",
        };
        let cursor = if app.tick_count % 20 < 10 { "\u{2588}" } else { " " };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("  {} ", prompt)).monospace().color(YELLOW).strong());
            ui.label(RichText::new(format!("{}{}", app.wol_add_input, cursor)).monospace().color(GREEN).strong());
        });
        ui.label(RichText::new("  [ENTER] next  [ESC] cancel").monospace().color(GREEN_DIM));
        ui.add_space(4.0);
    }

    if app.wol_hosts.is_empty() && !app.wol_adding {
        ui.label(RichText::new("  No hosts configured.").monospace().color(RED));
        ui.label(RichText::new("  Press [A] to add a host.").monospace().color(YELLOW));
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, host) in app.wol_hosts.iter().enumerate() {
                let selected = i == app.wol_selected;
                let arrow = if selected { "\u{25b6}" } else { " " };

                let (status_icon, status_color) = match host.online {
                    Some(true) => ("\u{25cf}", GREEN),    // ● ONLINE
                    Some(false) => ("\u{25cf}", RED),      // ● OFFLINE
                    None => ("\u{25cb}", YELLOW),           // ○ checking...
                };

                let status_text = match host.online {
                    Some(true) => "ONLINE",
                    Some(false) => "OFFLINE",
                    None => "...",
                };

                let label = format!(
                    " {} {} {} {} ({})",
                    arrow, status_icon, host.name, status_text, host.ip
                );

                let bg = if selected { SELECTED_BG } else { Color32::TRANSPARENT };
                let frame = egui::Frame::default()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(2.0, 2.0));

                let resp = frame.show(ui, |ui: &mut egui::Ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&label).monospace().color(status_color).strong());

                    if selected {
                        ui.label(
                            RichText::new(format!("   MAC: {}  Port: {}", host.mac, host.port))
                                .monospace()
                                .color(GREEN_DIM)
                                .weak(),
                        );
                        ui.label(
                            RichText::new("   [W]ake [ENTER]wake [D]elete [A]dd")
                                .monospace()
                                .color(YELLOW)
                                .weak()
                                .italics(),
                        );
                    }
                });

                if resp.response.interact(Sense::click()).clicked() {
                    app.wol_selected = i;
                    crate::sounds::nav();
                }
            }
        });

    ui.add_space(4.0);
    ui.label(RichText::new(" [W]ake [A]dd [D]elete [R]efresh").monospace().color(GREEN_DIM));
}

// ──────────────────────────────────────────────────────────────────────────────
// Status bar (bottom row)
// ──────────────────────────────────────────────────────────────────────────────
fn render_statusbar(ui: &mut egui::Ui, app: &App, ctx: &egui::Context) {
    // Make status bar draggable too
    let response = ui.interact(ui.max_rect(), egui::Id::new("status_drag"), Sense::drag());
    if response.drag_started() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    ui.horizontal(|ui| {
        ui.label(RichText::new("\u{258c}").monospace().color(GREEN));
        ui.label(RichText::new(&app.status).monospace().color(GREEN).strong());
        ui.label(RichText::new(" \u{2590} ").monospace().color(GREEN));

        // Animated binary stream
        let bits: String = (0u64..16)
            .map(|i| {
                if (app.tick_count.wrapping_add(i * 7)) % 17 < 2 {
                    '1'
                } else {
                    '0'
                }
            })
            .collect();
        ui.label(RichText::new(bits).monospace().color(GREEN_DIM).weak());

        ui.add_space(8.0);

        // Keybinding hints (tab-specific)
        if app.shell_mode {
            ui.label(RichText::new("[ESC]exit").monospace().color(YELLOW));
            ui.label(RichText::new("[ENTER]run").monospace().color(GREEN));
            ui.label(RichText::new("[^Q]quit").monospace().color(RED));
        } else if app.search_mode {
            ui.label(RichText::new("[ESC]cancel").monospace().color(YELLOW));
            ui.label(RichText::new("[ENTER]launch").monospace().color(GREEN));
            ui.label(RichText::new("[^Q]quit").monospace().color(RED));
        } else {
            match app.active_tab {
                crate::app::Tab::Launcher => {
                    ui.label(RichText::new("[S]earch").monospace().color(CYAN));
                    ui.label(RichText::new("[/]shell").monospace().color(CYAN));
                    ui.label(RichText::new("[K]ill").monospace().color(RED));
                    ui.label(RichText::new("[R]efresh").monospace().color(YELLOW));
                }
                crate::app::Tab::Ssh => {
                    match app.ssh_mode {
                        crate::app::SshMode::HostList => {
                            ui.label(RichText::new("[A]dd").monospace().color(CYAN));
                            ui.label(RichText::new("[T]erminal").monospace().color(GREEN));
                            ui.label(RichText::new("[E]xternal").monospace().color(GREEN));
                            ui.label(RichText::new("[F]TP").monospace().color(GREEN));
                            ui.label(RichText::new("[D]elete").monospace().color(RED));
                        }
                        crate::app::SshMode::Sftp => {
                            ui.label(RichText::new("[U]pload").monospace().color(GREEN));
                            ui.label(RichText::new("[D]ownload").monospace().color(CYAN));
                            ui.label(RichText::new("[TAB]switch").monospace().color(YELLOW));
                            ui.label(RichText::new("[ESC]back").monospace().color(YELLOW));
                        }
                        crate::app::SshMode::Terminal => {
                            ui.label(RichText::new("[ENTER]run").monospace().color(GREEN));
                            ui.label(RichText::new("[ESC]disconnect").monospace().color(RED));
                        }
                        _ => {
                            ui.label(RichText::new("[ESC]back").monospace().color(YELLOW));
                        }
                    }
                }
                crate::app::Tab::Scanner => {
                    ui.label(RichText::new("[S]can").monospace().color(GREEN));
                    ui.label(RichText::new("[C]lear").monospace().color(YELLOW));
                }
                crate::app::Tab::Wsl => {
                    ui.label(RichText::new("[S]tart").monospace().color(GREEN));
                    ui.label(RichText::new("[X]stop").monospace().color(RED));
                    ui.label(RichText::new("[T]erminal").monospace().color(CYAN));
                    ui.label(RichText::new("[R]efresh").monospace().color(YELLOW));
                }
                crate::app::Tab::Wol => {
                    ui.label(RichText::new("[W]ake").monospace().color(GREEN));
                    ui.label(RichText::new("[A]dd").monospace().color(CYAN));
                    ui.label(RichText::new("[D]elete").monospace().color(RED));
                    ui.label(RichText::new("[R]efresh").monospace().color(YELLOW));
                }
            }
            ui.label(RichText::new("[TAB]switch").monospace().color(YELLOW));
            ui.label(RichText::new("[Q]tray").monospace().color(YELLOW));
            ui.label(RichText::new("[^Q]quit").monospace().color(RED));
        }
    });
}
