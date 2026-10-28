//! Look & feel: the modern-retro slot look.

use crate::saves::Kind;
use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Margin, Rect, Response, Rounding, Sense, Shadow, Stroke, TextStyle, Ui, Vec2,
    text::{LayoutJob, TextFormat},
    vec2,
};

// Tokens: one neutral hue, surfaces step up in lightness only.
pub const CAVE: Color32 = Color32::from_rgb(20, 20, 23); // canvas
pub const SLOT: Color32 = Color32::from_rgb(28, 28, 33); // cards
pub const SLOT_HOVER: Color32 = Color32::from_rgb(35, 35, 41);
pub const SLOT_SELECTED: Color32 = Color32::from_rgb(40, 40, 47);
pub const INSET: Color32 = Color32::from_rgb(14, 14, 16); // inputs
pub const EDGE: Color32 = Color32::from_rgba_premultiplied(18, 18, 18, 18);
pub const EDGE_STRONG: Color32 = Color32::from_rgba_premultiplied(40, 40, 40, 40);
pub const INK: Color32 = Color32::from_rgb(230, 230, 234);
pub const INK_2: Color32 = Color32::from_rgb(170, 170, 178);
pub const INK_MUTED: Color32 = Color32::from_rgb(112, 112, 122);

// Meaning colors, kept from the original design.
pub const ALIVE: Color32 = Color32::from_rgb(120, 220, 120); // active profile
pub const WARD: Color32 = Color32::from_rgb(214, 178, 60); // protected
pub const WARN: Color32 = Color32::from_rgb(220, 160, 60);
pub const DANGER: Color32 = Color32::from_rgb(220, 95, 90);
pub const STEAM: Color32 = Color32::from_rgb(70, 130, 200);

pub fn kind_color(k: Kind) -> Color32 {
    match k {
        Kind::Singleplayer => Color32::from_rgb(110, 110, 120),
        Kind::NoitaTogether => Color32::from_rgb(40, 150, 80),
        Kind::EntangledWorlds => Color32::from_rgb(140, 80, 210),
    }
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_style(modern_style());
}

fn modern_style() -> egui::Style {
    let mut s = egui::Style::default();
    s.text_styles = [
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Heading, FontId::proportional(22.0)),
        (TextStyle::Monospace, FontId::monospace(12.5)),
    ]
    .into();
    s.spacing.item_spacing = vec2(8.0, 6.0);
    s.spacing.button_padding = vec2(12.0, 5.0);
    s.spacing.interact_size.y = 26.0;
    s.spacing.window_margin = Margin::same(16.0);

    let mut v = egui::Visuals::dark();
    let r = Rounding::same(3.0);
    v.panel_fill = CAVE;
    v.window_fill = SLOT;
    v.window_stroke = Stroke::new(1.0_f32, EDGE_STRONG);
    v.window_rounding = Rounding::same(5.0);
    v.window_shadow = Shadow { offset: vec2(0.0, 8.0), blur: 24.0, spread: 0.0, color: Color32::from_black_alpha(140) };
    v.popup_shadow = Shadow { offset: vec2(0.0, 4.0), blur: 12.0, spread: 0.0, color: Color32::from_black_alpha(120) };
    v.menu_rounding = r;
    v.extreme_bg_color = INSET;
    v.faint_bg_color = SLOT;
    v.override_text_color = None;
    v.selection.bg_fill = Color32::from_rgb(70, 70, 82);
    v.selection.stroke = Stroke::new(1.0_f32, INK);
    v.hyperlink_color = STEAM;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = CAVE;
    w.noninteractive.weak_bg_fill = SLOT;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, EDGE);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, INK_2);
    w.noninteractive.rounding = r;
    for (state, fill, stroke, fg) in [
        (&mut w.inactive, SLOT, EDGE_STRONG, INK),
        (&mut w.hovered, SLOT_HOVER, Color32::from_rgba_premultiplied(70, 70, 70, 70), INK),
        (&mut w.active, SLOT_SELECTED, Color32::from_rgba_premultiplied(90, 90, 90, 90), INK),
        (&mut w.open, SLOT_SELECTED, EDGE_STRONG, INK),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0_f32, stroke);
        state.fg_stroke = Stroke::new(1.0_f32, fg);
        state.rounding = r;
        state.expansion = 0.0;
    }
    s.visuals = v;
    s
}

// ---------------------------------------------------------------- small building blocks

fn shade(c: Color32, f: f32) -> Color32 {
    let m = |x: u8| ((x as f32 * f).clamp(0.0, 255.0)) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// Uppercase monospace label with tracking: section titles, field names.
pub fn caption(ui: &mut Ui, text: &str, color: Color32) -> Response {
    ui.label(tracked(text, 10.5, color, 1.4))
}

pub fn tracked(text: &str, size: f32, color: Color32, spacing: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        &text.to_uppercase(),
        0.0,
        TextFormat { font_id: FontId::new(size, FontFamily::Monospace), color, extra_letter_spacing: spacing, ..Default::default() },
    );
    job
}

/// Flat fill with a one-pixel light top/left and dark bottom/right edge – an inventory slot.
pub fn bevel(p: &egui::Painter, rect: Rect, base: Color32) {
    p.rect_filled(rect, 2.0, base);
    let hi = Stroke::new(1.0_f32, shade(base, 1.45));
    let lo = Stroke::new(1.0_f32, shade(base, 0.55));
    let r = rect.shrink(0.5);
    p.line_segment([r.left_bottom() + vec2(0.0, -2.0), r.left_top() + vec2(0.0, 2.0)], hi);
    p.line_segment([r.left_top() + vec2(2.0, 0.0), r.right_top() + vec2(-2.0, 0.0)], hi);
    p.line_segment([r.right_top() + vec2(0.0, 2.0), r.right_bottom() + vec2(0.0, -2.0)], lo);
    p.line_segment([r.left_bottom() + vec2(2.0, 0.0), r.right_bottom() + vec2(-2.0, 0.0)], lo);
}

/// The profile's "spell slot": kind color, bevelled, with its short code.
pub fn kind_tile(ui: &mut Ui, kind: Kind, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    bevel(ui.painter(), rect, kind_color(kind));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, kind.short(), FontId::monospace((size * 0.36).round()), Color32::WHITE);
    resp
}

/// Small tag: monospace caps on a tinted ground.
pub fn tag(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::none()
        .fill(color.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.55)))
        .rounding(2.0)
        .inner_margin(Margin::symmetric(5.0, 1.0))
        .show(ui, |ui| ui.label(tracked(text, 10.0, color, 1.0)));
}

pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
    ui.painter().rect_filled(Rect::from_center_size(rect.center(), Vec2::splat(6.0)), 1.0, color);
}

/// Status item for the header strip: ■ CAPTION value
pub fn status(ui: &mut Ui, label: &str, value: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        dot(ui, color);
        caption(ui, label, INK_MUTED);
        ui.label(egui::RichText::new(value).color(INK));
    });
}

/// The one big action of a view. Bevelled in the profile color, presses down a pixel.
pub fn cta(ui: &mut Ui, text: &str, color: Color32, enabled: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(16.0), Color32::WHITE);
    let size = vec2((galley.size().x + 40.0).max(200.0), 42.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let (base, offset) = if !enabled {
        (shade(color, 0.45), 0.0)
    } else if resp.is_pointer_button_down_on() {
        (shade(color, 0.85), 1.0)
    } else if resp.hovered() {
        (shade(color, 1.15), 0.0)
    } else {
        (color, 0.0)
    };
    let r = rect.translate(vec2(0.0, offset));
    bevel(p, r, base);
    let text_color = if enabled { Color32::WHITE } else { Color32::from_white_alpha(110) };
    p.galley(r.center() - galley.size() / 2.0, galley, text_color);
    if enabled {
        resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Danger-colored secondary button.
pub fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_string()).color(DANGER))
}

/// "2026-10-26_02-17-22[_n]" → "2026-10-26  02:17:22"
pub fn pretty_stamp(s: &str) -> String {
    let b = s.as_bytes();
    if s.len() >= 19 && b[4] == b'-' && b[10] == b'_' {
        let rest = &s[19..];
        // Snapshots from before 1.1 carry a German suffix.
        let rest = rest.trim_start_matches('_').replace("vor-Wiederherstellung", "before-restore").replace('-', " ");
        format!("{}  {}:{}:{}{}{}", &s[..10], &s[11..13], &s[14..16], &s[17..19], if rest.is_empty() { "" } else { "  " }, rest)
    } else {
        s.to_string()
    }
}
