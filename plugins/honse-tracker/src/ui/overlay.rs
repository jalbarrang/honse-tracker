//! L2 overlay helpers: panel frame, scroll helper, content scaling.

use std::cell::Cell;

use crate::compat::egui;

use crate::memory_reader::CareerSnapshot;
use crate::{overlay_cache, overlay_prefs};

use super::constants::{OVERLAY_BASE_WIDTH, OVERLAY_FONT_SIZE, OVERLAY_MAX_HEIGHT};

/// Panel-frame inner margin (must match [`panel_frame`]); content sits inside it.
const PANEL_INNER_MARGIN: f32 = 10.0;

thread_local! {
    static ACTIVE_BASE_WIDTH: Cell<f32> = const { Cell::new(OVERLAY_BASE_WIDTH) };
}

/// Apply the user's content zoom to `ui` (font size + spacing) so the whole
/// panel scales uniformly. The zoom is an explicit setting (slider), not derived
/// from the window size — deriving it from width fed back into the panel's
/// auto-sizing and grew without bound. Returns the applied scale.
pub(super) fn apply_scale(ui: &mut egui::Ui) -> f32 {
    let scale = overlay_prefs::zoom();
    let style = ui.style_mut();
    // Install the normalized Tailwind-style type scale (base 16px), then scale by
    // the zoom. Mapping egui's built-in text styles onto the ramp means `.small()`
    // is `sm`, plain labels are `base`, and headings are `xl` — one coherent
    // scale. `override_font_id` alone is not enough: text set via a `TextStyle` or
    // explicit size bypasses it (egui `FontSelection::resolve`).
    use super::dimens;
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(dimens::FONT_SM * scale, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(dimens::FONT_BASE * scale, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(dimens::FONT_BASE * scale, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(dimens::FONT_XL * scale, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(dimens::FONT_SM * scale, FontFamily::Monospace),
        ),
    ]
    .into();
    style.override_font_id = Some(FontId::proportional(dimens::FONT_BASE * scale));
    let sp = ui.spacing_mut();
    sp.item_spacing *= scale;
    sp.button_padding *= scale;
    sp.interact_size *= scale;
    sp.indent *= scale;
    scale
}

/// The current overlay content scale.
pub(super) fn scale() -> f32 {
    overlay_prefs::zoom()
}

/// Reference game-viewport height (points) the standalone energy pill is sized
/// against. At this height the pill renders at its base size.
const ENERGY_REFERENCE_HEIGHT: f32 = 1080.0;

/// Scale factor derived from the *game* viewport size (not the overlay zoom).
/// The standalone energy HUD pill uses this so it tracks the game resolution like
/// a native HUD element instead of following the hachimi overlay zoom slider.
pub(super) fn viewport_scale(ui: &egui::Ui) -> f32 {
    (ui.ctx().content_rect().height() / ENERGY_REFERENCE_HEIGHT).clamp(0.5, 3.0)
}

/// Draw the standalone energy HUD pill. Unlike [`draw_panel`], this applies no
/// overlay zoom and paints no panel chrome: the body sizes itself from the game
/// viewport (see [`viewport_scale`]) and paints its own outlined text.
pub(super) fn draw_energy_standalone(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui, &CareerSnapshot)) {
    overlay_cache::maybe_request_refresh();
    if let Some(snap) = overlay_cache::snapshot() {
        if snap.is_playing {
            ui.allocate_ui_with_layout(egui::vec2(0.0, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                body(ui, &snap);
            });
        }
    }
}

/// Deterministic content column width (inside the panel-frame margins), driven by
/// the fixed base width × zoom. Use this instead of `ui.available_width()` for
/// full-width elements: under the host's `auto_sized` window `available_width` is
/// measured with a huge value and would inflate the panel (and the window).
pub(super) fn content_width() -> f32 {
    ACTIVE_BASE_WIDTH.with(|w| w.get()) * scale() - 2.0 * PANEL_INNER_MARGIN
}

fn with_base_width<R>(base_width: f32, f: impl FnOnce() -> R) -> R {
    ACTIVE_BASE_WIDTH.with(|active| {
        let prev = active.replace(base_width);
        let out = f();
        active.set(prev);
        out
    })
}

/// The overlay's own background panel (the whole visual, since the host renders
/// the panel chromeless). Rounded dark face with a faint border, matching the
/// Career card so the "inner frame" reads as the overlay itself.
pub(super) fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(PANEL_INNER_MARGIN as i8))
        .corner_radius(egui::CornerRadius::same(12))
        .fill(egui::Color32::from_rgb(0x12, 0x16, 0x1f))
        .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x2c, 0x36, 0x48)))
}

/// Transparent variant of [`panel_frame`]: keeps the inner margin (so layout and
/// [`content_width`] stay consistent) but paints no fill or border. Used by
/// windows whose content already draws its own background (e.g. the energy pill).
pub(super) fn chromeless_frame() -> egui::Frame {
    egui::Frame::new().inner_margin(egui::Margin::same(PANEL_INNER_MARGIN as i8))
}

/// Scaled base font size for callers that set an explicit text size.
#[allow(dead_code)]
pub(super) fn font_size() -> f32 {
    OVERLAY_FONT_SIZE * scale()
}

/// Add vertical space that scales with the panel.
#[allow(dead_code)]
pub(super) fn space(ui: &mut egui::Ui, base: f32) {
    ui.add_space(base * scale());
}

/// Compact zoom slider so the user can scale the whole panel up or down.
///
/// The slider edits a *pending* value; the live zoom (and thus the panel + the
/// slider's own size) only changes when the drag ends. This stops the slider from
/// rescaling under the cursor mid-drag, which made it jitter/overshoot.
pub(super) fn draw_zoom_control(ui: &mut egui::Ui) {
    // Plain egui: the egui `Slider` is an interactive widget whose measured size
    // depends on `slider_width`/`interact_size` (both zoom-scaled per frame), so it
    // can't be a stable egui_taffy leaf — it kept the `shell:zoom` taffy node
    // dirty every frame and flickered the overlay. This is a leaf control row, so
    // egui's own horizontal layout is the right tool.
    ui.horizontal(|ui| {
        ui.label("\u{1f50d} Zoom");
        let mut z = overlay_prefs::pending_zoom();
        let slider = egui::Slider::new(&mut z, overlay_prefs::MIN_ZOOM..=overlay_prefs::MAX_ZOOM)
            // Log scale so the multiplicative range is symmetric: 0.4 .. 2.5
            // places 1.0 (100%) at the visual center (sqrt(0.4 * 2.5) == 1.0).
            .logarithmic(true)
            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
            .show_value(true);
        let resp = ui.add(slider);
        if resp.changed() {
            overlay_prefs::set_pending_zoom(z);
        }
        // Commit on release (drag) or on a discrete change (click / keyboard).
        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
            overlay_prefs::commit_zoom();
            crate::config::persist();
        }
    });
}

/// Draw an overlay panel. `fixed_height` pins the window to that height (× zoom);
/// `None` keeps the default behaviour of auto-sizing up to [`OVERLAY_MAX_HEIGHT`].
/// `chromeless` drops the panel frame's fill + border (for windows whose content,
/// e.g. the energy pill, already paints its own background).
pub(super) fn draw_panel(
    ui: &mut egui::Ui,
    base_width: f32,
    fixed_height: Option<f32>,
    chromeless: bool,
    body: impl FnOnce(&mut egui::Ui, &CareerSnapshot),
) {
    with_base_width(base_width, || {
        overlay_cache::maybe_request_refresh();
        let scale = apply_scale(ui);
        let width = base_width * scale;
        // Cap to the host viewport so a pinned height can't exceed the screen.
        let viewport_height = ui.ctx().content_rect().height();
        let height = match fixed_height {
            Some(h) => (h * scale).min(viewport_height),
            None => viewport_height.min(OVERLAY_MAX_HEIGHT * scale),
        };

        ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(width);
            ui.set_max_height(height);
            if fixed_height.is_some() {
                ui.set_min_height(height);
            }
            let frame = if chromeless { chromeless_frame() } else { panel_frame() };
            frame.show(ui, |ui| match overlay_cache::snapshot() {
                Some(snap) if snap.is_playing => body(ui, &snap),
                _ => {
                    ui.label(egui::RichText::new("Waiting for an active career\u{2026}").italics());
                }
            });
        });
    });
}

pub(super) fn scroll_list(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    // Let the window auto-size to its content: the scroll area shrinks vertically
    // to the body's height, capped at the overlay max (then it scrolls). It still
    // fills the width so full-width rows lay out correctly.
    let max_height = (OVERLAY_MAX_HEIGHT * scale()).min(ui.ctx().content_rect().height());
    egui::ScrollArea::vertical()
        .max_height(max_height)
        .auto_shrink([false, true])
        .show(ui, body);
}
