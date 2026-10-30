#![cfg_attr(not(test), windows_subsystem = "windows")]

mod detect;
mod ewlog;
mod progress;
mod saves;
mod sys;
mod theme;
mod ui_ew;
mod ui_modern;
mod update;

use anyhow::{Result, bail};
use eframe::egui::{self, Color32, RichText};
use progress::ProgressChoice;
use saves::{Kind, LiveStatus, Paths, Profile, Settings, State};
use theme::kind_color;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant, SystemTime};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = Paths::detect();
    install_crash_log(&paths);

    // Installer: search the program paths once, store them, no window.
    if args.iter().any(|a| a == "--detect") {
        let mut state = saves::load_state(&paths);
        let report = detect::fill(&mut state.settings, false);
        let _ = saves::save_state(&paths, &state);
        let _ = std::fs::write(paths.root.join("detected.txt"), report.join("\r\n"));
        return Ok(());
    }

    // Two instances swapping the same live save at once would be dangerous.
    if !sys::single_instance(args.iter().any(|a| a == "--updated")) {
        sys::message_box("Noita Save Selector", "Noita Save Selector is already running.");
        return Ok(());
    }
    update::cleanup();
    if args.iter().any(|a| a == "--updated") {
        sys::refresh_shell_icons();
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Noita Save Selector {}", update::VERSION))
            .with_icon(window_icon())
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native("Noita Save Selector", options, Box::new(move |cc| Ok(Box::new(App::new(cc, paths)))))
}

/// Panics end up in <data>/crash.log instead of vanishing with the window.
fn install_crash_log(paths: &Paths) {
    let file = paths.root.join("crash.log");
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let line = format!("[{}] v{} {info}\n{}\n\n", saves::now_human(), update::VERSION, std::backtrace::Backtrace::force_capture());
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
            let _ = f.write_all(line.as_bytes());
        }
        default(info);
    }));
}

include!("../assets/icon_art.rs");

/// Same pixel art as the exe icon (assets/icon.txt), scaled 4x.
fn window_icon() -> egui::IconData {
    let rows = icon_pixels(include_str!("../assets/icon.txt"));
    let (scale, n) = (4, rows.len());
    let mut rgba = Vec::with_capacity(n * n * scale * scale * 4);
    for y in 0..n * scale {
        for x in 0..n * scale {
            rgba.extend(rows[y / scale].get(x / scale).copied().unwrap_or([0, 0, 0, 0]));
        }
    }
    egui::IconData { rgba, width: (n * scale) as u32, height: (n * scale) as u32 }
}

enum Dialog {
    Import { name: String, kind: Kind, protected: bool },
    /// The live save is marked for the active profile but holds another run.
    NewRun { name: String, kind: Kind },
    New { name: String, kind: Kind, keep_progress: bool, progress: ProgressChoice },
    /// Give a profile another progress.
    SetProgress { id: String, choice: ProgressChoice },
    /// Create, rename and delete progress sets.
    ProgressSets { new_name: String, from: Option<String>, renaming: Option<(String, String)>, confirm_delete: Option<String> },
    ConfirmDelete(String),
    ConfirmRestore(String, String),
    ConfirmForeign { target: String, play: bool, ew_args: Vec<String> },
    Settings(Settings),
    Update(update::Release),
    Uninstall { delete_data: bool },
}

type JobResult = (Result<String>, State);

struct App {
    paths: Paths,
    state: State,
    profiles: Vec<Profile>,
    live: LiveStatus,
    selected: Option<String>,
    edit: Option<Profile>,
    sel_info: (u64, u64, bool, Vec<String>),
    /// (unlocks, enemies) of the selected profile's progress.
    sel_progress: (usize, usize),
    procs: sys::Procs,
    last_poll: Instant,
    running: Vec<String>,
    steam_on: bool,
    cloud: Option<bool>,
    saw_game: bool,
    busy: Option<(String, Receiver<JobResult>)>,
    log: Vec<String>,
    dialog: Option<Dialog>,
    style_applied: bool,
    hover_card: Option<String>,
    view: View,
    session_start: Option<SystemTime>,
    net: Option<ewlog::NetWatch>,
    ts_ip: Option<String>,
    logs: Vec<ewlog::Session>,
    log_sel: Option<usize>,
    log_loaded: Option<ewlog::Loaded>,
    log_only_problems: bool,
    /// A newer release on GitHub, if any.
    update: Option<update::Release>,
    /// Running update check; `bool` = started by hand (report "up to date" too).
    update_rx: Option<(Receiver<Result<Option<update::Release>>>, bool)>,
    /// A run started directly in Noita sits under the active profile's marker.
    other_run: bool,
    /// Progress mode a merge was last started for (no retry loop when it fails).
    progress_tried: Option<progress::ProgressMode>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Profiles,
    Logs,
}

impl App {
    fn new(_cc: &eframe::CreationContext, paths: Paths) -> Self {
        let state = saves::load_state(&paths);
        let mut app = App {
            paths,
            state,
            profiles: vec![],
            live: LiveStatus::Missing,
            selected: None,
            edit: None,
            sel_info: (0, 0, false, vec![]),
            sel_progress: (0, 0),
            procs: sys::Procs::new(),
            last_poll: Instant::now() - Duration::from_secs(10),
            running: vec![],
            steam_on: false,
            cloud: None,
            saw_game: false,
            busy: None,
            log: vec![],
            dialog: None,
            style_applied: false,
            hover_card: None,
            view: View::Profiles,
            session_start: None,
            net: None,
            ts_ip: sys::tailscale_ip(),
            logs: vec![],
            log_sel: None,
            log_loaded: None,
            log_only_problems: true,
            update: None,
            update_rx: None,
            other_run: false,
            progress_tried: None,
        };
        app.info(format!("Version {} – data: {}", update::VERSION, app.paths.root.display()));
        if detect::needs_search(&app.state.settings) {
            app.search_paths(false);
        }
        if app.state.settings.check_updates {
            app.check_update(false);
        }
        app.reload();
        app.selected = app.state.active.clone().or_else(|| app.profiles.first().map(|p| p.id.clone()));
        // Logs from earlier sessions (the proxy only keeps two) go into the active profile once.
        if app.active_kind() == Some(Kind::EntangledWorlds) {
            app.archive_logs(None);
        }
        app.load_selection();
        if app.live == LiveStatus::Foreign {
            app.open_import_dialog();
        }
        app
    }

    fn reload(&mut self) {
        self.profiles = saves::load_profiles(&self.paths);
        self.live = saves::live_status(&self.paths);
        self.other_run = saves::live_is_other_run(&self.paths, &self.state);
        self.cloud = sys::noita_cloud_enabled(&self.state.settings.steam_dir);
        if self.selected.as_ref().is_some_and(|s| !self.profiles.iter().any(|p| &p.id == s)) {
            self.selected = self.state.active.clone();
        }
        self.load_selection();
    }

    fn load_selection(&mut self) {
        self.edit = self.selected.as_ref().and_then(|id| self.profiles.iter().find(|p| &p.id == id).cloned());
        if let Some(p) = &self.edit {
            let (files, bytes) = saves::dir_stats(&self.paths.profile_save(&p.id));
            let ew = self.paths.profile_ew(&p.id).exists();
            self.sel_info = (files, bytes, ew, saves::list_snapshots(&self.paths, &p.id));
            let live = self.live == LiveStatus::Owned(p.id.clone());
            self.sel_progress = match saves::pool_of(&self.state, p) {
                // The pool doesn't have what the running game added since the last save.
                Some(pool) if progress::pool_dir(&self.paths, &pool).exists() && !live => progress::counts(&progress::pool_dir(&self.paths, &pool)),
                _ if live => progress::counts(&self.paths.live_save),
                _ => progress::counts(&self.paths.profile_save(&p.id)),
            };
        }
        self.refresh_logs();
    }

    fn refresh_logs(&mut self) {
        let prev = self.log_sel.and_then(|i| self.logs.get(i)).map(|s| s.path.clone());
        self.logs = self.selected.as_ref().map(|id| ewlog::list_sessions(&self.paths, id)).unwrap_or_default();
        self.log_sel = prev.and_then(|p| self.logs.iter().position(|s| s.path == p));
        if self.log_sel.is_none() {
            self.log_loaded = None;
        }
    }

    fn select_log(&mut self, i: usize) {
        let Some(s) = self.logs.get(i) else { return };
        match ewlog::load(&s.path) {
            Ok(l) => {
                self.log_sel = Some(i);
                self.log_loaded = Some(l);
            }
            Err(e) => self.info(format!("ERROR reading the log: {e:#}")),
        }
    }

    fn active_kind(&self) -> Option<Kind> {
        let a = self.state.active.as_ref()?;
        self.profiles.iter().find(|p| &p.id == a).map(|p| p.kind)
    }

    /// Copy proxy/Noita logs into the active profile's log folder.
    fn archive_logs(&mut self, since: Option<SystemTime>) {
        let Some(id) = self.state.active.clone() else { return };
        let ew = self.active_kind() == Some(Kind::EntangledWorlds);
        match ewlog::archive(&self.paths, &self.state.settings, &id, ew, since) {
            Ok(0) => {}
            Ok(n) => self.info(format!("{n} log file(s) archived")),
            Err(e) => self.info(format!("ERROR archiving logs: {e:#}")),
        }
        self.refresh_logs();
    }

    fn info(&mut self, msg: impl Into<String>) {
        let line = format!("[{}] {}", chrono::Local::now().format("%H:%M:%S"), msg.into());
        self.log.push(line);
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    fn open_import_dialog(&mut self) {
        let kind = saves::detect_kind(&self.paths.live_save);
        if matches!(self.live, LiveStatus::Owned(_)) {
            // A run under an existing profile's marker: a copy or a run started in Noita.
            self.dialog = Some(Dialog::Import { name: format!("Run {}", saves::now_human()), kind, protected: false });
            return;
        }
        let name = match kind {
            Kind::EntangledWorlds => "Entangled Worlds Run".to_string(),
            Kind::NoitaTogether => "Noita Together Run".to_string(),
            Kind::Singleplayer => "Singleplayer".to_string(),
        };
        self.dialog = Some(Dialog::Import { name, kind, protected: kind.is_multiplayer() });
    }

    fn open_new_run_dialog(&mut self) {
        let kind = saves::detect_kind(&self.paths.live_save);
        self.dialog = Some(Dialog::NewRun { name: format!("Run {}", saves::now_human()), kind });
    }

    fn game_running(&self) -> bool {
        !self.running.is_empty()
    }

    /// Run a file operation on a worker thread. Refused while the game is running.
    fn job(&mut self, label: &str, needs_closed_game: bool, f: impl FnOnce(&Paths, &mut State) -> Result<String> + Send + 'static) {
        if let Some((running, _)) = &self.busy {
            let msg = format!("Please wait: {running} is still running");
            self.info(msg);
            return;
        }
        if needs_closed_game && self.game_running() {
            self.info(format!("Close these first: {}", self.running.join(", ")));
            return;
        }
        let (tx, rx) = channel();
        let paths = self.paths.clone();
        let mut state = self.state.clone();
        std::thread::spawn(move || {
            let res = (|| {
                if needs_closed_game {
                    let running = sys::running_games(&sys::Procs::new().refresh());
                    if !running.is_empty() {
                        bail!("Close these first: {}", running.join(", "));
                    }
                }
                f(&paths, &mut state)
            })();
            let _ = tx.send((res, state));
        });
        self.busy = Some((label.to_string(), rx));
    }

    fn poll_job(&mut self) {
        let Some((label, rx)) = &self.busy else { return };
        let label = label.clone();
        if let Ok((res, state)) = rx.try_recv() {
            self.busy = None;
            self.state = state;
            match res {
                Ok(msg) => self.info(if msg.is_empty() { format!("{label}: done") } else { msg }),
                Err(e) => self.info(format!("ERROR in {label}: {e:#}")),
            }
            self.reload();
        }
    }

    /// Merge progress once after the mode changed (or on the first start with this feature).
    fn poll_progress(&mut self) {
        let mode = self.state.settings.progress;
        let merged = self.state.progress_merged == Some(mode) && self.state.progress_format >= progress::FORMAT;
        if merged || self.progress_tried == Some(mode) || self.busy.is_some() || self.game_running() {
            return;
        }
        self.progress_tried = Some(mode);
        self.job("Progress", true, saves::merge_progress);
    }

    fn poll_system(&mut self) {
        if self.last_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_poll = Instant::now();
        let names = self.procs.refresh();
        self.running = sys::running_games(&names);
        self.steam_on = sys::steam_running(&names);
        if self.running.iter().any(|n| n == "noita_proxy.exe") {
            let net = self.net.get_or_insert_with(|| ewlog::NetWatch::new(&self.state.settings));
            net.poll();
        } else {
            self.net = None;
        }
        if self.game_running() {
            if !self.saw_game {
                self.session_start = Some(SystemTime::now() - Duration::from_secs(10));
            }
            self.saw_game = true;
        } else if self.saw_game && self.busy.is_none() {
            self.saw_game = false;
            let since = self.session_start.take();
            self.archive_logs(since);
            self.live = saves::live_status(&self.paths);
            let owned = matches!((&self.live, &self.state.active), (LiveStatus::Owned(o), Some(a)) if o == a);
            if owned && saves::live_is_other_run(&self.paths, &self.state) {
                self.info("Game closed – Noita holds a different run than the active profile, nothing saved automatically");
                self.reload();
                self.open_new_run_dialog();
            } else if self.state.settings.auto_save_on_exit && owned {
                self.info("Game closed – saving to profile …");
                self.job("Auto-save", true, |p, st| Ok(format!("Auto-save: \"{}\" saved", saves::sync_active(p, st)?)));
            } else {
                self.reload();
            }
        }
    }

    fn activate(&mut self, id: String, play: bool) {
        self.activate_with(id, play, vec![]);
    }

    /// Switch to a profile (if needed) and optionally start it; `ew_args` go to the EW proxy.
    fn activate_with(&mut self, id: String, play: bool, ew_args: Vec<String>) {
        if self.state.active.as_deref() == Some(id.as_str()) && self.live == LiveStatus::Owned(id.clone()) {
            if play && let Some(k) = self.profiles.iter().find(|p| p.id == id).map(|p| p.kind) {
                self.play_with(k, ew_args);
            }
            return;
        }
        if self.live == LiveStatus::Foreign {
            self.dialog = Some(Dialog::ConfirmForeign { target: id, play, ew_args });
            return;
        }
        // Switching would first save the live run into the active profile and replace its run.
        if self.other_run {
            self.info("Decide first what happens with the run in Noita");
            self.open_new_run_dialog();
            return;
        }
        self.do_switch(id, play, ew_args);
    }

    fn do_switch(&mut self, id: String, play: bool, ew_args: Vec<String>) {
        let kind = self.profiles.iter().find(|p| p.id == id).map(|p| p.kind);
        let name = self.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default();
        self.job("Profile switch", true, move |p, st| {
            saves::switch_to(p, st, &id)?;
            let mut msg = format!("Profile \"{name}\" is now active");
            if play && let Some(k) = kind {
                msg.push_str(" – ");
                msg.push_str(&launch_for(k, &st.settings, &ew_args)?);
            }
            Ok(msg)
        });
    }

    fn play_with(&mut self, kind: Kind, ew_args: Vec<String>) {
        let settings = self.state.settings.clone();
        self.job("Start", false, move |_, _| launch_for(kind, &settings, &ew_args));
    }

    fn save_settings(&mut self) {
        if let Err(e) = saves::save_state(&self.paths, &self.state) {
            self.info(format!("ERROR: {e:#}"));
        }
    }

    fn sync_now(&mut self) {
        if self.other_run {
            self.open_new_run_dialog();
            return;
        }
        self.job("Save", true, |p, st| Ok(format!("\"{}\" saved", saves::sync_active(p, st)?)));
    }

    fn toggle_steam(&mut self) {
        let dir = self.state.settings.steam_dir.clone();
        let res = if self.steam_on { sys::shutdown_steam(&dir) } else { sys::start_steam(&dir) };
        match res {
            Ok(()) => self.info(if self.steam_on { "Closing Steam …" } else { "Starting Steam …" }),
            Err(e) => self.info(format!("ERROR: {e:#}")),
        }
    }

    fn save_edit(&mut self, p: &Profile) {
        if p.name.trim().is_empty() {
            self.info("Name must not be empty");
            return;
        }
        match saves::save_profile(&self.paths, p) {
            Ok(()) => self.info(format!("Profile \"{}\" saved", p.name)),
            Err(e) => self.info(format!("ERROR: {e:#}")),
        }
        self.reload();
    }

    /// How a progress choice reads for a profile of `kind`.
    fn choice_label(&self, choice: &ProgressChoice, kind: Kind) -> String {
        match choice {
            ProgressChoice::Own => "Own (only this profile)".into(),
            ProgressChoice::Set(id) if self.state.progress_sets.iter().any(|s| &s.id == id) => saves::pool_name(&self.state, id),
            _ => match progress::pool(self.state.settings.progress, kind) {
                Some(pool) => format!("Default: {}", saves::pool_name(&self.state, pool)),
                None => "Default: own progress".into(),
            },
        }
    }

    /// Dropdown with every progress a profile can use.
    fn progress_picker(&self, ui: &mut egui::Ui, salt: &str, choice: &mut ProgressChoice, kind: Kind) {
        egui::ComboBox::from_id_salt(salt).width(260.0).selected_text(self.choice_label(choice, kind)).show_ui(ui, |ui| {
            ui.selectable_value(choice, ProgressChoice::Default, self.choice_label(&ProgressChoice::Default, kind));
            for s in &self.state.progress_sets {
                ui.selectable_value(choice, ProgressChoice::Set(s.id.clone()), &s.name);
            }
            ui.selectable_value(choice, ProgressChoice::Own, self.choice_label(&ProgressChoice::Own, kind));
        });
    }

    fn claim_live(&mut self, name: &str) {
        match saves::claim_live(&self.paths, &self.state) {
            Ok(()) => self.info(format!("Save assigned to \"{name}\" again")),
            Err(e) => self.info(format!("ERROR: {e:#}")),
        }
        self.reload();
    }

    fn duplicate(&mut self, id: String) {
        self.job("Duplicate", true, move |p, st| Ok(format!("Copy \"{}\" created", saves::duplicate_profile(p, st, &id)?.name)));
    }

    fn active_name(&self) -> Option<String> {
        let a = self.state.active.as_ref()?;
        self.profiles.iter().find(|p| &p.id == a).map(|p| p.name.clone())
    }

    fn play(&mut self, kind: Kind) {
        self.play_with(kind, vec![]);
    }

    /// Look for Noita, Steam, the EW proxy and Noita Together (`force`: also re-check paths that work).
    fn search_paths(&mut self, force: bool) {
        self.info("Searching for program paths …");
        self.job("Path search", false, move |p, st| {
            let report = detect::fill(&mut st.settings, force);
            saves::save_state(p, st)?;
            Ok(format!("Paths: {}", report.join(" | ")))
        });
    }

    fn check_update(&mut self, manual: bool) {
        if self.update_rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(update::check());
        });
        self.update_rx = Some((rx, manual));
        if manual {
            self.info("Checking for updates …");
        }
    }

    fn poll_update(&mut self) {
        let Some((rx, manual)) = &self.update_rx else { return };
        let manual = *manual;
        let Ok(res) = rx.try_recv() else { return };
        self.update_rx = None;
        match res {
            Ok(Some(r)) => {
                self.info(format!("Update available: version {} (installed: {})", r.version, update::VERSION));
                if manual {
                    self.dialog = Some(Dialog::Update(r.clone()));
                }
                self.update = Some(r);
            }
            Ok(None) if manual => self.info(format!("Version {} is up to date", update::VERSION)),
            Ok(None) => {}
            Err(e) if manual => self.info(format!("ERROR checking for updates: {e:#}")),
            Err(_) => {} // offline at startup is not worth a message
        }
    }

    /// Leave the live save00 / EW save_state alone (it stays playable in Noita), optionally remove
    /// the data folder contents, then hand over to the uninstaller and exit.
    fn uninstall(&mut self, delete_data: bool) {
        let res = (|| -> Result<()> {
            if delete_data {
                saves::delete_app_data(&self.paths)?;
            }
            sys::start_uninstall()
        })();
        match res {
            Ok(()) => std::process::exit(0),
            Err(e) => self.info(format!("ERROR: uninstall failed: {e:#}")),
        }
    }

    /// Download + swap the exe, then restart into the new version.
    fn install_update(&mut self, r: update::Release) {
        self.job("Update", true, move |_, _| {
            let exe = update::install(&r)?;
            std::process::Command::new(&exe).arg("--updated").spawn()?;
            std::process::exit(0);
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let mut keep = true;
        match &mut dialog {
            Dialog::Import { name, kind, protected } => {
                let owner = match &self.live {
                    LiveStatus::Owned(id) => self.profiles.iter().find(|p| &p.id == id).map(|p| p.name.clone()),
                    _ => None,
                };
                modal(ctx, "Import current run", 480.0).show(ctx, |ui| {
                    ui.label("The run currently in Noita is stored as a new profile and becomes the active one.\nThe save itself is not changed.");
                    if let Some(owner) = &owner {
                        ui.label(RichText::new(format!("\"{owner}\" keeps the run it has stored; nothing is written into it.")).small().color(theme::INK_MUTED));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(name);
                    });
                    kind_picker(ui, kind);
                    ui.checkbox(protected, "Protect (cannot be deleted)");
                    if *kind == Kind::EntangledWorlds {
                        ui.label(RichText::new("The Entangled Worlds state (%APPDATA%\\quant\\entangledworlds\\data\\save_state) is stored too.").small());
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Import").clicked() && !name.trim().is_empty() {
                            let (n, k, pr) = (name.clone(), *kind, *protected);
                            self.job("Import", true, move |p, st| {
                                let prof = saves::import_live(p, st, &n, k, pr)?;
                                Ok(format!("Profile \"{}\" created and active", prof.name))
                            });
                            keep = false;
                        }
                        if ui.button("Later").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::NewRun { name, kind } => {
                let active = self.active_name().unwrap_or_default();
                modal(ctx, "A different run is in Noita", 520.0).show(ctx, |ui| {
                    ui.label(format!("The run in Noita is not the one stored in \"{active}\" – probably a daily or a new game started directly in Noita."));
                    ui.label(RichText::new(format!("Nothing was saved. \"{active}\" still has its own run stored.")).small().color(theme::INK_MUTED));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(name);
                    });
                    kind_picker(ui, kind);
                    if self.game_running() {
                        ui.label(RichText::new("Close Noita / proxy / Noita Together first, the files are still in use.").color(theme::WARN));
                    }
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        let idle = self.busy.is_none() && !self.game_running();
                        if ui.add_enabled(idle && !name.trim().is_empty(), egui::Button::new(RichText::new("Import as new profile").strong())).clicked() {
                            let (n, k) = (name.clone(), *kind);
                            self.job("Import", true, move |p, st| {
                                let prof = saves::import_live(p, st, &n, k, false)?;
                                Ok(format!("Profile \"{}\" created from the run in Noita and active", prof.name))
                            });
                            keep = false;
                        }
                        if ui
                            .add_enabled(idle, theme::danger_button(&format!("Overwrite \"{active}\"")))
                            .on_hover_text("Replaces the run stored in this profile with the one in Noita.\nThe previous copy stays available as a snapshot.")
                            .clicked()
                        {
                            self.job("Save", true, |p, st| Ok(format!("\"{}\" overwritten with the run in Noita", saves::sync_active(p, st)?)));
                            keep = false;
                        }
                        if ui.button("Later").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::New { name, kind, keep_progress, progress: choice } => {
                modal(ctx, "New profile", 480.0).show(ctx, |ui| {
                    ui.label("Starts a fresh run. The mod list is taken from the current save,\nmultiplayer mods are switched on/off to match the type.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(name);
                    });
                    kind_picker(ui, kind);
                    ui.horizontal(|ui| {
                        ui.label("Progress:");
                        self.progress_picker(ui, "new_progress", choice, *kind);
                    });
                    let own = {
                        let tmp = Profile { id: String::new(), name: String::new(), kind: *kind, note: String::new(), protected: false, created: String::new(), last_used: None, progress: choice.clone() };
                        saves::pool_of(&self.state, &tmp).is_none()
                    };
                    if own {
                        ui.checkbox(keep_progress, "Start with the progress of the save in Noita");
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() && !name.trim().is_empty() {
                            let (n, k, kp, c) = (name.clone(), *kind, *keep_progress && own, choice.clone());
                            self.job("Create", false, move |p, _| {
                                let template = p.live_save.clone();
                                let mut prof = saves::create_profile(p, &n, k, kp, template.exists().then_some(template.as_path()))?;
                                prof.progress = c;
                                saves::save_profile(p, &prof)?;
                                Ok(format!("Profile \"{}\" created", prof.name))
                            });
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::SetProgress { id, choice } => {
                let prof = self.profiles.iter().find(|p| &p.id == id).cloned();
                let live = self.live == LiveStatus::Owned(id.clone());
                modal(ctx, "Change progress?", 520.0).show(ctx, |ui| {
                    let Some(prof) = prof else {
                        keep = false;
                        return;
                    };
                    let from = self.choice_label(&prof.progress, prof.kind);
                    let to = self.choice_label(choice, prof.kind);
                    ui.label(format!("\"{}\" switches from \"{from}\" to \"{to}\".", prof.name));
                    ui.label(
                        RichText::new(format!(
                            "Its progress is first saved into \"{from}\" and backed up in backups\\_progress. Then it gets exactly the unlocks of \"{to}\"{}. The run itself is not touched.",
                            if *choice == ProgressChoice::Own { " (it keeps what it has)" } else { "" }
                        ))
                        .small()
                        .color(theme::INK_MUTED),
                    );
                    if live && self.game_running() {
                        ui.label(RichText::new("This profile is in Noita right now – close the game first.").color(theme::WARN));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!(live && self.game_running()), egui::Button::new(RichText::new("Switch progress").strong())).clicked() {
                            let (id, c) = (id.clone(), choice.clone());
                            self.job("Progress", live, move |p, st| saves::set_profile_progress(p, st, &id, c));
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::ProgressSets { new_name, from, renaming, confirm_delete } => {
                modal(ctx, "Progress sets", 560.0).show(ctx, |ui| {
                    ui.label("Progress sets work like separate save slots: one with everything unlocked, one for challenge runs … Profiles using the same set share their unlocks. Pick the set for a profile under Progress in the profile view.");
                    ui.add_space(8.0);
                    let sets = self.state.progress_sets.clone();
                    if sets.is_empty() {
                        ui.label(RichText::new("No sets yet.").color(theme::INK_MUTED));
                    }
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for s in &sets {
                            let users: Vec<String> =
                                self.profiles.iter().filter(|p| p.progress == ProgressChoice::Set(s.id.clone())).map(|p| p.name.clone()).collect();
                            let (flags, enemies) = progress::counts(&progress::pool_dir(&self.paths, &s.id));
                            ui.horizontal_wrapped(|ui| match renaming {
                                Some((rid, text)) if *rid == s.id => {
                                    ui.text_edit_singleline(text);
                                    if ui.button("Save").clicked() && !text.trim().is_empty() {
                                        if let Err(e) = saves::rename_set(&self.paths, &mut self.state, &s.id, text) {
                                            self.info(format!("ERROR: {e:#}"));
                                        }
                                        *renaming = None;
                                    } else if ui.button("Cancel").clicked() {
                                        *renaming = None;
                                    }
                                }
                                _ => {
                                    ui.label(RichText::new(&s.name).strong());
                                    ui.label(RichText::new(format!("{flags} unlocks · {enemies} enemies")).small().color(theme::INK_MUTED));
                                    if ui.small_button("Rename").clicked() {
                                        *renaming = Some((s.id.clone(), s.name.clone()));
                                    }
                                    if ui.add_enabled(self.busy.is_none(), egui::Button::new("Delete").small()).clicked() {
                                        *confirm_delete = Some(s.id.clone());
                                    }
                                }
                            });
                            let used = if users.is_empty() { "not used by any profile".to_string() } else { format!("used by {}", users.join(", ")) };
                            ui.label(RichText::new(used).small().color(theme::INK_2));
                            if confirm_delete.as_deref() == Some(s.id.as_str()) {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new("Delete? Its profiles go back to the default progress. The set is moved to backups\\_deleted.").color(theme::WARN));
                                    if ui.add(theme::danger_button("Delete set")).clicked() {
                                        let id = s.id.clone();
                                        self.job("Progress", !users.is_empty(), move |p, st| saves::delete_set(p, st, &id));
                                        *confirm_delete = None;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        *confirm_delete = None;
                                    }
                                });
                            }
                            ui.add_space(6.0);
                        }
                    });
                    ui.separator();
                    theme::caption(ui, "New set", theme::INK_MUTED);
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(new_name);
                    });
                    let pools = saves::existing_pools(&self.paths, &self.state);
                    let from_label = |f: &Option<String>| match f {
                        Some(id) => format!("Copy of {}", saves::pool_name(&self.state, id)),
                        None => "Empty (everything locked, like a fresh Noita)".to_string(),
                    };
                    ui.horizontal(|ui| {
                        ui.label("Start:");
                        egui::ComboBox::from_id_salt("set_from").width(320.0).selected_text(from_label(from)).show_ui(ui, |ui| {
                            ui.selectable_value(from, None, from_label(&None));
                            for id in &pools {
                                ui.selectable_value(from, Some(id.clone()), from_label(&Some(id.clone())));
                            }
                        });
                    });
                    if from.is_none() {
                        ui.label(RichText::new("Empty also locks the spells Noita only hands out after certain achievements.").small().color(theme::INK_MUTED));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(self.busy.is_none() && !new_name.trim().is_empty(), egui::Button::new(RichText::new("Create set").strong())).clicked() {
                            let (n, f) = (new_name.clone(), from.clone());
                            self.job("Progress", false, move |p, st| saves::create_set(p, st, &n, f.as_deref()));
                            new_name.clear();
                        }
                        if ui.button("Close").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::ConfirmDelete(id) => {
                let name = self.profiles.iter().find(|p| &p.id == id).map(|p| p.name.clone()).unwrap_or_default();
                modal(ctx, "Delete profile?", 480.0).show(ctx, |ui| {
                    ui.label(format!("Delete \"{name}\"?\nIt is moved to backups\\_deleted, not deleted permanently."));
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            let id = id.clone();
                            self.job("Delete", false, move |p, st| saves::delete_profile(p, st, &id).map(|_| format!("\"{name}\" deleted (moved to backups\\_deleted)")));
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::ConfirmRestore(id, snap) => {
                modal(ctx, "Restore snapshot?", 480.0).show(ctx, |ui| {
                    ui.label(format!("Reset the profile to {snap}?\nThe current state is stored as its own snapshot first."));
                    ui.horizontal(|ui| {
                        if ui.button("Restore").clicked() {
                            let (id, snap) = (id.clone(), snap.clone());
                            self.job("Restore", true, move |p, st| saves::restore_snapshot(p, st, &id, &snap).map(|_| format!("Snapshot {snap} restored")));
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::ConfirmForeign { target, play, ew_args } => {
                modal(ctx, "Unknown save", 480.0).show(ctx, |ui| {
                    ui.label("The current Noita save belongs to no profile.\nRecommended: import it as a profile first.");
                    ui.horizontal(|ui| {
                        if ui.button("Import first").clicked() {
                            self.open_import_dialog();
                            return;
                        }
                        if ui.button("Put aside & switch").on_hover_text("Moved to backups\\_unassigned").clicked() {
                            let (t, pl, a) = (target.clone(), *play, ew_args.clone());
                            self.do_switch(t, pl, a);
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::Settings(s) => {
                modal(ctx, "Settings", 620.0).show(ctx, |ui| {
                    let idle = self.busy.is_none();
                    // Body scrolls, footer stays visible however small the window is.
                    let body_h = (ctx.screen_rect().height() - 190.0).max(120.0);
                    egui::ScrollArea::vertical().max_height(body_h).auto_shrink([false, true]).show(ui, |ui| {
                        theme::caption(ui, "Programs", theme::INK_MUTED);
                        ui.add_space(4.0);
                        for (label, val) in [
                            ("noita.exe", &mut s.noita_exe),
                            ("Entangled Worlds noita_proxy.exe", &mut s.ew_proxy_exe),
                            ("Noita Together.exe", &mut s.nt_exe),
                            ("Steam folder", &mut s.steam_dir),
                        ] {
                            ui.label(RichText::new(label).small().color(theme::INK_2));
                            ui.add(egui::TextEdit::singleline(val).desired_width(f32::INFINITY));
                            ui.add_space(4.0);
                        }
                        if ui
                            .add_enabled(idle, egui::Button::new("Find paths automatically"))
                            .on_hover_text("Searches for Steam, Noita, the EW proxy and Noita Together again.\nFound paths replace the entries above.")
                            .clicked()
                        {
                            self.state.settings = s.clone();
                            self.search_paths(true);
                            keep = false;
                        }

                        ui.add_space(14.0);
                        theme::caption(ui, "Behavior", theme::INK_MUTED);
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Snapshots per profile");
                            ui.add(egui::DragValue::new(&mut s.keep_snapshots).range(1..=50));
                        });
                        ui.checkbox(&mut s.auto_save_on_exit, "Save into the profile automatically when the game closes");
                        ui.checkbox(&mut s.close_steam_for_singleplayer, "Singleplayer: close Steam first and start Noita without Steam");
                        ui.label(RichText::new("Entangled Worlds needs Steam (lobby/network) – Steam is started when needed.").small().color(theme::INK_MUTED));
                        ui.checkbox(&mut s.check_updates, "Check GitHub for updates at startup");

                        ui.add_space(14.0);
                        theme::caption(ui, "Progress", theme::INK_MUTED);
                        ui.add_space(4.0);
                        ui.label(RichText::new("Unlocked spells, perks and enemies (Noita's Progress screen). The runs themselves always stay in their profile.").small().color(theme::INK_MUTED));
                        egui::ComboBox::from_id_salt("progress_mode")
                            .width(300.0)
                            .selected_text(s.progress.label())
                            .show_ui(ui, |ui| {
                                for m in progress::ProgressMode::ALL {
                                    ui.selectable_value(&mut s.progress, m, m.label());
                                }
                            });
                        ui.label(RichText::new(s.progress.hint()).small());
                        ui.label(RichText::new("Single profiles can use their own progress or a progress set instead (Progress in the profile view).").small().color(theme::INK_MUTED));
                        if s.progress != self.state.settings.progress && s.progress != progress::ProgressMode::PerProfile {
                            ui.label(
                                RichText::new("On save, the progress of the affected profiles is merged once the game is closed. Merged progress can't be split again; each profile's previous progress is kept in backups\\_progress.")
                                    .small()
                                    .color(theme::WARN),
                            );
                        }

                        ui.add_space(14.0);
                        theme::caption(ui, "About", theme::INK_MUTED);
                        ui.add_space(4.0);
                        ui.label(RichText::new(format!("Version {}", update::VERSION)).small());
                        ui.label(RichText::new(format!("Data folder: {}", self.paths.root.display())).small());
                        ui.label(RichText::new(format!("Live save: {}", self.paths.live_save.display())).small());
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(self.update_rx.is_none(), egui::Button::new("Check for updates")).clicked() {
                                self.check_update(true);
                                keep = false;
                            }
                            if ui.button("Open data folder").clicked() {
                                sys::open_folder(&self.paths.root);
                            }
                        });

                        ui.add_space(14.0);
                        theme::caption(ui, "Uninstall", theme::DANGER);
                        ui.add_space(4.0);
                        ui.label(RichText::new("Removes the app. The save that is loaded in Noita stays as it is.").small().color(theme::INK_MUTED));
                        if ui.add_enabled(idle, theme::danger_button("Uninstall …")).clicked() {
                            self.dialog = Some(Dialog::Uninstall { delete_data: false });
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.state.settings = s.clone();
                            match saves::save_state(&self.paths, &self.state) {
                                Ok(()) => self.info("Settings saved"),
                                Err(e) => self.info(format!("ERROR: {e:#}")),
                            }
                            self.reload();
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::Uninstall { delete_data } => {
                let in_noita = match &self.live {
                    LiveStatus::Owned(id) => self.profiles.iter().find(|p| &p.id == id).cloned(),
                    _ => None,
                };
                modal(ctx, "Uninstall Noita Save Selector", 520.0).show(ctx, |ui| {
                    ui.label(match &in_noita {
                        Some(p) => format!("\"{}\" is loaded in Noita right now and stays there, so you can keep playing it without this app.", p.name),
                        None => "The save that is in Noita right now stays where it is.".to_string(),
                    });
                    ui.label(RichText::new("Steam, Noita, Entangled Worlds and Noita Together are not touched.").small().color(theme::INK_MUTED));
                    ui.add_space(8.0);
                    ui.checkbox(delete_data, "Also delete the stored profiles, snapshots and logs");
                    if *delete_data {
                        let lost: Vec<&str> = self.profiles.iter().filter(|p| in_noita.as_ref().is_none_or(|n| n.id != p.id)).map(|p| p.name.as_str()).collect();
                        if !lost.is_empty() {
                            ui.label(RichText::new(format!("These profiles exist only in the data folder and will be gone for good: {}", lost.join(", "))).color(theme::DANGER));
                        }
                        ui.label(RichText::new(format!("Data folder: {}", self.paths.root.display())).small().color(theme::INK_MUTED));
                    } else {
                        ui.label(RichText::new(format!("The data folder is kept, a reinstall picks it up again: {}", self.paths.root.display())).small().color(theme::INK_MUTED));
                    }
                    if sys::uninstaller().is_none() {
                        ui.label(RichText::new("Portable copy: the exe is deleted once the app has closed.").small().color(theme::INK_MUTED));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let label = if *delete_data { "Uninstall and delete data" } else { "Uninstall" };
                        if ui.add_enabled(self.busy.is_none(), theme::danger_button(label)).clicked() {
                            self.uninstall(*delete_data);
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::Update(r) => {
                modal(ctx, format!("Update to version {}", r.version), 480.0).show(ctx, |ui| {
                    ui.label(format!("Installed: {}, new: {}", update::VERSION, r.version));
                    if !r.notes.is_empty() {
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            ui.label(RichText::new(&r.notes).small());
                        });
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new("Profiles, backups and settings stay as they are. The app restarts afterwards.").small());
                    let blocked = self.game_running() || self.busy.is_some();
                    if self.game_running() {
                        ui.label(RichText::new("Close Noita / proxy / Noita Together first.").color(Color32::from_rgb(220, 160, 60)));
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!blocked, egui::Button::new("Update now")).clicked() {
                            self.install_update(r.clone());
                            keep = false;
                        }
                        if !r.page.is_empty() && ui.button("Release page").clicked() {
                            ctx.open_url(egui::OpenUrl::new_tab(&r.page));
                        }
                        if ui.button("Later").clicked() {
                            keep = false;
                        }
                    });
                });
            }
        }
        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }
}

/// Centered dialog with a width tied to the screen. A content-sized window jitters when the
/// screen is too small: wrapping changes its width, which changes the wrapping again.
fn modal<'a>(ctx: &egui::Context, title: impl Into<egui::WidgetText>, width: f32) -> egui::Window<'a> {
    let screen = ctx.screen_rect();
    let w = width.min(screen.width() - 48.0).max(220.0);
    egui::Window::new(title)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .min_width(w)
        .max_width(w)
        .max_height((screen.height() - 48.0).max(160.0))
}

fn kind_picker(ui: &mut egui::Ui, kind: &mut Kind) {
    ui.horizontal(|ui| {
        ui.label("Type:");
        for k in Kind::ALL {
            ui.selectable_value(kind, k, RichText::new(k.label()).color(kind_color(k)).strong());
        }
    });
}

/// Start the right program(s) for a profile kind. Runs on the worker thread.
fn launch_for(kind: Kind, s: &Settings, ew_args: &[String]) -> Result<String> {
    let wait_steam = |want_running: bool| {
        let mut procs = sys::Procs::new();
        for _ in 0..60 {
            if sys::steam_running(&procs.refresh()) == want_running {
                return true;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        false
    };
    match kind {
        Kind::Singleplayer => {
            if s.close_steam_for_singleplayer && sys::steam_running(&sys::Procs::new().refresh()) {
                sys::shutdown_steam(&s.steam_dir)?;
                if !wait_steam(false) {
                    bail!("Steam did not close within 30 s");
                }
            }
            sys::launch(&s.noita_exe)?;
            Ok("Noita started".into())
        }
        Kind::EntangledWorlds => {
            if !sys::steam_running(&sys::Procs::new().refresh()) {
                sys::start_steam(&s.steam_dir)?;
                if !wait_steam(true) {
                    bail!("Could not start Steam (needed for Entangled Worlds)");
                }
                std::thread::sleep(Duration::from_secs(8));
            }
            if ew_args.is_empty() {
                sys::launch_with(&s.ew_proxy_exe, ew_args)?;
                return Ok("Entangled Worlds proxy started (create/join a lobby, then Noita starts)".into());
            }
            // Direct IP via the proxy's CLI: it runs without a window and does not start Noita itself.
            sys::launch_ew_pinned(&s.ew_proxy_exe, ew_args)?;
            std::thread::sleep(Duration::from_secs(4));
            if !sys::running_games(&sys::Procs::new().refresh()).iter().any(|n| n == "noita.exe") {
                sys::launch_ew_pinned(&s.noita_exe, &[])?;
            }
            let target = ew_args.get(1).map(String::as_str).unwrap_or("");
            Ok(if ew_args[0] == "--host" {
                format!("EW proxy hosting via IP on port {target} (no proxy window), Noita started")
            } else {
                format!("EW proxy connecting to {target} (no proxy window), Noita started")
            })
        }
        Kind::NoitaTogether => {
            sys::launch(&s.nt_exe)?;
            sys::launch(&s.noita_exe)?;
            Ok("Noita Together + Noita started".into())
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job();
        self.poll_update();
        self.poll_system();
        self.poll_progress();
        ctx.request_repaint_after(Duration::from_millis(if self.busy.is_some() { 100 } else { 1000 }));

        if !self.style_applied {
            theme::apply(ctx);
            self.style_applied = true;
        }
        self.ui_modern(ctx);
        self.dialogs(ctx);
    }
}
