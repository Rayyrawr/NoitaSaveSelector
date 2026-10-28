#![cfg_attr(not(test), windows_subsystem = "windows")]

mod detect;
mod ewlog;
mod saves;
mod sys;
mod theme;
mod ui_ew;
mod ui_modern;
mod update;

use anyhow::{Result, bail};
use eframe::egui::{self, Color32, RichText};
use saves::{Kind, LiveStatus, Paths, Profile, Settings, State};
use theme::{UiTheme, kind_color};
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
        sys::message_box("Noita Save Selector", "Der Noita Save Selector läuft bereits.");
        return Ok(());
    }
    update::cleanup();

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

/// Same pixel art as the exe icon (assets/icon.txt), scaled 4x.
fn window_icon() -> egui::IconData {
    let rows: Vec<&str> = include_str!("../assets/icon.txt").lines().filter(|l| !l.is_empty()).collect();
    let (scale, n) = (4, rows.len());
    let mut rgba = Vec::with_capacity(n * n * scale * scale * 4);
    for y in 0..n * scale {
        for x in 0..n * scale {
            let c = rows[y / scale].as_bytes().get(x / scale).copied().unwrap_or(b'.');
            rgba.extend(match c {
                b'C' => [150, 100, 60, 255],
                b'G' => [200, 200, 215, 255],
                b'P' => [140, 80, 210, 255],
                b'W' => [230, 210, 255, 255],
                b'D' => [90, 45, 150, 255],
                _ => [0, 0, 0, 0],
            });
        }
    }
    egui::IconData { rgba, width: (n * scale) as u32, height: (n * scale) as u32 }
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::none()
        .fill(color)
        .rounding(4.0)
        .inner_margin(egui::Margin::symmetric(6.0, 1.0))
        .show(ui, |ui| ui.label(RichText::new(text).color(Color32::WHITE).strong().small()));
}

enum Dialog {
    Import { name: String, kind: Kind, protected: bool },
    New { name: String, kind: Kind, keep_progress: bool },
    ConfirmDelete(String),
    ConfirmRestore(String, String),
    ConfirmForeign { target: String, play: bool, ew_args: Vec<String> },
    Settings(Settings),
    Update(update::Release),
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
    procs: sys::Procs,
    last_poll: Instant,
    running: Vec<String>,
    steam_on: bool,
    cloud: Option<bool>,
    saw_game: bool,
    busy: Option<(String, Receiver<JobResult>)>,
    log: Vec<String>,
    dialog: Option<Dialog>,
    applied_theme: Option<UiTheme>,
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
            procs: sys::Procs::new(),
            last_poll: Instant::now() - Duration::from_secs(10),
            running: vec![],
            steam_on: false,
            cloud: None,
            saw_game: false,
            busy: None,
            log: vec![],
            dialog: None,
            applied_theme: None,
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
        };
        app.info(format!("Version {} – Daten: {}", update::VERSION, app.paths.root.display()));
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
            Err(e) => self.info(format!("FEHLER beim Lesen des Logs: {e:#}")),
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
            Ok(n) => self.info(format!("{n} Log-Datei(en) archiviert")),
            Err(e) => self.info(format!("FEHLER beim Archivieren der Logs: {e:#}")),
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
        let name = match kind {
            Kind::EntangledWorlds => "Entangled Worlds Run".to_string(),
            Kind::NoitaTogether => "Noita Together Run".to_string(),
            Kind::Singleplayer => "Singleplayer".to_string(),
        };
        self.dialog = Some(Dialog::Import { name, kind, protected: kind.is_multiplayer() });
    }

    fn game_running(&self) -> bool {
        !self.running.is_empty()
    }

    /// Run a file operation on a worker thread. Refused while the game is running.
    fn job(&mut self, label: &str, needs_closed_game: bool, f: impl FnOnce(&Paths, &mut State) -> Result<String> + Send + 'static) {
        if let Some((running, _)) = &self.busy {
            let msg = format!("Bitte warten: {running} läuft noch");
            self.info(msg);
            return;
        }
        if needs_closed_game && self.game_running() {
            self.info(format!("Bitte erst schließen: {}", self.running.join(", ")));
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
                        bail!("Bitte erst schließen: {}", running.join(", "));
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
                Ok(msg) => self.info(if msg.is_empty() { format!("{label}: fertig") } else { msg }),
                Err(e) => self.info(format!("FEHLER bei {label}: {e:#}")),
            }
            self.reload();
        }
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
            if self.state.settings.auto_save_on_exit && owned {
                self.info("Spiel beendet – sichere Spielstand ins Profil …");
                self.job("Auto-Sicherung", true, |p, st| Ok(format!("Auto-Sicherung: \"{}\" gesichert", saves::sync_active(p, st)?)));
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
        self.do_switch(id, play, ew_args);
    }

    fn do_switch(&mut self, id: String, play: bool, ew_args: Vec<String>) {
        let kind = self.profiles.iter().find(|p| p.id == id).map(|p| p.kind);
        let name = self.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default();
        self.job("Profilwechsel", true, move |p, st| {
            saves::switch_to(p, st, &id)?;
            let mut msg = format!("Profil \"{name}\" ist jetzt aktiv");
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
            self.info(format!("FEHLER: {e:#}"));
        }
    }

    fn sync_now(&mut self) {
        self.job("Sichern", true, |p, st| Ok(format!("\"{}\" gesichert", saves::sync_active(p, st)?)));
    }

    fn toggle_steam(&mut self) {
        let dir = self.state.settings.steam_dir.clone();
        let res = if self.steam_on { sys::shutdown_steam(&dir) } else { sys::start_steam(&dir) };
        match res {
            Ok(()) => self.info(if self.steam_on { "Steam wird beendet …" } else { "Steam wird gestartet …" }),
            Err(e) => self.info(format!("FEHLER: {e:#}")),
        }
    }

    fn save_edit(&mut self, p: &Profile) {
        if p.name.trim().is_empty() {
            self.info("Name darf nicht leer sein");
            return;
        }
        match saves::save_profile(&self.paths, p) {
            Ok(()) => self.info(format!("Profil \"{}\" gespeichert", p.name)),
            Err(e) => self.info(format!("FEHLER: {e:#}")),
        }
        self.reload();
    }

    fn claim_live(&mut self, name: &str) {
        match saves::claim_live(&self.paths, &self.state) {
            Ok(()) => self.info(format!("Spielstand wieder \"{name}\" zugeordnet")),
            Err(e) => self.info(format!("FEHLER: {e:#}")),
        }
        self.reload();
    }

    fn duplicate(&mut self, id: String) {
        self.job("Duplizieren", true, move |p, st| Ok(format!("Kopie \"{}\" angelegt", saves::duplicate_profile(p, st, &id)?.name)));
    }

    fn set_theme(&mut self, t: UiTheme) {
        self.state.settings.theme = t;
        if let Err(e) = saves::save_state(&self.paths, &self.state) {
            self.info(format!("FEHLER: {e:#}"));
        }
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
        self.info("Suche Programmpfade …");
        self.job("Pfadsuche", false, move |p, st| {
            let report = detect::fill(&mut st.settings, force);
            saves::save_state(p, st)?;
            Ok(format!("Pfade: {}", report.join(" | ")))
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
            self.info("Suche nach Updates …");
        }
    }

    fn poll_update(&mut self) {
        let Some((rx, manual)) = &self.update_rx else { return };
        let manual = *manual;
        let Ok(res) = rx.try_recv() else { return };
        self.update_rx = None;
        match res {
            Ok(Some(r)) => {
                self.info(format!("Update verfügbar: Version {} (installiert: {})", r.version, update::VERSION));
                if manual {
                    self.dialog = Some(Dialog::Update(r.clone()));
                }
                self.update = Some(r);
            }
            Ok(None) if manual => self.info(format!("Version {} ist aktuell", update::VERSION)),
            Ok(None) => {}
            Err(e) if manual => self.info(format!("FEHLER bei der Update-Suche: {e:#}")),
            Err(_) => {} // offline at startup is not worth a message
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

    // ------------------------------------------------------------ UI parts

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Noita Save Selector");
            ui.separator();
            if self.game_running() {
                badge(ui, &format!("LÄUFT: {}", self.running.join(", ")), Color32::from_rgb(200, 120, 30));
            } else {
                badge(ui, "Spiel geschlossen", Color32::from_rgb(60, 90, 60));
            }
            if let Some((net, color)) = self.net_status() {
                ui.label("Netz:");
                badge(ui, &net, color.gamma_multiply(0.6));
            }
            if self.proxy_orphaned() && ui.small_button("Proxy beenden").clicked() {
                self.stop_proxy();
            }
            ui.separator();
            if ui.selectable_label(self.view == View::Profiles, "Profile").clicked() {
                self.view = View::Profiles;
            }
            if ui.selectable_label(self.view == View::Logs, "Logs").clicked() {
                self.view = View::Logs;
            }
            ui.separator();
            ui.label("Steam:");
            if self.steam_on {
                badge(ui, "läuft", Color32::from_rgb(40, 90, 150));
                if ui.small_button("Steam schließen").on_hover_text("Beendet Steam sauber (steam.exe -shutdown)").clicked() {
                    match sys::shutdown_steam(&self.state.settings.steam_dir) {
                        Ok(()) => self.info("Steam wird beendet …"),
                        Err(e) => self.info(format!("FEHLER: {e:#}")),
                    }
                }
            } else {
                badge(ui, "aus", Color32::from_rgb(80, 80, 80));
                if ui.small_button("Steam starten").clicked() {
                    match sys::start_steam(&self.state.settings.steam_dir) {
                        Ok(()) => self.info("Steam wird gestartet …"),
                        Err(e) => self.info(format!("FEHLER: {e:#}")),
                    }
                }
            }
            ui.separator();
            ui.label("Steam Cloud (Noita):");
            match self.cloud {
                Some(false) => badge(ui, "AUS – gut", Color32::from_rgb(60, 120, 60)),
                Some(true) => {
                    badge(ui, "AN", Color32::from_rgb(180, 50, 50));
                    ui.label(RichText::new("(?)").color(Color32::LIGHT_RED)).on_hover_text(
                        "Steam Cloud kann beim Profilwechsel Konflikte melden oder Saves überschreiben.\n\
                         Ausschalten: Steam > Bibliothek > Noita > Rechtsklick > Eigenschaften > Allgemein >\n\
                         'Spielstände in der Steam Cloud speichern' deaktivieren.",
                    );
                }
                None => badge(ui, "unbekannt", Color32::from_rgb(80, 80, 80)),
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Einstellungen").clicked() {
                    self.dialog = Some(Dialog::Settings(self.state.settings.clone()));
                }
                if ui.button("Modern").on_hover_text("Zum neuen Design wechseln").clicked() {
                    self.set_theme(UiTheme::Modern);
                }
                if let Some(r) = &self.update
                    && ui.button(RichText::new(format!("Update {}", r.version)).color(Color32::from_rgb(120, 220, 120))).clicked()
                {
                    self.dialog = Some(Dialog::Update(r.clone()));
                }
            });
        });
    }

    fn live_banner(&mut self, ui: &mut egui::Ui) {
        match &self.live {
            LiveStatus::Foreign => {
                egui::Frame::none().fill(Color32::from_rgb(90, 60, 20)).inner_margin(6.0).rounding(4.0).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Der aktuelle Noita-Spielstand gehört zu keinem Profil.").strong());
                        if ui.button("Jetzt als Profil übernehmen").clicked() {
                            self.open_import_dialog();
                        }
                        let active = self.state.active.as_ref().and_then(|a| self.profiles.iter().find(|p| &p.id == a)).map(|p| p.name.clone());
                        if let Some(name) = active {
                            if ui
                                .button(format!("Gehört zu \"{name}\""))
                                .on_hover_text("Nur wählen, wenn du sicher bist, dass das der Spielstand dieses Profils ist\n(z.B. nach einem neuen Run, wenn die Markierung fehlt).")
                                .clicked()
                            {
                                match saves::claim_live(&self.paths, &self.state) {
                                    Ok(()) => self.info(format!("Spielstand wieder \"{name}\" zugeordnet")),
                                    Err(e) => self.info(format!("FEHLER: {e:#}")),
                                }
                                self.reload();
                            }
                        }
                    });
                });
            }
            LiveStatus::Owned(id) => {
                let name = self.profiles.iter().find(|p| &p.id == id).map(|p| p.name.clone()).unwrap_or_else(|| "(gelöschtes Profil)".into());
                ui.label(format!("Im Spiel geladen: {name}"));
            }
            LiveStatus::Missing => {
                ui.label("Im Spiel geladen: (kein Spielstand – Noita startet neu)");
            }
        }
    }

    fn profile_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("Profile");
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(ui.available_height() - 70.0).show(ui, |ui| {
            let mut clicked = None;
            for p in &self.profiles {
                let active = self.state.active.as_deref() == Some(p.id.as_str());
                let sel = self.selected.as_deref() == Some(p.id.as_str());
                ui.horizontal(|ui| {
                    badge(ui, p.kind.short(), kind_color(p.kind));
                    let mut text = RichText::new(&p.name);
                    if active {
                        text = text.strong().color(Color32::from_rgb(120, 220, 120));
                    }
                    if ui.selectable_label(sel, text).clicked() {
                        clicked = Some(p.id.clone());
                    }
                    if active {
                        ui.label(RichText::new("aktiv").small().color(Color32::from_rgb(120, 220, 120)));
                    }
                    if p.protected {
                        ui.label(RichText::new("geschützt").small().color(Color32::GOLD));
                    }
                });
            }
            if self.profiles.is_empty() {
                ui.label("Noch keine Profile.");
            }
            if let Some(id) = clicked {
                self.selected = Some(id);
                self.load_selection();
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("+ Neues Profil").clicked() {
                self.dialog = Some(Dialog::New { name: String::new(), kind: Kind::Singleplayer, keep_progress: true });
            }
            if self.live == LiveStatus::Foreign && ui.button("Aktuellen Save übernehmen").clicked() {
                self.open_import_dialog();
            }
        });
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        let Some(mut p) = self.edit.clone() else {
            ui.label("Links ein Profil auswählen.");
            return;
        };
        let active = self.state.active.as_deref() == Some(p.id.as_str());
        let busy = self.busy.is_some();
        let locked = busy || self.game_running();

        ui.horizontal(|ui| {
            badge(ui, p.kind.label(), kind_color(p.kind));
            ui.heading(&p.name);
            if active {
                badge(ui, "AKTIV", Color32::from_rgb(40, 140, 60));
            }
            if p.protected {
                badge(ui, "GESCHÜTZT", Color32::from_rgb(160, 130, 20));
            }
        });
        ui.add_space(8.0);

        // Main actions.
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!locked, |ui| {
                let big = |t: &str| egui::Button::new(RichText::new(t).size(17.0)).min_size(egui::vec2(170.0, 34.0));
                if active {
                    let play_label = match p.kind {
                        Kind::EntangledWorlds => "Spielen (EW Proxy)",
                        Kind::NoitaTogether => "Spielen (Noita Together)",
                        Kind::Singleplayer => "Spielen",
                    };
                    if ui.add(big(play_label)).clicked() {
                        self.play(p.kind);
                    }
                    if ui.button("Jetzt sichern").on_hover_text("Live-Spielstand ins Profil kopieren (mit Snapshot)").clicked() {
                        self.job("Sichern", true, |p, st| Ok(format!("\"{}\" gesichert", saves::sync_active(p, st)?)));
                    }
                } else {
                    if ui.add(big("Aktivieren & Spielen")).clicked() {
                        self.activate(p.id.clone(), true);
                    }
                    if ui.button("Nur aktivieren").clicked() {
                        self.activate(p.id.clone(), false);
                    }
                }
            });
        });
        if self.game_running() && !busy {
            ui.label(RichText::new("Wechseln/Sichern geht erst, wenn Noita (und der EW-Proxy / Noita Together) geschlossen sind.").color(Color32::from_rgb(220, 160, 60)));
        }
        ui.add_space(10.0);

        egui::Grid::new("meta").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut p.name);
            ui.end_row();
            ui.label("Typ");
            egui::ComboBox::from_id_salt("kind").selected_text(p.kind.label()).show_ui(ui, |ui| {
                for k in Kind::ALL {
                    ui.selectable_value(&mut p.kind, k, k.label());
                }
            });
            ui.end_row();
            ui.label("Geschützt");
            ui.checkbox(&mut p.protected, "kann nicht gelöscht werden");
            ui.end_row();
            ui.label("Notiz");
            ui.add(egui::TextEdit::multiline(&mut p.note).desired_rows(2).hint_text("z.B. mit wem, welcher Seed, wie weit …"));
            ui.end_row();
            ui.label("Erstellt");
            ui.label(&p.created);
            ui.end_row();
            ui.label("Zuletzt aktiviert");
            ui.label(p.last_used.clone().unwrap_or_else(|| "–".into()));
            ui.end_row();
            ui.label("Gespeicherte Kopie");
            ui.label(format!("{} Dateien, {:.1} MB", self.sel_info.0, self.sel_info.1 as f64 / 1_048_576.0));
            ui.end_row();
            if p.kind == Kind::EntangledWorlds {
                ui.label("EW save_state");
                ui.label(if self.sel_info.2 { "vorhanden (Run wird fortgesetzt)" } else { "keiner (neuer EW-Run)" });
                ui.end_row();
            }
        });

        // `edit` is the buffer; compare against the stored profile.
        self.edit = Some(p.clone());
        let stored = self.profiles.iter().find(|x| x.id == p.id).cloned();
        let changed = stored.is_some_and(|o| o.name != p.name || o.kind != p.kind || o.note != p.note || o.protected != p.protected);
        if changed {
            ui.horizontal(|ui| {
                if ui.button("Änderungen speichern").clicked() && !p.name.trim().is_empty() {
                    match saves::save_profile(&self.paths, &p) {
                        Ok(()) => self.info(format!("Profil \"{}\" gespeichert", p.name)),
                        Err(e) => self.info(format!("FEHLER: {e:#}")),
                    }
                    self.reload();
                }
                if ui.button("Verwerfen").clicked() {
                    self.load_selection();
                }
            });
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Ordner öffnen").clicked() {
                sys::open_folder(&self.paths.profile_dir(&p.id));
            }
            ui.add_enabled_ui(!locked, |ui| {
                if ui.button("Duplizieren").clicked() {
                    let id = p.id.clone();
                    self.job("Duplizieren", true, move |p, st| Ok(format!("Kopie \"{}\" angelegt", saves::duplicate_profile(p, st, &id)?.name)));
                }
                if ui.add_enabled(!p.protected && !active, egui::Button::new("Löschen")).on_disabled_hover_text("Geschützte oder aktive Profile können nicht gelöscht werden").clicked() {
                    self.dialog = Some(Dialog::ConfirmDelete(p.id.clone()));
                }
            });
        });

        if p.kind == Kind::EntangledWorlds {
            ui.add_space(10.0);
            self.ew_connect(ui, &p, locked);
        }

        ui.add_space(10.0);
        let snaps = self.sel_info.3.clone();
        egui::CollapsingHeader::new(format!("Automatische Snapshots ({})", snaps.len())).show(ui, |ui| {
            ui.label(RichText::new("Vor jedem Sichern wird die alte Kopie hier aufbewahrt.").small());
            egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                for s in &snaps {
                    ui.horizontal(|ui| {
                        ui.monospace(s);
                        if ui.add_enabled(!locked, egui::Button::new("Wiederherstellen").small()).clicked() {
                            self.dialog = Some(Dialog::ConfirmRestore(p.id.clone(), s.clone()));
                        }
                    });
                }
            });
            if ui.button("Snapshot-Ordner öffnen").clicked() {
                let d = self.paths.snapshots(&p.id);
                let _ = std::fs::create_dir_all(&d);
                sys::open_folder(&d);
            }
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let mut keep = true;
        let center = egui::Align2::CENTER_CENTER;
        match &mut dialog {
            Dialog::Import { name, kind, protected } => {
                egui::Window::new("Aktuellen Spielstand übernehmen").anchor(center, [0.0, 0.0]).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label("Der Spielstand, der gerade in Noita liegt, wird als Profil gespeichert.\nAn deinem Spielstand ändert sich dabei nichts.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(name);
                    });
                    kind_picker(ui, kind);
                    ui.checkbox(protected, "Schützen (kann nicht gelöscht werden)");
                    if *kind == Kind::EntangledWorlds {
                        ui.label(RichText::new("Der Entangled-Worlds-Zustand (%APPDATA%\\quant\\entangledworlds\\data\\save_state) wird mitgesichert.").small());
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Übernehmen").clicked() && !name.trim().is_empty() {
                            let (n, k, pr) = (name.clone(), *kind, *protected);
                            self.job("Übernehmen", true, move |p, st| {
                                let prof = saves::import_live(p, st, &n, k, pr)?;
                                Ok(format!("Profil \"{}\" angelegt und aktiv", prof.name))
                            });
                            keep = false;
                        }
                        if ui.button("Später").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::New { name, kind, keep_progress } => {
                egui::Window::new("Neues Profil").anchor(center, [0.0, 0.0]).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label("Startet einen frischen Run. Mod-Liste wird vom aktuellen Spielstand übernommen,\ndie Multiplayer-Mods passend zum Typ an/aus geschaltet.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(name);
                    });
                    kind_picker(ui, kind);
                    ui.checkbox(keep_progress, "Freischaltungen (persistent/) übernehmen");
                    ui.horizontal(|ui| {
                        if ui.button("Anlegen").clicked() && !name.trim().is_empty() {
                            let (n, k, kp) = (name.clone(), *kind, *keep_progress);
                            self.job("Anlegen", false, move |p, _| {
                                let template = p.live_save.clone();
                                let prof = saves::create_profile(p, &n, k, kp, template.exists().then_some(template.as_path()))?;
                                Ok(format!("Profil \"{}\" angelegt", prof.name))
                            });
                            keep = false;
                        }
                        if ui.button("Abbrechen").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::ConfirmDelete(id) => {
                let name = self.profiles.iter().find(|p| &p.id == id).map(|p| p.name.clone()).unwrap_or_default();
                egui::Window::new("Profil löschen?").anchor(center, [0.0, 0.0]).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(format!("\"{name}\" löschen?\nEs wird nach backups\\_deleted verschoben, nicht endgültig gelöscht."));
                    ui.horizontal(|ui| {
                        if ui.button("Löschen").clicked() {
                            let id = id.clone();
                            self.job("Löschen", false, move |p, st| saves::delete_profile(p, st, &id).map(|_| format!("\"{name}\" gelöscht (in backups\\_deleted)")));
                            keep = false;
                        }
                        if ui.button("Abbrechen").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::ConfirmRestore(id, snap) => {
                egui::Window::new("Snapshot wiederherstellen?").anchor(center, [0.0, 0.0]).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(format!("Profil auf den Stand {snap} zurücksetzen?\nDer jetzige Stand wird vorher als eigener Snapshot gesichert."));
                    ui.horizontal(|ui| {
                        if ui.button("Wiederherstellen").clicked() {
                            let (id, snap) = (id.clone(), snap.clone());
                            self.job("Wiederherstellen", true, move |p, st| saves::restore_snapshot(p, st, &id, &snap).map(|_| format!("Snapshot {snap} wiederhergestellt")));
                            keep = false;
                        }
                        if ui.button("Abbrechen").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::ConfirmForeign { target, play, ew_args } => {
                egui::Window::new("Unbekannter Spielstand").anchor(center, [0.0, 0.0]).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label("Der aktuelle Noita-Spielstand gehört zu keinem Profil.\nEmpfohlen: erst als Profil übernehmen.");
                    ui.horizontal(|ui| {
                        if ui.button("Erst übernehmen").clicked() {
                            self.open_import_dialog();
                            return;
                        }
                        if ui.button("Wegpacken & wechseln").on_hover_text("Wird nach backups\\_unassigned verschoben").clicked() {
                            let (t, pl, a) = (target.clone(), *play, ew_args.clone());
                            self.do_switch(t, pl, a);
                            keep = false;
                        }
                        if ui.button("Abbrechen").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::Settings(s) => {
                egui::Window::new("Einstellungen").anchor(center, [0.0, 0.0]).collapsible(false).resizable(false).show(ctx, |ui| {
                    egui::Grid::new("settings").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        for (label, val) in [
                            ("noita.exe", &mut s.noita_exe),
                            ("Entangled Worlds noita_proxy.exe", &mut s.ew_proxy_exe),
                            ("Noita Together.exe", &mut s.nt_exe),
                            ("Steam-Ordner", &mut s.steam_dir),
                        ] {
                            ui.label(label);
                            ui.add(egui::TextEdit::singleline(val).desired_width(420.0));
                            ui.end_row();
                        }
                        ui.label("Snapshots pro Profil");
                        ui.add(egui::DragValue::new(&mut s.keep_snapshots).range(1..=50));
                        ui.end_row();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Design:");
                        ui.selectable_value(&mut s.theme, UiTheme::Modern, "Modern");
                        ui.selectable_value(&mut s.theme, UiTheme::Classic, "Classic");
                    });
                    ui.checkbox(&mut s.auto_save_on_exit, "Nach Spielende automatisch ins Profil sichern");
                    ui.checkbox(&mut s.close_steam_for_singleplayer, "Singleplayer: Steam vorher schließen und Noita ohne Steam starten");
                    ui.label(RichText::new("Entangled Worlds braucht Steam (Lobby/Netzwerk) – dort wird Steam bei Bedarf gestartet.").small());
                    ui.checkbox(&mut s.check_updates, "Beim Start auf GitHub nach Updates suchen");
                    ui.label(RichText::new(format!("Version: {}", update::VERSION)).small());
                    ui.label(RichText::new(format!("Daten-Ordner: {}", self.paths.root.display())).small());
                    ui.label(RichText::new(format!("Live-Save: {}", self.paths.live_save.display())).small());
                    ui.horizontal(|ui| {
                        let idle = self.busy.is_none();
                        if ui
                            .add_enabled(idle, egui::Button::new("Pfade automatisch suchen"))
                            .on_hover_text("Sucht Steam, Noita, den EW-Proxy und Noita Together neu.\nGefundene Pfade ersetzen die Einträge oben.")
                            .clicked()
                        {
                            self.state.settings = s.clone();
                            self.search_paths(true);
                            keep = false;
                        }
                        if ui.add_enabled(self.update_rx.is_none(), egui::Button::new("Nach Updates suchen")).clicked() {
                            self.check_update(true);
                            keep = false;
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Speichern").clicked() {
                            self.state.settings = s.clone();
                            match saves::save_state(&self.paths, &self.state) {
                                Ok(()) => self.info("Einstellungen gespeichert"),
                                Err(e) => self.info(format!("FEHLER: {e:#}")),
                            }
                            self.reload();
                            keep = false;
                        }
                        if ui.button("Abbrechen").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::Update(r) => {
                egui::Window::new(format!("Update auf Version {}", r.version)).anchor(center, [0.0, 0.0]).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(format!("Installiert: {}, neu: {}", update::VERSION, r.version));
                    if !r.notes.is_empty() {
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            ui.label(RichText::new(&r.notes).small());
                        });
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new("Profile, Backups und Einstellungen bleiben unverändert. Die App startet danach neu.").small());
                    let blocked = self.game_running() || self.busy.is_some();
                    if self.game_running() {
                        ui.label(RichText::new("Erst Noita / Proxy / Noita Together schließen.").color(Color32::from_rgb(220, 160, 60)));
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!blocked, egui::Button::new("Jetzt aktualisieren")).clicked() {
                            self.install_update(r.clone());
                            keep = false;
                        }
                        if !r.page.is_empty() && ui.button("Release-Seite").clicked() {
                            ctx.open_url(egui::OpenUrl::new_tab(&r.page));
                        }
                        if ui.button("Später").clicked() {
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

fn kind_picker(ui: &mut egui::Ui, kind: &mut Kind) {
    ui.horizontal(|ui| {
        ui.label("Typ:");
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
                    bail!("Steam hat sich nicht innerhalb von 30 s beendet");
                }
            }
            sys::launch(&s.noita_exe)?;
            Ok("Noita gestartet".into())
        }
        Kind::EntangledWorlds => {
            if !sys::steam_running(&sys::Procs::new().refresh()) {
                sys::start_steam(&s.steam_dir)?;
                if !wait_steam(true) {
                    bail!("Steam konnte nicht gestartet werden (wird für Entangled Worlds gebraucht)");
                }
                std::thread::sleep(Duration::from_secs(8));
            }
            if ew_args.is_empty() {
                sys::launch_with(&s.ew_proxy_exe, ew_args)?;
                return Ok("Entangled Worlds Proxy gestartet (Lobby erstellen/beitreten, dann startet Noita)".into());
            }
            // Direct IP via the proxy's CLI: it runs without a window and does not start Noita itself.
            sys::launch_ew_pinned(&s.ew_proxy_exe, ew_args)?;
            std::thread::sleep(Duration::from_secs(4));
            if !sys::running_games(&sys::Procs::new().refresh()).iter().any(|n| n == "noita.exe") {
                sys::launch_ew_pinned(&s.noita_exe, &[])?;
            }
            let target = ew_args.get(1).map(String::as_str).unwrap_or("");
            Ok(if ew_args[0] == "--host" {
                format!("EW Proxy hostet per IP auf Port {target} (ohne Proxy-Fenster), Noita gestartet")
            } else {
                format!("EW Proxy verbindet zu {target} (ohne Proxy-Fenster), Noita gestartet")
            })
        }
        Kind::NoitaTogether => {
            sys::launch(&s.nt_exe)?;
            sys::launch(&s.noita_exe)?;
            Ok("Noita Together + Noita gestartet".into())
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job();
        self.poll_update();
        self.poll_system();
        ctx.request_repaint_after(Duration::from_millis(if self.busy.is_some() { 100 } else { 1000 }));

        let theme = self.state.settings.theme;
        if self.applied_theme != Some(theme) {
            theme::apply(ctx, theme);
            self.applied_theme = Some(theme);
        }
        if theme == UiTheme::Modern {
            self.ui_modern(ctx);
            self.dialogs(ctx);
            return;
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui);
            ui.add_space(2.0);
            self.live_banner(ui);
            ui.add_space(4.0);
        });
        egui::TopBottomPanel::bottom("log").resizable(true).default_height(90.0).show(ctx, |ui| {
            if let Some((label, _)) = &self.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("{label} …"));
                });
            }
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for l in &self.log {
                    let color = if l.contains("FEHLER") { Color32::LIGHT_RED } else { Color32::GRAY };
                    ui.label(RichText::new(l).color(color).monospace());
                }
            });
        });
        egui::SidePanel::left("list").resizable(true).default_width(300.0).show(ctx, |ui| {
            ui.add_space(4.0);
            self.profile_list(ui);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.view == View::Logs {
                    self.logs_view(ui);
                } else {
                    self.details(ui);
                }
            });
        });
        self.dialogs(ctx);
    }
}
