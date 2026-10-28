//! The main window layout.

use super::*;
use crate::theme::*;
use eframe::egui::{Align, CursorIcon, Frame, Id, Layout, Margin, Rect, Rounding, Sense, Stroke, Ui, vec2};

enum EditAction {
    None,
    Save,
    Discard,
}

fn card() -> Frame {
    Frame::none().fill(SLOT).stroke(Stroke::new(1.0_f32, EDGE)).rounding(5.0).inner_margin(Margin::same(16.0))
}

impl App {
    pub(super) fn ui_modern(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("m_top")
            .frame(Frame::none().fill(CAVE).inner_margin(Margin { left: 20.0, right: 20.0, top: 14.0, bottom: 12.0 }))
            .show_separator_line(false)
            .show(ctx, |ui| self.m_header(ui));
        egui::TopBottomPanel::bottom("m_log")
            .frame(Frame::none().fill(CAVE).inner_margin(Margin { left: 20.0, right: 20.0, top: 8.0, bottom: 8.0 }))
            .resizable(true)
            .default_height(80.0)
            .show(ctx, |ui| self.m_log(ui));
        egui::SidePanel::left("m_list")
            .frame(Frame::none().fill(CAVE).inner_margin(Margin { left: 20.0, right: 14.0, top: 6.0, bottom: 14.0 }))
            .resizable(true)
            .default_width(320.0)
            .width_range(260.0..=460.0)
            .show(ctx, |ui| self.m_list(ui));
        egui::CentralPanel::default()
            .frame(Frame::none().fill(CAVE).inner_margin(Margin { left: 18.0, right: 20.0, top: 6.0, bottom: 14.0 }))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if self.view == View::Logs {
                        ui.add_space(8.0);
                        self.logs_view(ui);
                    } else {
                        self.m_details(ui);
                    }
                });
            });
    }

    // ------------------------------------------------------------ header

    fn m_header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(tracked("Noita", 17.0, INK, 3.5));
            ui.label(tracked("Save Selector", 17.0, INK_MUTED, 3.5));
            ui.add_space(18.0);
            for (v, label) in [(View::Profiles, "Profiles"), (View::Logs, "Logs")] {
                let on = self.view == v;
                let text = tracked(label, 12.0, if on { INK } else { INK_MUTED }, 1.6);
                if ui.add(egui::SelectableLabel::new(on, text)).clicked() {
                    self.view = v;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Settings").clicked() {
                    self.dialog = Some(Dialog::Settings(self.state.settings.clone()));
                }
                if let Some(r) = &self.update
                    && ui
                        .button(RichText::new(format!("Update {}", r.version)).color(ALIVE))
                        .on_hover_text(format!("Installed: {}", update::VERSION))
                        .clicked()
                {
                    self.dialog = Some(Dialog::Update(r.clone()));
                }
            });
        });
        ui.add_space(10.0);

        Frame::none().fill(SLOT).stroke(Stroke::new(1.0_f32, EDGE)).rounding(4.0).inner_margin(Margin::symmetric(14.0, 8.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if self.game_running() {
                    status(ui, "Game", &format!("running ({})", self.running.join(", ")), WARN);
                } else {
                    status(ui, "Game", "closed", ALIVE);
                }
                ui.add_space(14.0);
                status(ui, "Steam", if self.steam_on { "running" } else { "off" }, if self.steam_on { STEAM } else { INK_MUTED });
                let label = if self.steam_on { "Close" } else { "Start" };
                if ui.small_button(label).on_hover_text("Close or start Steam cleanly").clicked() {
                    self.toggle_steam();
                }
                ui.add_space(14.0);
                match self.cloud {
                    Some(false) => status(ui, "Cloud", "off", ALIVE),
                    Some(true) => {
                        status(ui, "Cloud", "ON", DANGER);
                        ui.label(RichText::new("turn off?").color(DANGER).underline()).on_hover_text(
                            "Steam Cloud may report conflicts or overwrite saves when switching profiles.\n\
                             Steam > Library > Noita > Properties > General >\n\
                             untick 'Keep game saves in the Steam Cloud'.",
                        );
                    }
                    None => status(ui, "Cloud", "unknown", INK_MUTED),
                }
                ui.add_space(14.0);
                let (text, color) = match &self.live {
                    LiveStatus::Owned(id) => match self.profiles.iter().find(|p| &p.id == id) {
                        Some(_) if self.other_run => ("a different run".into(), WARN),
                        Some(p) => (p.name.clone(), kind_color(p.kind)),
                        None => ("(deleted profile)".into(), WARN),
                    },
                    LiveStatus::Missing => ("nothing – Noita starts fresh".into(), INK_MUTED),
                    LiveStatus::Foreign => ("unknown save".into(), WARN),
                };
                status(ui, "In game", &text, color);
                if let Some((net, color)) = self.net_status() {
                    ui.add_space(14.0);
                    status(ui, "Network", &net, color);
                }
                if self.proxy_orphaned() {
                    ui.add_space(8.0);
                    if ui.small_button("Stop proxy").on_hover_text("Noita is closed but the EW proxy is still running (e.g. windowless IP mode)").clicked() {
                        self.stop_proxy();
                    }
                }
            });
        });

        if self.other_run {
            ui.add_space(8.0);
            Frame::none()
                .fill(WARN.gamma_multiply(0.10))
                .stroke(Stroke::new(1.0_f32, WARN.gamma_multiply(0.45)))
                .rounding(4.0)
                .inner_margin(Margin::symmetric(14.0, 8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let name = self.active_name().unwrap_or_default();
                        ui.label(RichText::new(format!("Noita holds a different run than \"{name}\" (started directly in Noita?).")).color(WARN));
                        if ui.button("Import as new profile").clicked() {
                            self.open_new_run_dialog();
                        }
                    });
                });
        }
        if self.live == LiveStatus::Foreign {
            ui.add_space(8.0);
            Frame::none()
                .fill(WARN.gamma_multiply(0.10))
                .stroke(Stroke::new(1.0_f32, WARN.gamma_multiply(0.45)))
                .rounding(4.0)
                .inner_margin(Margin::symmetric(14.0, 8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("The current Noita save belongs to no profile.").color(WARN));
                        if ui.button("Import as profile").clicked() {
                            self.open_import_dialog();
                        }
                        if let Some(name) = self.active_name() {
                            if ui
                                .button(format!("Belongs to \"{name}\""))
                                .on_hover_text("Only choose this if it is definitely this profile's save\n(e.g. after a new run when the marker is missing).")
                                .clicked()
                            {
                                self.claim_live(&name);
                            }
                        }
                    });
                });
        }
    }

    // ------------------------------------------------------------ log

    fn m_log(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            caption(ui, "Log", INK_MUTED);
            if let Some((label, _)) = &self.busy {
                ui.add_space(8.0);
                ui.add(egui::Spinner::new().size(12.0).color(INK_2));
                ui.label(RichText::new(format!("{label} …")).color(INK_2).small());
            }
        });
        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if self.log.is_empty() {
                ui.label(RichText::new("Nothing happened yet.").monospace().size(11.5).color(INK_MUTED));
            }
            for l in &self.log {
                let color = if l.contains("ERROR") { DANGER } else { INK_MUTED };
                ui.label(RichText::new(l).monospace().size(11.5).color(color));
            }
        });
    }

    // ------------------------------------------------------------ profile list

    fn m_list(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(2.0);
            if ui.add_sized([ui.available_width(), 32.0], egui::Button::new("+  New profile")).clicked() {
                self.dialog = Some(Dialog::New { name: String::new(), kind: Kind::Singleplayer, keep_progress: true });
            }
            if self.live != LiveStatus::Missing
                && ui
                    .add_sized([ui.available_width(), 28.0], egui::Button::new("Import current run"))
                    .on_hover_text("Started a run directly in Noita (e.g. a daily)?\nStore the run that is in Noita now as its own profile.")
                    .clicked()
            {
                if self.other_run {
                    self.open_new_run_dialog();
                } else {
                    self.open_import_dialog();
                }
            }
            ui.add_space(6.0);
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let profiles = self.profiles.clone();
                    if profiles.is_empty() {
                        ui.add_space(24.0);
                        ui.label(RichText::new("No profiles yet.").color(INK_2));
                        ui.label(RichText::new("Import your current save or create a new profile.").small().color(INK_MUTED));
                        return;
                    }
                    let mut new_hover = None;
                    let mut clicked = None;
                    for (title, mp) in [("Multiplayer", true), ("Singleplayer", false)] {
                        let group: Vec<&Profile> = profiles.iter().filter(|p| p.kind.is_multiplayer() == mp).collect();
                        if group.is_empty() {
                            continue;
                        }
                        ui.add_space(8.0);
                        caption(ui, &format!("{title}  {}", group.len()), INK_MUTED);
                        ui.add_space(2.0);
                        for p in group {
                            let resp = self.m_card(ui, p);
                            if resp.hovered() {
                                new_hover = Some(p.id.clone());
                            }
                            if resp.clicked() {
                                clicked = Some(p.id.clone());
                            }
                            ui.add_space(2.0);
                        }
                    }
                    if new_hover != self.hover_card {
                        self.hover_card = new_hover;
                        ui.ctx().request_repaint();
                    }
                    if let Some(id) = clicked {
                        self.selected = Some(id);
                        self.load_selection();
                    }
                });
            });
        });
    }

    fn m_card(&self, ui: &mut Ui, p: &Profile) -> egui::Response {
        let active = self.state.active.as_deref() == Some(p.id.as_str());
        let selected = self.selected.as_deref() == Some(p.id.as_str());
        let hovered = self.hover_card.as_deref() == Some(p.id.as_str());
        let fill = if selected {
            SLOT_SELECTED
        } else if hovered {
            SLOT_HOVER
        } else {
            SLOT
        };
        let stroke = if selected { Stroke::new(1.0_f32, kind_color(p.kind).gamma_multiply(0.8)) } else { Stroke::new(1.0_f32, EDGE) };
        let inner = Frame::none().fill(fill).stroke(stroke).rounding(4.0).inner_margin(Margin::symmetric(10.0, 9.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                kind_tile(ui, p.kind, 34.0);
                ui.add_space(2.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(egui::Label::new(RichText::new(&p.name).size(14.5).strong().color(if active { ALIVE } else { INK })).truncate());
                    let last = p.last_used.clone().unwrap_or_else(|| "never".into());
                    ui.add(egui::Label::new(RichText::new(format!("last used {last}")).small().color(INK_MUTED)).truncate());
                });
            });
            if active || p.protected {
                ui.horizontal(|ui| {
                    ui.add_space(42.0);
                    if active {
                        tag(ui, "Active", ALIVE);
                    }
                    if p.protected {
                        tag(ui, "Protected", WARD);
                    }
                });
            }
        });
        let rect = inner.response.rect;
        if active {
            let bar = Rect::from_min_size(rect.min + vec2(0.0, 6.0), vec2(3.0, rect.height() - 12.0));
            ui.painter().rect_filled(bar, Rounding::same(1.5), ALIVE);
        }
        ui.interact(rect, Id::new(("card", &p.id)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand)
    }

    // ------------------------------------------------------------ details

    fn m_details(&mut self, ui: &mut Ui) {
        let Some(mut p) = self.edit.clone() else {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(tracked("No profile selected", 13.0, INK_2, 2.0));
                ui.label(RichText::new("Pick a profile on the left.").color(INK_MUTED));
            });
            return;
        };
        let active = self.state.active.as_deref() == Some(p.id.as_str());
        let busy = self.busy.is_some();
        let locked = busy || self.game_running();
        let color = kind_color(p.kind);
        ui.add_space(8.0);

        // Hero: the profile and its one big action.
        card().stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.45))).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                kind_tile(ui, p.kind, 60.0);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    caption(ui, p.kind.label(), color.linear_multiply(1.6));
                    ui.label(RichText::new(&p.name).size(24.0).strong().color(INK));
                    ui.horizontal(|ui| {
                        if active {
                            tag(ui, "Active", ALIVE);
                        }
                        if self.live == LiveStatus::Owned(p.id.clone()) && !self.other_run {
                            tag(ui, "Loaded in game", INK_2);
                        }
                        if p.protected {
                            tag(ui, "Protected", WARD);
                        }
                    });
                });
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if active {
                    let label = match p.kind {
                        Kind::EntangledWorlds => "Play  ·  EW proxy",
                        Kind::NoitaTogether => "Play  ·  Noita Together",
                        Kind::Singleplayer => "Play",
                    };
                    if cta(ui, label, color, !locked).clicked() {
                        self.play(p.kind);
                    }
                    if ui
                        .add_enabled(!locked, egui::Button::new("Save now").min_size(vec2(0.0, 32.0)))
                        .on_hover_text("Copy the live save into the profile (the old copy becomes a snapshot)")
                        .clicked()
                    {
                        self.sync_now();
                    }
                } else {
                    if cta(ui, "Activate & play", color, !locked).clicked() {
                        self.activate(p.id.clone(), true);
                    }
                    if ui.add_enabled(!locked, egui::Button::new("Activate only").min_size(vec2(0.0, 32.0))).clicked() {
                        self.activate(p.id.clone(), false);
                    }
                }
            });
            if self.game_running() && !busy {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    dot(ui, WARN);
                    ui.label(RichText::new("Switching and saving only work once Noita, the EW proxy and Noita Together are closed.").color(WARN));
                });
            }
        });
        ui.add_space(12.0);
        if p.kind == Kind::EntangledWorlds {
            self.ew_connect(ui, &p, locked);
            ui.add_space(12.0);
        }

        // Profile fields + save facts, side by side when there is room.
        let mut action = EditAction::None;
        if ui.available_width() > 700.0 {
            ui.columns(2, |cols| {
                action = self.m_profile_section(&mut cols[0], &mut p);
                self.m_save_section(&mut cols[1], &p);
            });
        } else {
            action = self.m_profile_section(ui, &mut p);
            ui.add_space(12.0);
            self.m_save_section(ui, &p);
        }
        ui.add_space(12.0);
        self.m_snapshots(ui, &p, locked);
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            if ui.button("Open folder").clicked() {
                sys::open_folder(&self.paths.profile_dir(&p.id));
            }
            if ui.add_enabled(!locked, egui::Button::new("Duplicate")).clicked() {
                self.duplicate(p.id.clone());
            }
            if ui
                .add_enabled(!locked && !p.protected && !active, danger_button("Delete"))
                .on_disabled_hover_text("Protected or active profiles cannot be deleted")
                .clicked()
            {
                self.dialog = Some(Dialog::ConfirmDelete(p.id.clone()));
            }
        });

        match action {
            EditAction::Save => self.save_edit(&p),
            EditAction::Discard => self.load_selection(),
            EditAction::None => self.edit = Some(p),
        }
    }

    fn m_profile_section(&self, ui: &mut Ui, p: &mut Profile) -> EditAction {
        let mut action = EditAction::None;
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            caption(ui, "Profile", INK_MUTED);
            ui.add_space(6.0);
            egui::Grid::new("m_meta").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                caption(ui, "Name", INK_2);
                ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(f32::INFINITY));
                ui.end_row();
                caption(ui, "Type", INK_2);
                egui::ComboBox::from_id_salt("m_kind").width(180.0).selected_text(RichText::new(p.kind.label()).color(INK)).show_ui(ui, |ui| {
                    for k in Kind::ALL {
                        ui.selectable_value(&mut p.kind, k, k.label());
                    }
                });
                ui.end_row();
                caption(ui, "Protection", INK_2);
                ui.checkbox(&mut p.protected, "cannot be deleted");
                ui.end_row();
                caption(ui, "Note", INK_2);
                ui.add(egui::TextEdit::multiline(&mut p.note).desired_rows(2).desired_width(f32::INFINITY).hint_text("Who with, which seed, how far …"));
                ui.end_row();
            });
            let stored = self.profiles.iter().find(|x| x.id == p.id);
            let changed = stored.is_some_and(|o| o.name != p.name || o.kind != p.kind || o.note != p.note || o.protected != p.protected);
            if changed {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    tag(ui, "Unsaved", WARN);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Discard").clicked() {
                            action = EditAction::Discard;
                        }
                        if ui.button(RichText::new("Save").strong()).clicked() {
                            action = EditAction::Save;
                        }
                    });
                });
            }
        });
        action
    }

    fn m_save_section(&self, ui: &mut Ui, p: &Profile) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            caption(ui, "Save", INK_MUTED);
            ui.add_space(6.0);
            egui::Grid::new("m_facts").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                let mono = |t: String| RichText::new(t).monospace().color(INK);
                caption(ui, "Created", INK_2);
                ui.label(mono(p.created.clone()));
                ui.end_row();
                caption(ui, "Activated", INK_2);
                ui.label(mono(p.last_used.clone().unwrap_or_else(|| "–".into())));
                ui.end_row();
                caption(ui, "Copy", INK_2);
                ui.label(mono(format!("{:.1} MB  ·  {} files", self.sel_info.1 as f64 / 1_048_576.0, self.sel_info.0)));
                ui.end_row();
                if p.kind == Kind::EntangledWorlds {
                    caption(ui, "EW run", INK_2);
                    if self.sel_info.2 {
                        ui.label(RichText::new("save_state stored – run continues").color(ALIVE));
                    } else {
                        ui.label(RichText::new("no save_state – new EW run").color(INK_2));
                    }
                    ui.end_row();
                }
            });
        });
    }

    fn m_snapshots(&mut self, ui: &mut Ui, p: &Profile, locked: bool) {
        let snaps = self.sel_info.3.clone();
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                caption(ui, &format!("Snapshots  {}", snaps.len()), INK_MUTED);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Open folder").clicked() {
                        let d = self.paths.snapshots(&p.id);
                        let _ = std::fs::create_dir_all(&d);
                        sys::open_folder(&d);
                    }
                });
            });
            ui.add_space(4.0);
            if snaps.is_empty() {
                ui.label(RichText::new("None yet. Before every save the old copy is kept here.").color(INK_MUTED));
                return;
            }
            egui::ScrollArea::vertical().max_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
                for (i, s) in snaps.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(pretty_stamp(s)).monospace().color(if i == 0 { INK } else { INK_2 }));
                        if i == 0 {
                            tag(ui, "Newest", INK_2);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add_enabled(!locked, egui::Button::new("Restore").small()).clicked() {
                                self.dialog = Some(Dialog::ConfirmRestore(p.id.clone(), s.clone()));
                            }
                        });
                    });
                }
            });
        });
    }
}
