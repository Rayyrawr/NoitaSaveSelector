//! Entangled Worlds connection panel, network status and the log viewer. Used by both themes.

use super::*;
use crate::ewlog::{Level, LogKind, NetState};
use crate::theme::*;
use eframe::egui::{Align, Layout, Margin, Stroke, Ui, vec2};

fn panel() -> egui::Frame {
    egui::Frame::none().fill(SLOT).stroke(Stroke::new(1.0_f32, EDGE)).rounding(5.0).inner_margin(Margin::same(16.0))
}

impl App {
    /// (label, color) for the status bars while the EW proxy runs.
    pub(super) fn net_status(&self) -> Option<(String, Color32)> {
        let net = self.net.as_ref()?;
        let st = net.state();
        let color = match st {
            NetState::Waiting => INK_2,
            NetState::Good => ALIVE,
            NetState::Shaky => WARN,
            NetState::Kicked => DANGER,
        };
        let mut text = st.label().to_string();
        if net.peers() > 0 {
            text.push_str(&format!(" · {} player(s)", net.peers()));
        }
        if net.incidents > 0 {
            text.push_str(&format!(" · {} incident(s)", net.incidents));
        }
        Some((text, color))
    }

    /// Proxy still running after Noita closed (e.g. windowless IP mode).
    pub(super) fn proxy_orphaned(&self) -> bool {
        self.running.iter().any(|n| n == "noita_proxy.exe") && !self.running.iter().any(|n| n == "noita.exe")
    }

    pub(super) fn stop_proxy(&mut self) {
        let n = sys::kill_proxy();
        self.info(if n > 0 { "EW proxy stopped".to_string() } else { "No EW proxy found".to_string() });
        self.last_poll = Instant::now() - Duration::from_secs(10);
    }

    // ------------------------------------------------------------ connection panel

    pub(super) fn ew_connect(&mut self, ui: &mut Ui, p: &Profile, locked: bool) {
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                caption(ui, "Connection", INK_MUTED);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let before = self.state.settings.ew_direct_ip;
                    ui.selectable_value(&mut self.state.settings.ew_direct_ip, true, "Direct IP (Tailscale)");
                    ui.selectable_value(&mut self.state.settings.ew_direct_ip, false, "Steam");
                    if before != self.state.settings.ew_direct_ip {
                        self.save_settings();
                    }
                });
            });
            ui.add_space(6.0);

            if !self.state.settings.ew_direct_ip {
                ui.label(RichText::new("Default and, according to the EW developers, the most reliable. Create a lobby in the proxy window, copy the code and send it to your friend.").color(INK_2));
                ui.add_space(4.0);
                ui.label(RichText::new("Just use 'Play' / 'Activate & play' above.").small().color(INK_MUTED));
                return;
            }

            // Direct IP
            let port = self.state.settings.ew_port;
            egui::Grid::new("ew_ip").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                caption(ui, "Your IP", INK_2);
                ui.horizontal(|ui| match self.ts_ip.clone() {
                    Some(ip) => {
                        let addr = format!("{ip}:{port}");
                        ui.label(RichText::new(&addr).monospace().color(INK));
                        if ui.small_button("Copy").on_hover_text("Your friend enters this address in the proxy under 'Connect by IP'").clicked() {
                            ui.ctx().copy_text(addr.clone());
                            self.info(format!("{addr} copied to the clipboard"));
                        }
                    }
                    None => {
                        ui.label(RichText::new("Tailscale not found or not connected").color(WARN));
                        if ui.small_button("Check again").clicked() {
                            self.ts_ip = sys::tailscale_ip();
                        }
                    }
                });
                ui.end_row();

                caption(ui, "Port", INK_2);
                ui.horizontal(|ui| {
                    if ui.add(egui::DragValue::new(&mut self.state.settings.ew_port).range(1024..=65535)).changed() {
                        self.save_settings();
                    }
                    ui.label(RichText::new("UDP · default 21251").small().color(INK_MUTED));
                });
                ui.end_row();

                caption(ui, "Host", INK_2);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!locked, egui::Button::new(format!("Host via IP (port {port})"))).clicked() {
                        self.activate_with(p.id.clone(), true, vec!["--host".into(), port.to_string()]);
                    }
                });
                ui.end_row();

                caption(ui, "Join", INK_2);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.state.settings.ew_join).hint_text("100.x.y.z:21251").desired_width(170.0).font(egui::TextStyle::Monospace));
                    let target = self.state.settings.ew_join.trim().to_string();
                    let valid = target.parse::<std::net::SocketAddr>().is_ok();
                    if ui.add_enabled(!locked && valid, egui::Button::new("Connect")).on_disabled_hover_text("Format: IP:port, e.g. 100.101.102.103:21251").clicked() {
                        self.save_settings();
                        self.activate_with(p.id.clone(), true, vec!["--lobby".into(), target]);
                    }
                });
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new("In IP mode the proxy runs without a window. Connection and logs are shown here in the app. After playing, click 'Stop proxy'.")
                    .small()
                    .color(INK_MUTED),
            );
        });
    }

    // ------------------------------------------------------------ log viewer

    pub(super) fn logs_view(&mut self, ui: &mut Ui) {
        let Some(p) = self.edit.clone() else {
            ui.label(RichText::new("Pick a profile on the left.").color(INK_MUTED));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Logs · {}", p.name)).size(20.0).strong().color(INK));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Open folder").clicked() {
                    let d = ewlog::logs_dir(&self.paths, &p.id);
                    let _ = std::fs::create_dir_all(&d);
                    sys::open_folder(&d);
                }
                let can = self.state.active.as_deref() == Some(p.id.as_str());
                if ui
                    .add_enabled(can, egui::Button::new("Archive current logs"))
                    .on_hover_text("Copies ew_log.txt and Noita's logger.txt into this profile's archive now")
                    .on_disabled_hover_text("Only for the active profile – the logs belong to the last played session")
                    .clicked()
                {
                    self.archive_logs(None);
                }
            });
        });
        ui.label(RichText::new("After every session the logs are copied here automatically. The EW proxy itself keeps only the last two.").small().color(INK_MUTED));
        ui.add_space(10.0);

        if self.logs.is_empty() {
            panel().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("No logs for this profile yet.").color(INK_2));
            });
            return;
        }

        // Session list
        let mut clicked = None;
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            caption(ui, &format!("Sessions  {}", self.logs.len()), INK_MUTED);
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("log_sessions").max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
                for (i, s) in self.logs.iter().enumerate() {
                    let sel = self.log_sel == Some(i);
                    ui.horizontal(|ui| {
                        // Monospace + padding keeps both tag kinds the same width, so the times line up.
                        tag(ui, &format!("{:<8}", s.kind.label()), if s.kind == LogKind::Proxy { kind_color(Kind::EntangledWorlds).linear_multiply(1.6) } else { INK_2 });
                        let text = format!("{}   {:.0} KB", pretty_stamp(&s.stamp), s.bytes as f64 / 1024.0);
                        if ui.selectable_label(sel, RichText::new(text).monospace()).clicked() {
                            clicked = Some(i);
                        }
                    });
                }
            });
        });
        if let Some(i) = clicked {
            self.select_log(i);
        }
        ui.add_space(10.0);

        let Some(loaded) = &self.log_loaded else {
            ui.label(RichText::new("Select a session to see its summary.").color(INK_MUTED));
            return;
        };
        let s = &loaded.summary;

        // Summary
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            caption(ui, "Summary", INK_MUTED);
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                let stat = |ui: &mut Ui, label: &str, value: String, color: Color32| {
                    ui.vertical(|ui| {
                        caption(ui, label, INK_2);
                        ui.label(RichText::new(value).size(18.0).monospace().color(color));
                    });
                };
                if let Some(start) = &s.start {
                    stat(ui, "Start", start.clone(), INK);
                }
                if let Some(d) = &s.duration {
                    stat(ui, "Duration", d.clone(), INK);
                }
                stat(ui, "Players", s.peers.to_string(), INK);
                stat(ui, "Incidents", s.incidents.len().to_string(), if s.incidents.is_empty() { ALIVE } else { WARN });
                stat(ui, "Kicks", s.kicks.to_string(), if s.kicks == 0 { ALIVE } else { DANGER });
                stat(ui, "Warnings", s.warns.to_string(), if s.warns == 0 { INK } else { WARN });
                stat(ui, "Errors", s.errors.to_string(), if s.errors == 0 { INK } else { DANGER });
            });
            if !s.incidents.is_empty() {
                ui.add_space(8.0);
                for inc in &s.incidents {
                    ui.horizontal(|ui| {
                        dot(ui, if inc.kicks > 0 { DANGER } else { WARN });
                        ui.label(RichText::new(format!("{} – {}", inc.from, inc.to)).monospace().color(INK));
                        ui.label(
                            RichText::new(format!(
                                "{} congestion message(s){}",
                                inc.congestion,
                                if inc.kicks > 0 { format!(", {} kick(s)", inc.kicks) } else { String::new() }
                            ))
                            .color(INK_2),
                        );
                    });
                }
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Congestion means Steam no longer accepts packets for that player – usually dropouts on their line/Wi-Fi or the Steam relay route. After 30 s of congestion the proxy kicks them.")
                        .small()
                        .color(INK_MUTED),
                );
            }
        });
        ui.add_space(10.0);

        // Lines
        let only = self.log_only_problems;
        let total = if only { loaded.problems.len() } else { loaded.all.len() };
        let mut toggle = only;
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                caption(ui, &format!("Entries  {total}"), INK_MUTED);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.checkbox(&mut toggle, "only warnings & connection");
                });
            });
            ui.add_space(4.0);
            let lines = if only { &loaded.problems } else { &loaded.all };
            // Collapsed logs are a few hundred rows at most, so plain rendering is fine.
            let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
            // Nested in the page's scroll area: without a minimum it shrinks to the space left on screen.
            egui::ScrollArea::both().id_salt("log_lines").min_scrolled_height(380.0).max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.spacing_mut().interact_size.y = row_h;
                for l in lines {
                    let color = match l.level {
                        Level::Error => DANGER,
                        Level::Warn => WARN,
                        Level::Info => INK_2,
                    };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(RichText::new(&l.time).monospace().color(INK_MUTED));
                        if l.repeat > 1 {
                            ui.add_sized(vec2(56.0, row_h), egui::Label::new(RichText::new(format!("×{}", l.repeat)).monospace().color(color)));
                        } else {
                            ui.add_sized(vec2(56.0, row_h), egui::Label::new(""));
                        }
                        ui.add(egui::Label::new(RichText::new(&l.text).monospace().color(color)).extend());
                    });
                }
            });
        });
        self.log_only_problems = toggle;
    }
}
