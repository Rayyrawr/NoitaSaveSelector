//! Finds Steam, Noita, the Entangled Worlds proxy and Noita Together on this machine.
//!
//! Order per program: registry / Steam libraries / uninstall entries first (fast, exact),
//! then well-known folders, and for the EW proxy (a portable zip) a bounded folder search.

use crate::saves::Settings;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};

const PROXY_EXE: &str = "noita_proxy.exe";
const NT_EXE: &str = "Noita Together.exe";

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn reg_str(hive: winreg::HKEY, key: &str, value: &str) -> Option<String> {
    RegKey::predef(hive).open_subkey_with_flags(key, KEY_READ).ok()?.get_value::<String, _>(value).ok()
}

fn clean(p: &str) -> PathBuf {
    PathBuf::from(p.trim().trim_matches('"').replace('/', "\\"))
}

pub fn steam_dir() -> Option<PathBuf> {
    let reg = [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ];
    reg.iter()
        .filter_map(|(h, k, v)| reg_str(*h, k, v))
        .map(|s| clean(&s))
        .chain([PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"C:\Program Files\Steam")])
        .find(|p| p.join("steam.exe").exists())
}

/// All Steam library folders (from libraryfolders.vdf), the Steam folder itself first.
pub fn steam_libraries(steam: &Path) -> Vec<PathBuf> {
    let mut libs = vec![steam.to_path_buf()];
    let vdf = std::fs::read_to_string(steam.join(r"steamapps\libraryfolders.vdf")).unwrap_or_default();
    for line in vdf.lines() {
        let parts: Vec<&str> = line.split('"').collect();
        // \t"path"\t\t"D:\\SteamLibrary"  ->  ["\t", "path", "\t\t", "D:\\\\SteamLibrary", ""]
        if parts.len() >= 4 && parts[1].eq_ignore_ascii_case("path") {
            let p = clean(&parts[3].replace(r"\\", r"\"));
            if !libs.contains(&p) {
                libs.push(p);
            }
        }
    }
    libs
}

/// Install folders from the Windows "Apps & features" list whose display name matches.
fn uninstall_locations(matches: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let key = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    for (hive, flags) in [
        (HKEY_CURRENT_USER, KEY_READ),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY),
    ] {
        let Ok(root) = RegKey::predef(hive).open_subkey_with_flags(key, flags) else { continue };
        for name in root.enum_keys().flatten() {
            let Ok(app) = root.open_subkey_with_flags(&name, flags) else { continue };
            let display: String = app.get_value("DisplayName").unwrap_or_default();
            if !matches(&display) {
                continue;
            }
            if let Ok(loc) = app.get_value::<String, _>("InstallLocation")
                && !loc.trim().is_empty()
            {
                out.push(clean(&loc));
            }
            if let Ok(icon) = app.get_value::<String, _>("DisplayIcon") {
                let icon = icon.split(',').next().unwrap_or_default();
                if let Some(dir) = clean(icon).parent() {
                    out.push(dir.to_path_buf());
                }
            }
        }
    }
    out
}

pub fn noita_exe(steam: Option<&Path>) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = steam.map(steam_libraries).unwrap_or_default().iter().map(|l| l.join(r"steamapps\common\Noita")).collect();
    dirs.extend(uninstall_locations(|n| n.eq_ignore_ascii_case("Noita")));
    dirs.extend([r"C:\GOG Games\Noita", r"C:\Program Files (x86)\GOG Galaxy\Games\Noita", r"C:\Games\Noita"].map(PathBuf::from));
    dirs.into_iter().map(|d| d.join("noita.exe")).find(|p| p.exists())
}

pub fn nt_exe() -> Option<PathBuf> {
    let mut dirs = uninstall_locations(|n| n.to_lowercase().starts_with("noita together"));
    if let Some(l) = env_path("LOCALAPPDATA") {
        dirs.push(l.join(r"Programs\noita-together"));
        dirs.push(l.join(r"Programs\Noita Together"));
    }
    dirs.extend([r"C:\Program Files\Noita Together", r"C:\Program Files (x86)\Noita Together"].map(PathBuf::from));
    dirs.into_iter().map(|d| d.join(NT_EXE)).find(|p| p.exists())
}

/// The EW proxy is a portable zip that can live anywhere: a running proxy wins,
/// then likely folders, then a bounded search over all drives.
pub fn ew_proxy_exe(noita: Option<&Path>) -> Option<PathBuf> {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    if let Some(p) = sys.processes().values().filter(|p| p.name().eq_ignore_ascii_case(PROXY_EXE)).find_map(|p| p.exe().map(Path::to_path_buf)) {
        return Some(p);
    }

    let home = env_path("USERPROFILE").unwrap_or_default();
    let mut roots: Vec<(PathBuf, usize)> = ["Downloads", "Desktop", "Documents", "Games", "OneDrive"].iter().map(|d| (home.join(d), 4)).collect();
    if let Some(n) = noita.and_then(Path::parent) {
        roots.push((n.to_path_buf(), 3));
        if let Some(lib) = n.parent() {
            roots.push((lib.to_path_buf(), 3));
        }
    }
    roots.push((home.clone(), 3));
    for letter in b'C'..=b'Z' {
        let drive = PathBuf::from(format!("{}:\\", letter as char));
        if drive.exists() {
            roots.push((drive, 4));
        }
    }
    let mut budget = 60_000usize;
    let mut found: Vec<PathBuf> = Vec::new();
    for (root, depth) in roots {
        search(&root, depth, &mut budget, &mut found);
        if !found.is_empty() || budget == 0 {
            break;
        }
    }
    // Several copies: take the most recently updated one.
    found.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    found.pop()
}

fn search(root: &Path, max_depth: usize, budget: &mut usize, found: &mut Vec<PathBuf>) {
    const SKIP: [&str; 10] = ["windows", "$recycle.bin", "system volume information", "appdata", "programdata", "node_modules", ".git", "target", "winsxs", "recovery"];
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().to_lowercase();
            if ft.is_file() && name == PROXY_EXE {
                found.push(e.path());
            } else if ft.is_dir() && !ft.is_symlink() && depth < max_depth && !SKIP.contains(&name.as_str()) {
                queue.push_back((e.path(), depth + 1));
            }
        }
    }
}

fn valid_file(p: &str) -> bool {
    !p.trim().is_empty() && Path::new(p).is_file()
}

/// Fill in paths that are empty or point nowhere (`force`: search all of them again and
/// replace those where something was found). Returns one German report line per program.
pub fn fill(s: &mut Settings, force: bool) -> Vec<String> {
    let mut report = Vec::new();
    let mut set = |field: &mut String, label: &str, valid: bool, find: &mut dyn FnMut() -> Option<PathBuf>| {
        if valid && !force {
            report.push(format!("{label}: {field}"));
            return;
        }
        match find() {
            Some(p) => {
                *field = p.to_string_lossy().into_owned();
                report.push(format!("{label}: gefunden - {field}"));
            }
            None if valid => report.push(format!("{label}: {field}")),
            None => report.push(format!("{label}: nicht gefunden")),
        }
    };

    let steam_ok = !s.steam_dir.is_empty() && Path::new(&s.steam_dir).join("steam.exe").exists();
    set(&mut s.steam_dir, "Steam", steam_ok, &mut steam_dir);
    let steam = Some(PathBuf::from(&s.steam_dir)).filter(|p| p.join("steam.exe").exists());

    let ok = valid_file(&s.noita_exe);
    set(&mut s.noita_exe, "Noita", ok, &mut || noita_exe(steam.as_deref()));
    let noita = Some(PathBuf::from(&s.noita_exe)).filter(|p| p.is_file());

    let ok = valid_file(&s.nt_exe);
    set(&mut s.nt_exe, "Noita Together", ok, &mut nt_exe);

    let ok = valid_file(&s.ew_proxy_exe);
    set(&mut s.ew_proxy_exe, "Entangled Worlds Proxy", ok, &mut || ew_proxy_exe(noita.as_deref()));
    report
}

/// True when at least one configured path is missing (worth a background search at startup).
pub fn needs_search(s: &Settings) -> bool {
    !(valid_file(&s.noita_exe) && valid_file(&s.ew_proxy_exe) && valid_file(&s.nt_exe) && Path::new(&s.steam_dir).join("steam.exe").exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_folders() {
        let dir = std::env::temp_dir().join(format!("nss_vdf_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("steamapps")).unwrap();
        std::fs::write(
            dir.join(r"steamapps\libraryfolders.vdf"),
            "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}\n",
        )
        .unwrap();
        let libs = steam_libraries(&dir);
        assert!(libs.contains(&PathBuf::from(r"D:\SteamLibrary")));
        assert!(libs.contains(&PathBuf::from(r"C:\Program Files (x86)\Steam")));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
