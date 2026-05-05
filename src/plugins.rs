use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Returns `<home>/.0xid` (config root) and creates it if missing.
/// Windows: %USERPROFILE%\.0xid  ·  Linux/macOS: $HOME/.0xid
pub fn data_dir() -> PathBuf {
    let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var(home_var)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let dir = home.join(".0xid");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Returns `<data_dir>/apps` and creates it if missing.
pub fn apps_dir() -> PathBuf {
    let dir = data_dir().join("apps");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Folders excluded from plugin discovery (generated/runtime dirs).
fn is_skipped_dir(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "logs")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone)]
pub enum PluginEntry {
    Back,
    Folder { name: String, path: PathBuf },
    Plugin(Plugin),
}

/// Discover entries (folders + plugins) in the given directory.
/// If `is_subdir` is true, prepends a `Back` (..) entry.
pub fn discover_entries(dir: &Path, is_subdir: bool) -> Vec<PluginEntry> {
    let mut entries = Vec::new();

    if is_subdir {
        entries.push(PluginEntry::Back);
    }

    if !dir.exists() {
        let _ = std::fs::create_dir_all(dir);
        return entries;
    }

    // Collect subdirectories and executables
    let mut folders = Vec::new();
    let mut plugins: Vec<Plugin> = Vec::new();

    // Check for plugins.json (provides rich metadata for executables)
    let json_path = dir.join("plugins.json");
    let has_json = if let Ok(content) = std::fs::read_to_string(&json_path) {
        if let Ok(ps) = serde_json::from_str::<Vec<Plugin>>(&content) {
            if !ps.is_empty() {
                plugins = ps;
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut items: Vec<_> = rd.flatten().collect();
        items.sort_by_key(|e| e.file_name());

        for entry in items {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "plugins.json" {
                continue;
            }
            if path.is_dir() {
                if is_skipped_dir(&name) {
                    continue;
                }
                folders.push(PluginEntry::Folder { name, path });
            } else if !has_json && is_executable(&path) {
                // For .lnk files, resolve target and show target exe name
                let display_name = if name.to_lowercase().ends_with(".lnk") {
                    resolve_lnk_target(&path)
                        .and_then(|p| {
                            Path::new(&p).file_name().map(|f| f.to_string_lossy().to_string())
                        })
                        .unwrap_or_else(|| {
                            let stem = Path::new(&name)
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            format!("{}.exe", stem)
                        })
                } else {
                    name
                };
                plugins.push(Plugin {
                    name: display_name,
                    path: path.to_string_lossy().to_string(),
                    description: None,
                    args: None,
                    interactive: false,
                    icon: None,
                });
            }
        }
    }

    // Sort plugins alphabetically by name (case-insensitive)
    plugins.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Folders first (already sorted by readdir), then plugins
    entries.extend(folders);
    entries.extend(plugins.into_iter().map(PluginEntry::Plugin));

    entries
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if name.starts_with('.') || name == "plugins.json" {
        return false;
    }
    path.extension()
        .map(|e| {
            let e = e.to_string_lossy().to_lowercase();
            matches!(e.as_str(), "exe" | "bat" | "cmd" | "ps1" | "py" | "lnk")
        })
        .unwrap_or(false)
}

/// Recursively discover ALL plugins in dir and subdirs (flat list, for search).
pub fn discover_all_plugins(dir: &Path) -> Vec<Plugin> {
    let mut plugins = Vec::new();
    discover_all_recursive(dir, &mut plugins);
    plugins.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    plugins
}

fn discover_all_recursive(dir: &Path, out: &mut Vec<Plugin>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "plugins.json" {
            continue;
        }
        if path.is_dir() {
            if is_skipped_dir(&name) {
                continue;
            }
            discover_all_recursive(&path, out);
        } else if is_executable(&path) {
            let display_name = if name.to_lowercase().ends_with(".lnk") {
                resolve_lnk_target(&path)
                    .and_then(|p| Path::new(&p).file_name().map(|f| f.to_string_lossy().to_string()))
                    .unwrap_or_else(|| {
                        let stem = Path::new(&name).file_stem().unwrap_or_default().to_string_lossy().to_string();
                        format!("{}.exe", stem)
                    })
            } else {
                name
            };
            out.push(Plugin {
                name: display_name,
                path: path.to_string_lossy().to_string(),
                description: None,
                args: None,
                interactive: false,
                icon: None,
            });
        }
    }
}

/// Resolve a .lnk shortcut to get the full target path.
/// Uses a simple binary parser for the Windows Shell Link format.
pub fn resolve_lnk_target(lnk_path: &Path) -> Option<String> {
    let data = std::fs::read(lnk_path).ok()?;
    if data.len() < 0x4C + 4 {
        return None;
    }

    let flags = u32::from_le_bytes(data[0x14..0x18].try_into().ok()?);
    let has_link_target_id_list = flags & 0x01 != 0;
    let has_link_info = flags & 0x02 != 0;

    let mut offset = 0x4C;

    if has_link_target_id_list {
        if offset + 2 > data.len() {
            return None;
        }
        let list_size = u16::from_le_bytes(data[offset..offset + 2].try_into().ok()?) as usize;
        offset += 2 + list_size;
    }

    if has_link_info && offset + 4 <= data.len() {
        let link_info_size =
            u32::from_le_bytes(data[offset..offset + 4].try_into().ok()?) as usize;
        if offset + link_info_size <= data.len() && link_info_size >= 0x1C {
            let info = &data[offset..offset + link_info_size];
            let local_base_path_offset =
                u32::from_le_bytes(info[0x10..0x14].try_into().ok()?) as usize;
            if local_base_path_offset > 0 && local_base_path_offset < link_info_size {
                let path_bytes = &info[local_base_path_offset..];
                let end = path_bytes.iter().position(|&b| b == 0).unwrap_or(path_bytes.len());
                let target_path = String::from_utf8_lossy(&path_bytes[..end]).to_string();
                if !target_path.is_empty() {
                    return Some(target_path);
                }
            }
        }
    }

    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if !path.is_file() {
        return false;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if name.starts_with('.') || name == "plugins.json" {
        return false;
    }
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(any(windows, unix)))]
fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    !name.starts_with('.') && name != "plugins.json"
}
