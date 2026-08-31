//! Self-hosted egui overlay: our own context, our own D3D11 pass, driven from
//! edge's present callback.
//!
//! Nothing here touches the host's egui. No `egui::Ui` crosses the plugin↔host
//! boundary, so none of the ABI-lockstep rules in `edge_sdk::gui` apply and the
//! panels render whether or not the Hachimi menu is open — which is the entire
//! reason this module exists.
//!
//! # Shape
//!
//! - [`theme`] holds the design tokens and styles our context.
//! - [`render`] owns the D3D11 pass (per-frame render target, colour space).
//! - [`d3d11_state`] puts the game's pipeline state back afterwards.
//! - This file owns the panel registry and the once-per-present frame.
//!
//! # Input
//!
//! There is none yet. The overlay paints and forgets; nothing is clickable and
//! nothing is dragged. Panels sit where their registration puts them. Mouse
//! input arrives with the dormant-WndProc work and is deliberately not a
//! prerequisite for anything on screen today.
//!
//! # One instance per DLL
//!
//! These statics are per-DLL, because every plugin links its own copy of this
//! crate. Today exactly one plugin does.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

pub mod theme;

#[cfg(windows)]
mod d3d11_state;
#[cfg(windows)]
mod render;

/// Draw callback for one panel. Receives a `Ui` from OUR context.
pub type DrawFn = Box<dyn FnMut(&mut egui::Ui) + Send>;

/// Where a panel pins itself. Positions are stored as a corner plus an offset
/// so a resolution change moves nothing — see the design canvas' anchor rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Anchor {
    /// All four, in the order layout mode cycles them.
    pub const ALL: [Self; 4] = [Self::TopLeft, Self::TopRight, Self::BottomRight, Self::BottomLeft];

    /// Stable name for config files — an index would renumber if the enum grew.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
        }
    }

    /// Parse [`Anchor::name`]. Unknown names fail rather than defaulting, so a
    /// typo in a config surfaces instead of silently moving a panel.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == name)
    }

    /// The next corner, clockwise from top-left.
    #[must_use]
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&a| a == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    const fn align(self) -> egui::Align2 {
        match self {
            Self::TopLeft => egui::Align2::LEFT_TOP,
            Self::TopRight => egui::Align2::RIGHT_TOP,
            Self::BottomLeft => egui::Align2::LEFT_BOTTOM,
            Self::BottomRight => egui::Align2::RIGHT_BOTTOM,
        }
    }

    /// Offset from the anchored corner, with the sign the corner implies, so a
    /// caller always passes positive "inset from my corner" values.
    const fn signed(self, offset: egui::Vec2) -> egui::Vec2 {
        match self {
            Self::TopLeft => egui::vec2(offset.x, offset.y),
            Self::TopRight => egui::vec2(-offset.x, offset.y),
            Self::BottomLeft => egui::vec2(offset.x, -offset.y),
            Self::BottomRight => egui::vec2(-offset.x, -offset.y),
        }
    }
}

struct Panel {
    id: &'static str,
    anchor: Anchor,
    offset: egui::Vec2,
    width: f32,
    draw: DrawFn,
    /// Where registration put it, kept so layout mode can undo a move.
    home: (Anchor, egui::Vec2),
}

static PANELS: Lazy<Mutex<Vec<Panel>>> = Lazy::new(|| Mutex::new(Vec::new()));
static ENABLED: AtomicBool = AtomicBool::new(true);

/// The panel layout mode is editing, or `None` when it is off.
///
/// Lives here rather than in the plugin because [`draw_panels`] is what has to
/// outline the panels — the state and the thing it changes are one step apart.
static LAYOUT_SELECTION: Lazy<Mutex<Option<&'static str>>> = Lazy::new(|| Mutex::new(None));

/// Enter layout mode on a panel, or leave it with `None`.
pub fn set_layout_selection(id: Option<&'static str>) {
    *LAYOUT_SELECTION.lock() = id;
}

/// The panel layout mode is editing.
#[must_use]
pub fn layout_selection() -> Option<&'static str> {
    *LAYOUT_SELECTION.lock()
}

/// Registered panel ids, in registration order.
#[must_use]
pub fn panel_ids() -> Vec<&'static str> {
    PANELS.lock().iter().map(|p| p.id).collect()
}

/// Where a panel currently sits.
#[must_use]
pub fn placement(id: &str) -> Option<(Anchor, egui::Vec2)> {
    PANELS.lock().iter().find(|p| p.id == id).map(|p| (p.anchor, p.offset))
}

/// Move a panel. Offsets are insets from the anchored corner, so they are
/// clamped non-negative — a negative inset would push a panel off screen with
/// no way to see it and drag it back.
pub fn set_placement(id: &str, anchor: Anchor, offset: egui::Vec2) {
    if let Some(panel) = PANELS.lock().iter_mut().find(|p| p.id == id) {
        panel.anchor = anchor;
        panel.offset = egui::vec2(offset.x.max(0.0), offset.y.max(0.0));
    }
}

/// Put a panel back where registration placed it.
pub fn reset_placement(id: &str) {
    if let Some(panel) = PANELS.lock().iter_mut().find(|p| p.id == id) {
        let (anchor, offset) = panel.home;
        panel.anchor = anchor;
        panel.offset = offset;
    }
}

/// Register a panel. It renders every frame the overlay is enabled and its own
/// draw closure chooses to put something on screen — a panel with nothing to
/// say should draw nothing rather than an empty frame.
pub fn register_panel(
    id: &'static str,
    anchor: Anchor,
    offset: egui::Vec2,
    width: f32,
    draw: impl FnMut(&mut egui::Ui) + Send + 'static,
) {
    PANELS.lock().push(Panel {
        id,
        anchor,
        offset,
        width,
        draw: Box::new(draw),
        home: (anchor, offset),
    });
}

/// Draw `content` inside the standard panel chrome — background, border,
/// rounding, padding.
///
/// Call this *after* deciding you have something to show. The chrome is the
/// panel's to paint precisely so that it can be skipped: drawing it in the
/// registry meant a panel with nothing to say still left an empty box on
/// screen, which reads as a bug rather than as absence.
pub fn chrome(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::PANEL)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_PANEL))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            // Pin to the width the registration asked for, so a panel whose
            // content is narrow still gets a full-width box.
            ui.set_width(ui.available_width());
            content(ui);
        });
}

/// Turn the whole overlay off without unregistering anything.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Release);
}

/// Whether the overlay is painting.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

// ── the frame ───────────────────────────────────────────────────────────────

/// Consecutive render failures before the overlay gives up for this session.
///
/// A resize or fullscreen transition reliably produces a handful of transient
/// failures; treating the first one as fatal means the HUD disappears the first
/// time you alt-tab. Only a sustained run of them is a real fault.
const FAILURE_LIMIT: u32 = 300;

static CONSECUTIVE_FAILURES: AtomicU32 = AtomicU32::new(0);
static DISABLED_BY_FAILURE: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
static CONTEXT: Lazy<egui::Context> = Lazy::new(|| {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    ctx
});

#[cfg(windows)]
static PAINTER: Lazy<Mutex<Option<render::Painter>>> = Lazy::new(|| Mutex::new(None));

/// Paint one frame. Called from the present callback with edge's raw
/// `IDXGISwapChain` pointer.
///
/// Never panics out into the render thread: a failure increments the counter
/// and the next frame tries again.
#[cfg(windows)]
pub(crate) fn present(swapchain: *mut c_void) {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::IDXGISwapChain;

    if swapchain.is_null() || !is_enabled() || DISABLED_BY_FAILURE.load(Ordering::Acquire) {
        return;
    }
    // SAFETY: edge hands us the live swapchain for the frame being presented.
    // `from_raw_borrowed` does not take ownership, so we never release the
    // game's reference.
    let Some(swapchain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&swapchain) }) else {
        return;
    };

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| paint_frame(swapchain)));
    match result {
        Ok(Ok(())) => {
            CONSECUTIVE_FAILURES.store(0, Ordering::Release);
        }
        Ok(Err(err)) => note_failure(&format!("{err}")),
        Err(_) => note_failure("panic in overlay paint"),
    }
}

/// Non-Windows builds compile the registry but never paint.
#[cfg(not(windows))]
pub(crate) fn present(_swapchain: *mut c_void) {}

#[cfg(windows)]
fn note_failure(what: &str) {
    let n = CONSECUTIVE_FAILURES.fetch_add(1, Ordering::AcqRel) + 1;
    if n == 1 || n.is_multiple_of(60) {
        log::warn!("honse-services: overlay render failed ({n} in a row): {what}");
    }
    if n >= FAILURE_LIMIT && !DISABLED_BY_FAILURE.swap(true, Ordering::AcqRel) {
        log::error!("honse-services: overlay disabled after {FAILURE_LIMIT} consecutive render failures");
        // Drop the painter so a later fix (or a device reset) starts clean.
        *PAINTER.lock() = None;
    }
}

#[cfg(windows)]
fn paint_frame(swapchain: &windows::Win32::Graphics::Dxgi::IDXGISwapChain) -> windows::core::Result<()> {
    let mut guard = PAINTER.lock();
    if guard.is_none() {
        // SAFETY: called from the present callback with the live swapchain.
        *guard = Some(unsafe { render::Painter::new(swapchain) }?);
        log::info!("honse-services: overlay painter created");
        // The swapchain names the window the game actually renders into —
        // authoritative, unlike guessing at the process's first visible
        // top-level window, which can be a splash or a helper.
        // SAFETY: live swapchain from the present callback.
        if let Some(hwnd) = unsafe { render::output_window(swapchain) } {
            crate::input_block::note_render_window(hwnd);
        }
    }
    let painter = guard.as_mut().expect("painter created above");

    // SAFETY: as above.
    let (width, height) = unsafe { render::Painter::backbuffer_size(swapchain) }?;
    if width == 0 || height == 0 {
        return Ok(());
    }

    let ctx = &*CONTEXT;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32));
    let input = egui::RawInput {
        screen_rect: Some(screen),
        // No events: the overlay reads no input yet. See the module docs.
        ..Default::default()
    };

    let full_output = ctx.run(input, |ctx| draw_panels(ctx, screen));
    let (output, _platform, _viewports) = egui_directx11::split_output(full_output);

    // SAFETY: as above.
    unsafe { painter.paint(swapchain, ctx, output) }
}

#[cfg(windows)]
fn draw_panels(ctx: &egui::Context, screen: egui::Rect) {
    let selected = layout_selection();
    let mut panels = PANELS.lock();
    for panel in panels.iter_mut() {
        let anchor = panel.anchor;
        let width = panel.width;
        let id = panel.id;
        let draw = &mut panel.draw;
        let response = egui::Area::new(egui::Id::new(("honse-overlay", id)))
            .anchor(anchor.align(), anchor.signed(panel.offset))
            .interactable(false)
            .constrain_to(screen)
            .show(ctx, |ui| {
                ui.set_width(width);
                if selected.is_some() {
                    // Layout mode shows every panel, including ones with
                    // nothing to say — you cannot place what you cannot see.
                    ui.set_min_height(theme::LAYOUT_GHOST_HEIGHT);
                }
                // No chrome here: the panel paints its own via `chrome`, so a
                // panel that returns early leaves nothing at all on screen.
                draw(ui);
            });

        if let Some(selected) = selected {
            outline(ctx, id, response.response.rect, selected == id);
        }
    }
}

/// Layout-mode decoration: box the panel and name it.
#[cfg(windows)]
fn outline(ctx: &egui::Context, id: &str, rect: egui::Rect, selected: bool) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new(("honse-overlay-layout", id)),
    ));
    let colour = if selected { theme::ACCENT_BRIGHT } else { theme::TEXT_FAINT };
    painter.rect_stroke(
        rect.expand(2.0),
        egui::CornerRadius::same(theme::RADIUS_PANEL),
        egui::Stroke::new(if selected { 2.0 } else { 1.0 }, colour),
        egui::StrokeKind::Outside,
    );
    painter.text(
        rect.left_top() + egui::vec2(2.0, -4.0),
        egui::Align2::LEFT_BOTTOM,
        id,
        theme::text::meta(),
        colour,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_offsets_point_inward_from_every_corner() {
        let d = egui::vec2(24.0, 16.0);
        assert_eq!(Anchor::TopLeft.signed(d), egui::vec2(24.0, 16.0));
        assert_eq!(Anchor::TopRight.signed(d), egui::vec2(-24.0, 16.0));
        assert_eq!(Anchor::BottomLeft.signed(d), egui::vec2(24.0, -16.0));
        assert_eq!(Anchor::BottomRight.signed(d), egui::vec2(-24.0, -16.0));
    }

    #[test]
    fn overlay_is_enabled_by_default() {
        assert!(is_enabled());
    }
}
