//! The "Modern" layout. Same actions as Classic, different presentation.

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
            for (v, label) in [(View::Profiles, "Profile"), (View::Logs, "Logs")] {
                let on = self.view == v;
                let text = tracked(label, 12.0, if on { INK } else { INK_MUTED }, 1.6);
                if ui.add(egui::SelectableLabel::new(on, text)).clicked() {
                    self.view = v;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Einstellungen").clicked() {
                    self.dialog = Some(Dialog::Settings(self.state.settings.clone()));
                }
                if ui.button("Classic").on_hover_text("Zum ursprünglichen, schlichten Design wechseln").clicked() {
                    self.set_theme(UiTheme::Classic);
                }
                if let Some(r) = &self.update
                    && ui
                        .button(RichText::new(format!("Update {}", r.version)).color(ALIVE))
                        .on_hover_text(format!("Installiert: {}", update::VERSION))
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
                    status(ui, "Spiel", &format!("läuft ({})", self.running.join(", ")), WARN);
                } else {
                    status(ui, "Spiel", "geschlossen", ALIVE);
                }
                ui.add_space(14.0);
                status(ui, "Steam", if self.steam_on { "läuft" } else { "aus" }, if self.steam_on { STEAM } else { INK_MUTED });
                let label = if self.steam_on { "Schließen" } else { "Starten" };
                if ui.small_button(label).on_hover_text("Steam sauber beenden bzw. starten").clicked() {
                    self.toggle_steam();
                }
                ui.add_space(14.0);
                match self.cloud {
                    Some(false) => status(ui, "Cloud", "aus", ALIVE),
                    Some(true) => {
                        status(ui, "Cloud", "AN", DANGER);
                        ui.label(RichText::new("ausschalten?").color(DANGER).underline()).on_hover_text(
                            "Steam Cloud kann beim Profilwechsel Konflikte melden oder Saves überschreiben.\n\
                             Steam > Bibliothek > Noita > Eigenschaften > Allgemein >\n\
                             'Spielstände in der Steam Cloud speichern' deaktivieren.",
                        );
                    }
                    None => status(ui, "Cloud", "unbekannt", INK_MUTED),
                }
                ui.add_space(14.0);
                let (text, color) = match &self.live {
                    LiveStatus::Owned(id) => match self.profiles.iter().find(|p| &p.id == id) {
                        Some(p) => (p.name.clone(), kind_color(p.kind)),
                        None => ("(gelöschtes Profil)".into(), WARN),
                    },
                    LiveStatus::Missing => ("nichts – Noita startet neu".into(), INK_MUTED),
                    LiveStatus::Foreign => ("unbekannter Spielstand".into(), WARN),
                };
                status(ui, "Im Spiel", &text, color);
                if let Some((net, color)) = self.net_status() {
                    ui.add_space(14.0);
                    status(ui, "Netz", &net, color);
                }
                if self.proxy_orphaned() {
                    ui.add_space(8.0);
                    if ui.small_button("Proxy beenden").on_hover_text("Noita ist zu, der EW Proxy läuft noch (z.B. im IP-Modus ohne Fenster)").clicked() {
                        self.stop_proxy();
                    }
                }
            });
        });

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
                        ui.label(RichText::new("Der aktuelle Noita-Spielstand gehört zu keinem Profil.").color(WARN));
                        if ui.button("Als Profil übernehmen").clicked() {
                            self.open_import_dialog();
                        }
                        if let Some(name) = self.active_name() {
                            if ui
                                .button(format!("Gehört zu \"{name}\""))
                                .on_hover_text("Nur wählen, wenn das sicher der Spielstand dieses Profils ist\n(z.B. nach einem neuen Run, wenn die Markierung fehlt).")
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
            caption(ui, "Protokoll", INK_MUTED);
            if let Some((label, _)) = &self.busy {
                ui.add_space(8.0);
                ui.add(egui::Spinner::new().size(12.0).color(INK_2));
                ui.label(RichText::new(format!("{label} …")).color(INK_2).small());
            }
        });
        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if self.log.is_empty() {
                ui.label(RichText::new("Noch nichts passiert.").monospace().size(11.5).color(INK_MUTED));
            }
            for l in &self.log {
                let color = if l.contains("FEHLER") { DANGER } else { INK_MUTED };
                ui.label(RichText::new(l).monospace().size(11.5).color(color));
            }
        });
    }

    // ------------------------------------------------------------ profile list

    fn m_list(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(2.0);
            if ui.add_sized([ui.available_width(), 32.0], egui::Button::new("+  Neues Profil")).clicked() {
                self.dialog = Some(Dialog::New { name: String::new(), kind: Kind::Singleplayer, keep_progress: true });
            }
            if self.live == LiveStatus::Foreign && ui.add_sized([ui.available_width(), 28.0], egui::Button::new("Aktuellen Save übernehmen")).clicked() {
                self.open_import_dialog();
            }
            ui.add_space(6.0);
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let profiles = self.profiles.clone();
                    if profiles.is_empty() {
                        ui.add_space(24.0);
                        ui.label(RichText::new("Noch keine Profile.").color(INK_2));
                        ui.label(RichText::new("Übernimm deinen aktuellen Spielstand oder lege ein neues Profil an.").small().color(INK_MUTED));
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
                    let last = p.last_used.clone().unwrap_or_else(|| "nie".into());
                    ui.add(egui::Label::new(RichText::new(format!("zuletzt {last}")).small().color(INK_MUTED)).truncate());
                });
            });
            if active || p.protected {
                ui.horizontal(|ui| {
                    ui.add_space(42.0);
                    if active {
                        tag(ui, "Aktiv", ALIVE);
                    }
                    if p.protected {
                        tag(ui, "Geschützt", WARD);
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
                ui.label(tracked("Kein Profil ausgewählt", 13.0, INK_2, 2.0));
                ui.label(RichText::new("Links ein Profil wählen.").color(INK_MUTED));
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
                            tag(ui, "Aktiv", ALIVE);
                        }
                        if self.live == LiveStatus::Owned(p.id.clone()) {
                            tag(ui, "Im Spiel geladen", INK_2);
                        }
                        if p.protected {
                            tag(ui, "Geschützt", WARD);
                        }
                    });
                });
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if active {
                    let label = match p.kind {
                        Kind::EntangledWorlds => "Spielen  ·  EW Proxy",
                        Kind::NoitaTogether => "Spielen  ·  Noita Together",
                        Kind::Singleplayer => "Spielen",
                    };
                    if cta(ui, label, color, !locked).clicked() {
                        self.play(p.kind);
                    }
                    if ui
                        .add_enabled(!locked, egui::Button::new("Jetzt sichern").min_size(vec2(0.0, 32.0)))
                        .on_hover_text("Live-Spielstand ins Profil kopieren (alte Kopie wird Snapshot)")
                        .clicked()
                    {
                        self.sync_now();
                    }
                } else {
                    if cta(ui, "Aktivieren & Spielen", color, !locked).clicked() {
                        self.activate(p.id.clone(), true);
                    }
                    if ui.add_enabled(!locked, egui::Button::new("Nur aktivieren").min_size(vec2(0.0, 32.0))).clicked() {
                        self.activate(p.id.clone(), false);
                    }
                }
            });
            if self.game_running() && !busy {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    dot(ui, WARN);
                    ui.label(RichText::new("Wechseln und Sichern geht erst, wenn Noita, EW-Proxy und Noita Together geschlossen sind.").color(WARN));
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
            if ui.button("Ordner öffnen").clicked() {
                sys::open_folder(&self.paths.profile_dir(&p.id));
            }
            if ui.add_enabled(!locked, egui::Button::new("Duplizieren")).clicked() {
                self.duplicate(p.id.clone());
            }
            if ui
                .add_enabled(!locked && !p.protected && !active, danger_button("Löschen"))
                .on_disabled_hover_text("Geschützte oder aktive Profile können nicht gelöscht werden")
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
            caption(ui, "Profil", INK_MUTED);
            ui.add_space(6.0);
            egui::Grid::new("m_meta").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                caption(ui, "Name", INK_2);
                ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(f32::INFINITY));
                ui.end_row();
                caption(ui, "Typ", INK_2);
                egui::ComboBox::from_id_salt("m_kind").width(180.0).selected_text(RichText::new(p.kind.label()).color(INK)).show_ui(ui, |ui| {
                    for k in Kind::ALL {
                        ui.selectable_value(&mut p.kind, k, k.label());
                    }
                });
                ui.end_row();
                caption(ui, "Schutz", INK_2);
                ui.checkbox(&mut p.protected, "nicht löschbar");
                ui.end_row();
                caption(ui, "Notiz", INK_2);
                ui.add(egui::TextEdit::multiline(&mut p.note).desired_rows(2).desired_width(f32::INFINITY).hint_text("Mit wem, welcher Seed, wie weit …"));
                ui.end_row();
            });
            let stored = self.profiles.iter().find(|x| x.id == p.id);
            let changed = stored.is_some_and(|o| o.name != p.name || o.kind != p.kind || o.note != p.note || o.protected != p.protected);
            if changed {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    tag(ui, "Ungespeichert", WARN);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Verwerfen").clicked() {
                            action = EditAction::Discard;
                        }
                        if ui.button(RichText::new("Speichern").strong()).clicked() {
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
            caption(ui, "Spielstand", INK_MUTED);
            ui.add_space(6.0);
            egui::Grid::new("m_facts").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                let mono = |t: String| RichText::new(t).monospace().color(INK);
                caption(ui, "Erstellt", INK_2);
                ui.label(mono(p.created.clone()));
                ui.end_row();
                caption(ui, "Aktiviert", INK_2);
                ui.label(mono(p.last_used.clone().unwrap_or_else(|| "–".into())));
                ui.end_row();
                caption(ui, "Kopie", INK_2);
                ui.label(mono(format!("{:.1} MB  ·  {} Dateien", self.sel_info.1 as f64 / 1_048_576.0, self.sel_info.0)));
                ui.end_row();
                if p.kind == Kind::EntangledWorlds {
                    caption(ui, "EW-Run", INK_2);
                    if self.sel_info.2 {
                        ui.label(RichText::new("save_state gesichert – Run geht weiter").color(ALIVE));
                    } else {
                        ui.label(RichText::new("kein save_state – neuer EW-Run").color(INK_2));
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
                    if ui.small_button("Ordner öffnen").clicked() {
                        let d = self.paths.snapshots(&p.id);
                        let _ = std::fs::create_dir_all(&d);
                        sys::open_folder(&d);
                    }
                });
            });
            ui.add_space(4.0);
            if snaps.is_empty() {
                ui.label(RichText::new("Noch keine. Vor jedem Sichern wird die alte Kopie hier aufbewahrt.").color(INK_MUTED));
                return;
            }
            egui::ScrollArea::vertical().max_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
                for (i, s) in snaps.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(pretty_stamp(s)).monospace().color(if i == 0 { INK } else { INK_2 }));
                        if i == 0 {
                            tag(ui, "Neuester", INK_2);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add_enabled(!locked, egui::Button::new("Wiederherstellen").small()).clicked() {
                                self.dialog = Some(Dialog::ConfirmRestore(p.id.clone(), s.clone()));
                            }
                        });
                    });
                }
            });
        });
    }
}
