//! Profile storage and the save swapping logic.
//!
//! Layout of the data folder (the "root", see `data_root`):
//!   state.json                      active profile + settings
//!   profiles/<id>/profile.json      metadata
//!   profiles/<id>/save00/           stored copy of Noita's save00
//!   profiles/<id>/ew_save_state/    stored copy of Entangled Worlds' save_state (EW profiles only)
//!   backups/<id>/<timestamp>/       automatic snapshots taken before a profile copy is overwritten
//!   backups/_last_live/             the live save00 as it was right before the last switch
//!   backups/_unassigned/<ts>/       live data that belonged to no profile and had to make room
//!   backups/_deleted/<id>_<ts>/     deleted profiles
//!
//! The live save00 carries a marker file (.nss_profile) with the id of the profile it belongs to.
//! Data is only ever written back into a profile when that marker matches.

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const MARKER: &str = ".nss_profile";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Singleplayer,
    NoitaTogether,
    EntangledWorlds,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Singleplayer, Kind::NoitaTogether, Kind::EntangledWorlds];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Singleplayer => "Singleplayer",
            Kind::NoitaTogether => "Noita Together",
            Kind::EntangledWorlds => "Entangled Worlds",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Kind::Singleplayer => "SP",
            Kind::NoitaTogether => "NT",
            Kind::EntangledWorlds => "EW",
        }
    }

    pub fn is_multiplayer(self) -> bool {
        self != Kind::Singleplayer
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub protected: bool,
    pub created: String,
    #[serde(default)]
    pub last_used: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub noita_exe: String,
    pub ew_proxy_exe: String,
    pub nt_exe: String,
    pub steam_dir: String,
    pub keep_snapshots: usize,
    pub close_steam_for_singleplayer: bool,
    pub auto_save_on_exit: bool,
    /// Entangled Worlds connection: Steam lobby or direct IP (e.g. over Tailscale).
    pub ew_direct_ip: bool,
    pub ew_port: u16,
    /// Last lobby code / ip:port typed into "Join".
    pub ew_join: String,
    /// Ask GitHub for a newer version at startup.
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            // Filled in by detect::fill on first start.
            noita_exe: String::new(),
            ew_proxy_exe: String::new(),
            nt_exe: String::new(),
            steam_dir: String::new(),
            keep_snapshots: 8,
            close_steam_for_singleplayer: false,
            auto_save_on_exit: true,
            ew_direct_ip: false,
            ew_port: 21251,
            ew_join: String::new(),
            check_updates: true,
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct State {
    pub active: Option<String>,
    pub settings: Settings,
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub live_save: PathBuf,
    pub ew_state: PathBuf,
}

impl Paths {
    /// Real locations. NSS_ROOT / NSS_LIVE_SAVE / NSS_EW_STATE override them (used for testing).
    pub fn detect() -> Self {
        let root = std::env::var_os("NSS_ROOT").map(PathBuf::from).unwrap_or_else(data_root);
        let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
        let live_save = std::env::var_os("NSS_LIVE_SAVE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(r"AppData\LocalLow\Nolla_Games_Noita\save00"));
        let ew_state = std::env::var_os("NSS_EW_STATE")
            .map(PathBuf::from)
            .unwrap_or_else(|| appdata.join(r"quant\entangledworlds\data\save_state"));
        Paths { root, live_save, ew_state }
    }

    pub fn profiles(&self) -> PathBuf {
        self.root.join("profiles")
    }
    pub fn backups(&self) -> PathBuf {
        self.root.join("backups")
    }
    pub fn state_file(&self) -> PathBuf {
        self.root.join("state.json")
    }
    pub fn profile_dir(&self, id: &str) -> PathBuf {
        self.profiles().join(id)
    }
    pub fn profile_save(&self, id: &str) -> PathBuf {
        self.profile_dir(id).join("save00")
    }
    pub fn profile_ew(&self, id: &str) -> PathBuf {
        self.profile_dir(id).join("ew_save_state")
    }
    pub fn snapshots(&self, id: &str) -> PathBuf {
        self.backups().join(id)
    }
}

/// Where profiles live:
/// 1. next to the exe if it is used portably (`portable.txt` or an existing `state.json` there),
/// 2. the folder named in `data_dir.txt` next to the exe (written by the installer),
/// 3. `%LOCALAPPDATA%\NoitaSaveSelector`.
fn data_root() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."));
    if exe_dir.join("portable.txt").exists() || exe_dir.join("state.json").exists() {
        return exe_dir;
    }
    let configured = fs::read_to_string(exe_dir.join("data_dir.txt")).ok().map(|s| s.trim().trim_start_matches('\u{feff}').to_string());
    if let Some(dir) = configured.filter(|s| !s.is_empty()) {
        return PathBuf::from(dir);
    }
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("NoitaSaveSelector")).unwrap_or(exe_dir)
}

/// Remove what the app stored (uninstall with "delete data"). Only known entries go: in portable
/// mode the data folder is the exe's folder and may hold unrelated files. The live save00 keeps
/// its content; only our marker is removed so a later install offers to import it again.
pub fn delete_app_data(p: &Paths) -> Result<()> {
    for d in ["profiles", "backups", "logs"] {
        let d = p.root.join(d);
        if d.exists() {
            fs::remove_dir_all(&d).with_context(|| format!("Delete: {}", d.display()))?;
        }
    }
    for f in ["state.json", "state.tmp", "detected.txt", "crash.log"] {
        let _ = fs::remove_file(p.root.join(f));
    }
    let _ = fs::remove_file(p.live_save.join(MARKER));
    let _ = fs::remove_dir(&p.root); // only succeeds when nothing else is left
    Ok(())
}

pub fn now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

pub fn now_human() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

// ---------------------------------------------------------------- state & profiles

pub fn load_state(p: &Paths) -> State {
    fs::read_to_string(p.state_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_state(p: &Paths, st: &State) -> Result<()> {
    fs::create_dir_all(&p.root)?;
    write_atomic(&p.state_file(), &serde_json::to_string_pretty(st)?)
}

pub fn load_profiles(p: &Paths) -> Vec<Profile> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(p.profiles()) {
        for e in rd.flatten() {
            let f = e.path().join("profile.json");
            if let Some(prof) = fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str::<Profile>(&s).ok()) {
                out.push(prof);
            }
        }
    }
    out.sort_by(|a, b| (b.kind.is_multiplayer(), &a.name.to_lowercase()).cmp(&(a.kind.is_multiplayer(), &b.name.to_lowercase())));
    out
}

pub fn save_profile(p: &Paths, prof: &Profile) -> Result<()> {
    let dir = p.profile_dir(&prof.id);
    fs::create_dir_all(&dir)?;
    write_atomic(&dir.join("profile.json"), &serde_json::to_string_pretty(prof)?)
}

fn new_id(p: &Paths) -> String {
    let base = format!("p{}", chrono::Local::now().format("%Y%m%d%H%M%S"));
    let mut id = base.clone();
    let mut n = 1;
    while p.profile_dir(&id).exists() {
        n += 1;
        id = format!("{base}_{n}");
    }
    id
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

// ---------------------------------------------------------------- live save inspection

#[derive(Clone, Debug, PartialEq)]
pub enum LiveStatus {
    /// No save00 at all (Noita will create a fresh one).
    Missing,
    /// save00 carries our marker for this profile id.
    Owned(String),
    /// save00 exists but belongs to no profile (first start, or changed outside the app).
    Foreign,
}

pub fn live_status(p: &Paths) -> LiveStatus {
    if !p.live_save.exists() {
        return LiveStatus::Missing;
    }
    match fs::read_to_string(p.live_save.join(MARKER)) {
        Ok(id) if !id.trim().is_empty() => LiveStatus::Owned(id.trim().to_string()),
        _ if dir_is_empty(&p.live_save) => LiveStatus::Missing,
        _ => LiveStatus::Foreign,
    }
}

fn dir_is_empty(d: &Path) -> bool {
    fs::read_dir(d).map(|mut r| r.next().is_none()).unwrap_or(true)
}

/// Guess the profile kind from the enabled mods in a save's mod_config.xml.
pub fn detect_kind(save_dir: &Path) -> Kind {
    let text = fs::read_to_string(save_dir.join("mod_config.xml")).unwrap_or_default();
    let enabled = enabled_mods(&text);
    if enabled.iter().any(|m| m == "quant.ew") {
        Kind::EntangledWorlds
    } else if enabled.iter().any(|m| m == "noita-together") {
        Kind::NoitaTogether
    } else {
        Kind::Singleplayer
    }
}

pub fn enabled_mods(mod_config: &str) -> Vec<String> {
    mod_tags(mod_config)
        .filter(|t| attr(t, "enabled").as_deref() == Some("1"))
        .filter_map(|t| attr(t, "name"))
        .collect()
}

fn mod_tags(text: &str) -> impl Iterator<Item = &str> {
    text.split("<Mod ").skip(1).map(|s| s.split('>').next().unwrap_or(""))
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let tag = format!(" {tag}");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// Turns the multiplayer mods on/off to match the profile kind. Other mods stay untouched.
pub fn adjust_mod_config(text: &str, kind: Kind) -> String {
    let want = |name: &str| -> Option<&'static str> {
        match name {
            "quant.ew" => Some(if kind == Kind::EntangledWorlds { "1" } else { "0" }),
            "noita-together" => Some(if kind == Kind::NoitaTogether { "1" } else { "0" }),
            "noita-nemesis" if kind != Kind::NoitaTogether => Some("0"),
            _ => None,
        }
    };
    let mut out = String::with_capacity(text.len());
    let mut parts = text.split("<Mod ");
    out.push_str(parts.next().unwrap_or(""));
    for part in parts {
        out.push_str("<Mod ");
        let tag_end = part.find('>').unwrap_or(part.len());
        let (tag, rest) = part.split_at(tag_end);
        let new_tag = match attr(tag, "name").as_deref().and_then(want) {
            Some(v) => tag.replace("enabled=\"1\"", &format!("enabled=\"{v}\"")).replace("enabled=\"0\"", &format!("enabled=\"{v}\"")),
            None => tag.to_string(),
        };
        out.push_str(&new_tag);
        out.push_str(rest);
    }
    out
}

// ---------------------------------------------------------------- filesystem helpers

pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("Create folder: {}", dst.display()))?;
    for e in fs::read_dir(src).with_context(|| format!("Read folder: {}", src.display()))? {
        let e = e?;
        let from = e.path();
        let to = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to).with_context(|| format!("Copy: {}", from.display()))?;
        }
    }
    Ok(())
}

/// (file count, total bytes), ignoring our marker file.
pub fn dir_stats(d: &Path) -> (u64, u64) {
    fn walk(d: &Path, acc: &mut (u64, u64), top: bool) {
        if let Ok(rd) = fs::read_dir(d) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    walk(&path, acc, false);
                } else if !(top && e.file_name() == MARKER) {
                    acc.0 += 1;
                    acc.1 += e.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
        }
    }
    let mut acc = (0, 0);
    walk(d, &mut acc, true);
    acc
}

/// Copy src to dst via a temp dir and verify, so dst is never left half-written.
fn copy_verified(src: &Path, dst_tmp: &Path) -> Result<()> {
    if dst_tmp.exists() {
        fs::remove_dir_all(dst_tmp)?;
    }
    copy_dir(src, dst_tmp)?;
    let (a, b) = (dir_stats(src), dir_stats(dst_tmp));
    if a != b {
        let _ = fs::remove_dir_all(dst_tmp);
        bail!("Copy incomplete ({} files/{} bytes instead of {}/{}) – aborted", b.0, b.1, a.0, a.1);
    }
    Ok(())
}

/// Move a directory; falls back to copy+delete across volumes.
fn move_dir(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_verified(src, dst)?;
    fs::remove_dir_all(src).with_context(|| format!("Delete: {}", src.display()))?;
    Ok(())
}

/// Replace `target` with the contents of `src`; the previous `target` is moved to `old_dest` (if given).
fn replace_dir(src: &Path, target: &Path, old_dest: Option<&Path>) -> Result<()> {
    let tmp = target.with_file_name(format!(
        "{}.incoming",
        target.file_name().and_then(|s| s.to_str()).unwrap_or("dir")
    ));
    copy_verified(src, &tmp)?;
    if target.exists() {
        match old_dest {
            Some(d) => move_dir(target, d)?,
            None => fs::remove_dir_all(target)?,
        }
    }
    fs::rename(&tmp, target).with_context(|| format!("Rename to {}", target.display()))?;
    Ok(())
}

fn rotate_snapshots(p: &Paths, id: &str, keep: usize) {
    let mut snaps = list_snapshots(p, id);
    while snaps.len() > keep.max(1) {
        if let Some(oldest) = snaps.pop() {
            let _ = fs::remove_dir_all(p.snapshots(id).join(oldest));
        }
    }
}

/// Snapshot names, newest first.
pub fn list_snapshots(p: &Paths, id: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(p.snapshots(id))
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

fn write_marker(p: &Paths, id: &str) -> Result<()> {
    fs::create_dir_all(&p.live_save)?;
    fs::write(p.live_save.join(MARKER), id)?;
    Ok(())
}

// ---------------------------------------------------------------- operations

/// Mark the live save as belonging to the active profile (e.g. when the marker went missing).
pub fn claim_live(p: &Paths, st: &State) -> Result<()> {
    let id = st.active.as_deref().ok_or_else(|| anyhow!("No active profile."))?;
    if !p.live_save.exists() {
        bail!("No live save present.");
    }
    write_marker(p, id)
}

/// Store the current live save as a new profile (live stays as it is and becomes active).
pub fn import_live(p: &Paths, st: &mut State, name: &str, kind: Kind, protected: bool) -> Result<Profile> {
    if live_status(p) != LiveStatus::Foreign {
        bail!("The current save already belongs to a profile or does not exist.");
    }
    let id = new_id(p);
    let prof = Profile {
        id: id.clone(),
        name: name.trim().to_string(),
        kind,
        note: String::new(),
        protected,
        created: now_human(),
        last_used: Some(now_human()),
    };
    let res = (|| -> Result<()> {
        save_profile(p, &prof)?;
        copy_verified(&p.live_save, &p.profile_save(&id))?;
        if kind == Kind::EntangledWorlds && p.ew_state.exists() {
            copy_verified(&p.ew_state, &p.profile_ew(&id))?;
        }
        write_marker(p, &id)?;
        st.active = Some(id.clone());
        save_state(p, st)
    })();
    if let Err(e) = res {
        let _ = fs::remove_dir_all(p.profile_dir(&id));
        return Err(e);
    }
    Ok(prof)
}

/// New profile with a fresh run. Keeps unlocked progress (persistent/) and mod list from `template`.
pub fn create_profile(p: &Paths, name: &str, kind: Kind, keep_progress: bool, template: Option<&Path>) -> Result<Profile> {
    let id = new_id(p);
    let prof = Profile {
        id: id.clone(),
        name: name.trim().to_string(),
        kind,
        note: String::new(),
        protected: false,
        created: now_human(),
        last_used: None,
    };
    save_profile(p, &prof)?;
    let save = p.profile_save(&id);
    fs::create_dir_all(&save)?;
    if let Some(t) = template {
        if let Ok(cfg) = fs::read_to_string(t.join("mod_config.xml")) {
            fs::write(save.join("mod_config.xml"), adjust_mod_config(&cfg, kind))?;
        }
        if t.join("mod_settings.bin").exists() {
            fs::copy(t.join("mod_settings.bin"), save.join("mod_settings.bin"))?;
        }
        if keep_progress && t.join("persistent").exists() {
            copy_dir(&t.join("persistent"), &save.join("persistent"))?;
        }
    }
    Ok(prof)
}

/// Copy the live save (and EW state) back into the active profile, keeping a snapshot of the old copy.
pub fn sync_active(p: &Paths, st: &State) -> Result<String> {
    let id = st.active.clone().ok_or_else(|| anyhow!("No active profile."))?;
    match live_status(p) {
        LiveStatus::Owned(owner) if owner == id => {}
        LiveStatus::Missing => bail!("No live save present – nothing to save."),
        _ => bail!("The live save does not belong to the active profile – save refused (overwrite protection)."),
    }
    let prof = load_profiles(p).into_iter().find(|x| x.id == id).ok_or_else(|| anyhow!("Active profile is missing."))?;
    let mut snap = p.snapshots(&id).join(now_stamp());
    let mut n = 1;
    while snap.exists() {
        n += 1;
        snap = p.snapshots(&id).join(format!("{}_{n}", now_stamp()));
    }
    replace_dir(&p.live_save, &p.profile_save(&id), Some(&snap.join("save00")))?;
    if prof.kind == Kind::EntangledWorlds && p.ew_state.exists() {
        replace_dir(&p.ew_state, &p.profile_ew(&id), Some(&snap.join("ew_save_state")))?;
    }
    rotate_snapshots(p, &id, st.settings.keep_snapshots);
    Ok(prof.name)
}

/// Make `target` the live save. The current live save goes back into its own profile first.
/// Foreign live data (belonging to no profile) is parked in backups/_unassigned, never deleted.
pub fn switch_to(p: &Paths, st: &mut State, target: &str) -> Result<()> {
    let profiles = load_profiles(p);
    let tgt = profiles.iter().find(|x| x.id == target).ok_or_else(|| anyhow!("Profile not found."))?.clone();
    let stamp = now_stamp();

    // 1. Put the live data where it belongs.
    let mut leaving_ew = false;
    match live_status(p) {
        LiveStatus::Owned(owner) if profiles.iter().any(|x| x.id == owner) => {
            let mut tmp = st.clone();
            tmp.active = Some(owner.clone());
            sync_active(p, &tmp)?;
            leaving_ew = profiles.iter().any(|x| x.id == owner && x.kind == Kind::EntangledWorlds);
            let last = p.backups().join("_last_live");
            if last.exists() {
                fs::remove_dir_all(&last)?;
            }
            move_dir(&p.live_save, &last)?;
        }
        LiveStatus::Owned(_) | LiveStatus::Foreign => {
            move_dir(&p.live_save, &p.backups().join("_unassigned").join(&stamp).join("save00"))?;
        }
        LiveStatus::Missing => {
            if p.live_save.exists() {
                fs::remove_dir_all(&p.live_save)?;
            }
        }
    }

    // 2. EW state on disk: already stored in its profile if we leave an EW profile, otherwise park it.
    if p.ew_state.exists() {
        if leaving_ew {
            let last = p.backups().join("_last_ew_state");
            if last.exists() {
                fs::remove_dir_all(&last)?;
            }
            move_dir(&p.ew_state, &last)?;
        } else {
            move_dir(&p.ew_state, &p.backups().join("_unassigned").join(&stamp).join("ew_save_state"))?;
        }
    }

    // 3. Install the target.
    let src = p.profile_save(&tgt.id);
    if src.exists() {
        replace_dir(&src, &p.live_save, None)?;
    } else {
        fs::create_dir_all(&p.live_save)?;
    }
    if tgt.kind == Kind::EntangledWorlds && p.profile_ew(&tgt.id).exists() {
        replace_dir(&p.profile_ew(&tgt.id), &p.ew_state, None)?;
    }
    write_marker(p, &tgt.id)?;

    st.active = Some(tgt.id.clone());
    save_state(p, st)?;
    let mut t = tgt;
    t.last_used = Some(now_human());
    save_profile(p, &t)?;
    Ok(())
}

/// Replace a profile's stored copy with one of its snapshots (current copy is snapshotted first).
/// If the profile is live, the live save is replaced too (old live goes to backups/_last_live).
pub fn restore_snapshot(p: &Paths, st: &State, id: &str, snap: &str) -> Result<()> {
    let snap_dir = p.snapshots(id).join(snap);
    if !snap_dir.join("save00").exists() {
        bail!("Snapshot contains no save00.");
    }
    let safety = p.snapshots(id).join(format!("{}_before-restore", now_stamp()));
    replace_dir(&snap_dir.join("save00"), &p.profile_save(id), Some(&safety.join("save00")))?;
    if snap_dir.join("ew_save_state").exists() {
        replace_dir(&snap_dir.join("ew_save_state"), &p.profile_ew(id), Some(&safety.join("ew_save_state")))?;
    }
    if st.active.as_deref() == Some(id) && live_status(p) == LiveStatus::Owned(id.to_string()) {
        let last = p.backups().join("_last_live");
        if last.exists() {
            fs::remove_dir_all(&last)?;
        }
        replace_dir(&p.profile_save(id), &p.live_save, Some(&last))?;
        write_marker(p, id)?;
        if p.profile_ew(id).exists() {
            replace_dir(&p.profile_ew(id), &p.ew_state, None)?;
        }
    }
    Ok(())
}

pub fn duplicate_profile(p: &Paths, st: &State, id: &str) -> Result<Profile> {
    let src = load_profiles(p).into_iter().find(|x| x.id == id).ok_or_else(|| anyhow!("Profile not found."))?;
    if st.active.as_deref() == Some(id) && live_status(p) == LiveStatus::Owned(id.to_string()) {
        sync_active(p, st)?;
    }
    let new = Profile {
        id: new_id(p),
        name: format!("{} (copy)", src.name),
        protected: false,
        created: now_human(),
        last_used: None,
        ..src
    };
    save_profile(p, &new)?;
    if p.profile_save(id).exists() {
        copy_dir(&p.profile_save(id), &p.profile_save(&new.id))?;
    }
    if p.profile_ew(id).exists() {
        copy_dir(&p.profile_ew(id), &p.profile_ew(&new.id))?;
    }
    Ok(new)
}

/// Deleting only moves the profile into backups/_deleted.
pub fn delete_profile(p: &Paths, st: &State, id: &str) -> Result<()> {
    let prof = load_profiles(p).into_iter().find(|x| x.id == id).ok_or_else(|| anyhow!("Profile not found."))?;
    if prof.protected {
        bail!("Profile is protected. Remove the protection first.");
    }
    if st.active.as_deref() == Some(id) {
        bail!("The active profile cannot be deleted. Activate another one first.");
    }
    move_dir(&p.profile_dir(id), &p.backups().join("_deleted").join(format!("{id}_{}", now_stamp())))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Env {
        _dir: PathBuf,
        p: Paths,
    }

    fn env(name: &str) -> Env {
        let dir = std::env::temp_dir().join(format!("nss_test_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = Paths { root: dir.join("root"), live_save: dir.join("live/save00"), ew_state: dir.join("ew/save_state") };
        Env { _dir: dir, p }
    }

    fn put(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    const EW_CFG: &str = r#"<Mods>
  <Mod enabled="1" name="quant.ew" settings_fold_open="0" workshop_item_id="0" >
  </Mod>
  <Mod enabled="0" name="noita-together" settings_fold_open="0" workshop_item_id="0" >
  </Mod>
  <Mod enabled="1" name="spelldesc" settings_fold_open="0" workshop_item_id="3419582553" >
  </Mod>
</Mods>"#;

    #[test]
    fn detect_and_adjust_mods() {
        let e = env("mods");
        put(&e.p.live_save.join("mod_config.xml"), EW_CFG);
        assert_eq!(detect_kind(&e.p.live_save), Kind::EntangledWorlds);
        let nt = adjust_mod_config(EW_CFG, Kind::NoitaTogether);
        assert_eq!(enabled_mods(&nt), vec!["noita-together", "spelldesc"]);
        let sp = adjust_mod_config(EW_CFG, Kind::Singleplayer);
        assert_eq!(enabled_mods(&sp), vec!["spelldesc"]);
    }

    #[test]
    fn full_roundtrip_keeps_ew_run_intact() {
        let e = env("roundtrip");
        let p = &e.p;
        let mut st = State::default();
        // The running EW game.
        put(&p.live_save.join("mod_config.xml"), EW_CFG);
        put(&p.live_save.join("world/chunk1.bin"), "ew-world");
        put(&p.live_save.join("player.xml"), "ew-player");
        put(&p.ew_state.join("world_chunks.bit"), "ew-chunks");
        assert_eq!(live_status(p), LiveStatus::Foreign);

        let ew = import_live(p, &mut st, "EW Duo Run", Kind::EntangledWorlds, true).unwrap();
        assert_eq!(live_status(p), LiveStatus::Owned(ew.id.clone()));
        assert_eq!(read(&p.profile_ew(&ew.id).join("world_chunks.bit")), "ew-chunks");

        // Play a bit more in EW, then switch to a new singleplayer profile.
        put(&p.live_save.join("player.xml"), "ew-player-v2");
        put(&p.ew_state.join("world_chunks.bit"), "ew-chunks-v2");
        let sp = create_profile(p, "Solo", Kind::Singleplayer, true, Some(&p.live_save.clone())).unwrap();
        switch_to(p, &mut st, &sp.id).unwrap();
        assert_eq!(live_status(p), LiveStatus::Owned(sp.id.clone()));
        assert!(!p.live_save.join("player.xml").exists());
        assert!(!p.ew_state.exists(), "EW state must not linger under a SP profile");
        assert_eq!(enabled_mods(&read(&p.live_save.join("mod_config.xml"))), vec!["spelldesc"]);
        // EW progress was stored.
        assert_eq!(read(&p.profile_save(&ew.id).join("player.xml")), "ew-player-v2");
        assert_eq!(read(&p.profile_ew(&ew.id).join("world_chunks.bit")), "ew-chunks-v2");
        // And the previous copy is in a snapshot.
        assert_eq!(list_snapshots(p, &ew.id).len(), 1);

        // Singleplayer game writes something, then back to EW.
        put(&p.live_save.join("player.xml"), "sp-player");
        switch_to(p, &mut st, &ew.id).unwrap();
        assert_eq!(read(&p.live_save.join("player.xml")), "ew-player-v2");
        assert_eq!(read(&p.live_save.join("world/chunk1.bin")), "ew-world");
        assert_eq!(read(&p.ew_state.join("world_chunks.bit")), "ew-chunks-v2");
        assert_eq!(read(&p.profile_save(&sp.id).join("player.xml")), "sp-player");

        // Protected profile can't be deleted; active can't either.
        assert!(delete_profile(p, &st, &ew.id).is_err());
        delete_profile(p, &st, &sp.id).unwrap();
        assert!(!p.profile_dir(&sp.id).exists());
    }

    #[test]
    fn foreign_live_is_never_written_into_a_profile() {
        let e = env("foreign");
        let p = &e.p;
        let mut st = State::default();
        put(&p.live_save.join("player.xml"), "a");
        let a = import_live(p, &mut st, "A", Kind::Singleplayer, false).unwrap();
        // Something outside the app replaces save00 (e.g. Steam Cloud).
        fs::remove_dir_all(&p.live_save).unwrap();
        put(&p.live_save.join("player.xml"), "cloud-garbage");
        assert!(sync_active(p, &st).is_err());
        let b = create_profile(p, "B", Kind::Singleplayer, false, None).unwrap();
        switch_to(p, &mut st, &b.id).unwrap();
        assert_eq!(read(&p.profile_save(&a.id).join("player.xml")), "a");
        let parked = list_snapshots_in(&p.backups().join("_unassigned"));
        assert_eq!(parked.len(), 1);
    }

    #[test]
    fn snapshot_restore_and_rotation() {
        let e = env("snap");
        let p = &e.p;
        let mut st = State::default();
        st.settings.keep_snapshots = 2;
        put(&p.live_save.join("player.xml"), "v0");
        let a = import_live(p, &mut st, "A", Kind::Singleplayer, false).unwrap();
        for v in 1..=4 {
            put(&p.live_save.join("player.xml"), &format!("v{v}"));
            std::thread::sleep(std::time::Duration::from_millis(1100));
            sync_active(p, &st).unwrap();
        }
        let snaps = list_snapshots(p, &a.id);
        assert_eq!(snaps.len(), 2);
        // Newest snapshot holds v3 (the copy before the last sync).
        restore_snapshot(p, &st, &a.id, &snaps[0]).unwrap();
        assert_eq!(read(&p.live_save.join("player.xml")), "v3");
        assert_eq!(live_status(p), LiveStatus::Owned(a.id.clone()));
    }

    /// Run manually against a COPY of real data: set NSS_ROOT / NSS_LIVE_SAVE / NSS_EW_STATE, then
    /// `cargo test --release -- --ignored real_data`.
    #[test]
    #[ignore]
    fn real_data_roundtrip() {
        let p = Paths::detect();
        assert!(std::env::var_os("NSS_LIVE_SAVE").is_some(), "only run against a sandbox copy");
        let before = (dir_stats(&p.live_save), dir_stats(&p.ew_state));
        let mut st = State::default();
        let t = std::time::Instant::now();
        let ew = import_live(&p, &mut st, "EW Run", detect_kind(&p.live_save), true).unwrap();
        eprintln!("import {:?} kind={:?}", t.elapsed(), ew.kind);
        let sp = create_profile(&p, "Solo", Kind::Singleplayer, true, Some(&p.live_save.clone())).unwrap();
        let t = std::time::Instant::now();
        switch_to(&p, &mut st, &sp.id).unwrap();
        eprintln!("switch to SP {:?}", t.elapsed());
        let t = std::time::Instant::now();
        switch_to(&p, &mut st, &ew.id).unwrap();
        eprintln!("switch back to EW {:?}", t.elapsed());
        assert_eq!((dir_stats(&p.live_save), dir_stats(&p.ew_state)), before);
        for f in ["player.xml", "world_state.xml", "mod_config.xml"] {
            assert_eq!(fs::read(p.live_save.join(f)).unwrap(), fs::read(p.profile_save(&ew.id).join(f)).unwrap());
        }
    }

    fn list_snapshots_in(d: &Path) -> Vec<String> {
        fs::read_dir(d).map(|r| r.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default()
    }
}
