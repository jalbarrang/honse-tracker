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
//! Mouse events arrive from [`crate::input_block`] via [`crate::pointer`] and
//! are fed to the context every frame. Which clicks the overlay *takes* is a
//! narrower question: a panel's title bar is a drag handle and claims the mouse
//! while the pointer is on it, and the plugin opts a panel in for the rest of
//! its area when the panel has something to click. Everything else reaches the
//! game — a HUD you are not touching never swallows a click.
//!
//! Keyboard is not fed here at all — hotkeys are polled, not typed.
//!
//! # Moving a panel
//!
//! The top [`theme::TITLE_BAR_HEIGHT`] pixels of a panel are its handle, and
//! they are the panel's own header row — this module draws no header of its
//! own. A panel the game is not showing draws nothing, so it has no handle and
//! cannot be moved; the strip exists only where there is a box under it. What
//! this module owns is the drag, and where the panel ends up.
//!
//! A panel is stored as a corner plus an inset, but a drag moves a rectangle.
//! [`placement_from_rect`] turns the moved rectangle back into the other form,
//! re-picking the corner each frame, so a panel can be dragged anywhere on
//! screen rather than only inward from the corner it started at.
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
pub type DrawFn = Box<dyn FnMut(&mut egui::Ui) -> Painted + Send>;

/// What a panel's draw closure put on screen.
///
/// The registry needs the answer rather than the panel's early return: a panel
/// that painted nothing has no box, so a title strip measured off one would be
/// an invisible grab target sitting over the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Painted {
    /// The panel drew itself, chrome included — and so has a title bar.
    Body,
    /// Nothing at all. The game is not showing the panel, or the player turned
    /// it off; either way there is no box, no title bar and nothing to drag.
    Nothing,
}

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
    /// All four. The order is not stored anywhere — config files use
    /// [`Anchor::name`].
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
    /// Where the panel was drawn last frame. A drag moves this rectangle and
    /// re-derives the corner and inset from it, so it has to be kept.
    rect: egui::Rect,
    /// Where registration put it, so one reset can undo every move at once.
    home: (Anchor, egui::Vec2),
    /// Whether this panel takes the mouse over its whole area. Off until a
    /// panel has something to click, because an interactable panel is one the
    /// game cannot be clicked through.
    interactive: bool,
}

static PANELS: Lazy<Mutex<Vec<Panel>>> = Lazy::new(|| Mutex::new(Vec::new()));
static ENABLED: AtomicBool = AtomicBool::new(true);

/// A title-bar drag in progress, and the panel the last one ended on.
#[derive(Default)]
struct Drag {
    panel: Option<&'static str>,
    finished: Option<&'static str>,
}

static DRAGGING: Lazy<Mutex<Drag>> = Lazy::new(|| Mutex::new(Drag::default()));

/// Whether the pointer is over some panel's title bar.
///
/// The bars are not egui-interactable areas, so
/// [`egui::Context::is_pointer_over_area`] cannot see them. [`draw_panels`] sets
/// this while it has the rects in hand, and [`paint_frame`] reads it after the
/// frame to decide whether the next click belongs to the overlay.
static TITLE_HOVER: AtomicBool = AtomicBool::new(false);

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
        panel.offset = inset(offset);
    }
}

/// An inset is a distance from a corner, so it cannot be negative: a panel
/// pushed past its own corner would leave the screen with no way to see it and
/// drag it back.
fn inset(offset: egui::Vec2) -> egui::Vec2 {
    egui::vec2(offset.x.max(0.0), offset.y.max(0.0))
}

/// Move a rectangle the shortest way that puts it inside `screen`.
///
/// A drag can carry a panel past an edge — the pointer keeps moving after the
/// panel has nowhere left to go. Clamping the rectangle before a placement is
/// derived from it means what gets written to the config is already on screen,
/// rather than depending on egui's `constrain_to` to hide the result every
/// frame.
///
/// A rectangle larger than the screen cannot fit; it is aligned to the near edge
/// and allowed to overflow the far one.
fn clamp_into(rect: egui::Rect, screen: egui::Rect) -> egui::Rect {
    let dx = if rect.left() < screen.left() {
        screen.left() - rect.left()
    } else if rect.right() > screen.right() {
        screen.right() - rect.right()
    } else {
        0.0
    };
    let dy = if rect.top() < screen.top() {
        screen.top() - rect.top()
    } else if rect.bottom() > screen.bottom() {
        screen.bottom() - rect.bottom()
    } else {
        0.0
    };
    rect.translate(egui::vec2(dx, dy))
}

/// The corner a panel is nearest, and its inset from that corner.
///
/// The inverse of what [`Anchor::signed`] plus egui's `Area::anchor` do: those
/// turn a corner and a positive inset into a position. A drag moves a
/// rectangle, which is not in that form, so this turns it back — the corner is
/// the one nearest the panel's centre, and the inset is the gap between the
/// panel's edge and that corner's screen edges.
///
/// Re-deriving the corner every frame is what lets a panel be dragged anywhere.
/// An inset from a fixed corner can only push a panel inward from it; switching
/// corners as the centre crosses the middle of the screen costs nothing because
/// the inset is measured afresh from the same rectangle, so the position does
/// not move when the corner changes.
///
/// The rectangle is clamped into the screen first, so no inset this returns can
/// place a panel somewhere its title bar is off the edge and unreachable.
fn placement_from_rect(rect: egui::Rect, screen: egui::Rect) -> (Anchor, egui::Vec2) {
    let rect = clamp_into(rect, screen);
    let centre = rect.center();
    let anchor = match (centre.x < screen.center().x, centre.y < screen.center().y) {
        (true, true) => Anchor::TopLeft,
        (false, true) => Anchor::TopRight,
        (true, false) => Anchor::BottomLeft,
        (false, false) => Anchor::BottomRight,
    };
    let offset = match anchor {
        Anchor::TopLeft => egui::vec2(rect.left() - screen.left(), rect.top() - screen.top()),
        Anchor::TopRight => egui::vec2(screen.right() - rect.right(), rect.top() - screen.top()),
        Anchor::BottomLeft => egui::vec2(rect.left() - screen.left(), screen.bottom() - rect.bottom()),
        Anchor::BottomRight => egui::vec2(screen.right() - rect.right(), screen.bottom() - rect.bottom()),
    };
    (anchor, inset(offset))
}

/// Let a panel take the mouse, or stop it taking the mouse.
///
/// The plugin turns this on for a panel while it has something to click and off
/// again afterwards. While it is on, clicks landing on that panel do not reach
/// the game.
pub fn set_panel_interactive(id: &str, interactive: bool) {
    if let Some(panel) = PANELS.lock().iter_mut().find(|p| p.id == id) {
        panel.interactive = interactive;
    }
}

/// The panel a title-bar drag just finished on, taken once.
///
/// The drag moves the panel directly; persisting where it landed belongs to the
/// plugin, which owns the config file.
#[must_use]
pub fn take_moved_panel() -> Option<&'static str> {
    DRAGGING.lock().finished.take()
}

/// Put every panel back where it registered.
///
/// The recovery path for a layout that has gone somewhere unhelpful: a panel
/// parked over the thing it was reporting on, or a saved position from a
/// resolution the game no longer runs at.
pub fn reset_all_placements() {
    for panel in PANELS.lock().iter_mut() {
        let (anchor, offset) = panel.home;
        panel.anchor = anchor;
        panel.offset = offset;
    }
}

/// Register a panel. It renders every frame the overlay is enabled, and its own
/// draw closure decides what that means: a panel with nothing to say paints
/// nothing and says so, rather than leaving an empty box on screen.
pub fn register_panel(
    id: &'static str,
    anchor: Anchor,
    offset: egui::Vec2,
    width: f32,
    draw: impl FnMut(&mut egui::Ui) -> Painted + Send + 'static,
) {
    PANELS.lock().push(Panel {
        id,
        anchor,
        offset,
        width,
        draw: Box::new(draw),
        // Empty until the first frame. A drag cannot start before then either,
        // because the strip it grabs is measured off this.
        rect: egui::Rect::ZERO,
        home: (anchor, offset),
        interactive: false,
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
        // An overlay that is not painting cannot know where the pointer is, and
        // must not go on eating clicks on the strength of what it last saw.
        crate::pointer::set_capturing(false);
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
        events: pointer_events(screen),
        modifiers: current_modifiers(),
        ..Default::default()
    };

    let full_output = ctx.run(input, |ctx| draw_panels(ctx, screen));

    // Now that the frame knows where the pointer landed, decide whether the
    // next click is ours. One frame behind by construction — which is harmless,
    // because the move that establishes the hover always precedes the click.
    let capturing =
        ctx.is_pointer_over_area() || DRAGGING.lock().panel.is_some() || TITLE_HOVER.load(Ordering::Acquire);
    crate::pointer::set_capturing(capturing);

    let (output, _platform, _viewports) = egui_directx11::split_output(full_output);

    // SAFETY: as above.
    unsafe { painter.paint(swapchain, ctx, output) }
}

/// The window procedure's mouse events, mapped onto the backbuffer.
///
/// Client pixels and backbuffer pixels are the same thing right up until the
/// game renders at a scaled resolution, at which point a click lands somewhere
/// other than where it was aimed.
#[cfg(windows)]
fn pointer_events(screen: egui::Rect) -> Vec<egui::Event> {
    use crate::pointer::PointerEvent;

    let modifiers = current_modifiers();
    let scale = match crate::input_block::client_size() {
        Some((w, h)) => egui::vec2(screen.width() / w, screen.height() / h),
        None => egui::Vec2::splat(1.0),
    };
    let map = |p: egui::Pos2| egui::pos2(p.x * scale.x, p.y * scale.y);

    crate::pointer::take()
        .into_iter()
        .map(|event| match event {
            PointerEvent::Moved(pos) => egui::Event::PointerMoved(map(pos)),
            PointerEvent::Button { pos, button, pressed } => egui::Event::PointerButton {
                pos: map(pos),
                button,
                pressed,
                modifiers,
            },
            PointerEvent::Wheel(notches) => egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, notches),
                modifiers,
            },
            PointerEvent::Gone => egui::Event::PointerGone,
        })
        .collect()
}

/// Modifiers as egui wants them, from the same source the hotkeys use.
#[cfg(windows)]
fn current_modifiers() -> egui::Modifiers {
    let mods = crate::input_block::held_mods();
    let held = |bit: u8| mods & bit != 0;
    egui::Modifiers {
        alt: held(crate::hotkeys::MOD_ALT),
        ctrl: held(crate::hotkeys::MOD_CTRL),
        shift: held(crate::hotkeys::MOD_SHIFT),
        mac_cmd: false,
        command: held(crate::hotkeys::MOD_CTRL),
    }
}

/// Move the panel a drag is holding, or end the drag.
///
/// Called before the panels are drawn so the panel moves under the cursor this
/// frame rather than trailing it by one.
///
/// The panel's remembered rectangle is where it was drawn last frame, which is
/// where the pointer is holding it. Translating that and re-deriving the corner
/// and inset is what lets a drag cross the middle of the screen: an inset from a
/// fixed corner could only ever push the panel inward from it. The derivation
/// also clamps the moved rectangle into the screen, so a drag cannot park a
/// panel off the edge.
#[cfg(windows)]
fn drag_step(panels: &mut [Panel], screen: egui::Rect, down: bool, delta: egui::Vec2) {
    let mut drag = DRAGGING.lock();
    let Some(id) = drag.panel else {
        return;
    };
    if !down {
        drag.panel = None;
        drag.finished = Some(id);
        return;
    }
    if let Some(panel) = panels.iter_mut().find(|p| p.id == id) {
        let (anchor, offset) = placement_from_rect(panel.rect.translate(delta), screen);
        panel.anchor = anchor;
        panel.offset = offset;
    }
}

#[cfg(windows)]
fn draw_panels(ctx: &egui::Context, screen: egui::Rect) {
    let (pointer, pressed, down, delta) = ctx.input(|i| {
        (
            i.pointer.interact_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.delta(),
        )
    });

    let mut panels = PANELS.lock();
    drag_step(&mut panels, screen, down, delta);
    TITLE_HOVER.store(false, Ordering::Release);

    for panel in panels.iter_mut() {
        let anchor = panel.anchor;
        let width = panel.width;
        let id = panel.id;
        // Only a panel with something to click takes the mouse over its body;
        // the title bar is claimed separately, from the rect it ends up with.
        let interactive = panel.interactive;
        let draw = &mut panel.draw;
        let response = egui::Area::new(egui::Id::new(("honse-overlay", id)))
            .anchor(anchor.align(), anchor.signed(panel.offset))
            .interactable(interactive)
            .constrain_to(screen)
            .show(ctx, |ui| {
                ui.set_width(width);
                draw(ui)
            });

        // A panel with nothing on screen leaves no box, so a strip measured
        // off one would be an invisible grab target sitting over the game.
        if response.inner == Painted::Nothing {
            panel.rect = egui::Rect::ZERO;
            continue;
        }

        let rect = response.response.rect;
        panel.rect = rect;

        // What egui drew is the truth, because it constrains the area to the
        // screen. Deriving the placement from the drawn rectangle keeps what is
        // stored equal to what is on screen, so a position saved at a resolution
        // the game no longer runs at corrects itself on the first frame.
        let (anchor, offset) = placement_from_rect(rect, screen);
        panel.anchor = anchor;
        panel.offset = offset;

        // The handle is the top strip of the panel, which is its own header
        // row. Nothing is drawn for the handle itself — only the cue.
        let strip = title_strip(rect);
        let hovered = pointer.is_some_and(|pos| strip.contains(pos));
        let dragging = DRAGGING.lock().panel == Some(id);
        if hovered {
            TITLE_HOVER.store(true, Ordering::Release);
            // Press to grab. With two panels overlapping the first one in
            // registration order is the one that takes it.
            let mut drag = DRAGGING.lock();
            if pressed && drag.panel.is_none() {
                drag.panel = Some(id);
            }
        }
        if hovered || dragging {
            handle_cue(ctx, id, strip, dragging);
        }
    }
}

/// The draggable strip at the top of a panel.
#[cfg(windows)]
fn title_strip(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), theme::TITLE_BAR_HEIGHT))
}

/// Say that a panel's title bar is there to be picked up.
///
/// A line under the strip rather than a wash over it: at this row height a fill
/// behind small text costs more legibility than the cue is worth, and the strip
/// already carries the panel's own title.
#[cfg(windows)]
fn handle_cue(ctx: &egui::Context, id: &str, strip: egui::Rect, dragging: bool) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new(("honse-overlay-handle", id)),
    ));
    let colour = if dragging {
        theme::ACCENT_BRIGHT
    } else {
        theme::TEXT_FAINT
    };
    painter.hline(
        strip.left()..=strip.right(),
        strip.bottom() - 1.0,
        egui::Stroke::new(if dragging { 2.0 } else { 1.0 }, colour),
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

    #[test]
    fn a_screen_delta_becomes_an_inset_delta_for_every_corner() {
        // Dragging right and down, from each corner in turn. A right-anchored
        // panel moving right gets *closer* to its corner, so its inset shrinks.
        let drag = egui::vec2(10.0, 6.0);
        assert_eq!(Anchor::TopLeft.signed(drag), egui::vec2(10.0, 6.0));
        assert_eq!(Anchor::TopRight.signed(drag), egui::vec2(-10.0, 6.0));
        assert_eq!(Anchor::BottomLeft.signed(drag), egui::vec2(10.0, -6.0));
        assert_eq!(Anchor::BottomRight.signed(drag), egui::vec2(-10.0, -6.0));
    }

    #[test]
    fn insets_never_go_negative() {
        assert_eq!(inset(egui::vec2(-5.0, 12.0)), egui::vec2(0.0, 12.0));
    }

    fn screen() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0))
    }

    /// A corner and inset turned back into a rectangle the way egui's
    /// `Area::anchor` does it: align the size to the corner, then apply the
    /// signed inset. `placement_from_rect` has to invert exactly this.
    fn anchored_rect(anchor: Anchor, offset: egui::Vec2, size: egui::Vec2, screen: egui::Rect) -> egui::Rect {
        let corner = match anchor {
            Anchor::TopLeft => screen.left_top(),
            Anchor::TopRight => screen.right_top() - egui::vec2(size.x, 0.0),
            Anchor::BottomLeft => screen.left_bottom() - egui::vec2(0.0, size.y),
            Anchor::BottomRight => screen.right_bottom() - size,
        };
        egui::Rect::from_min_size(corner + anchor.signed(offset), size)
    }

    #[test]
    fn every_quadrant_stores_against_its_own_corner() {
        let size = egui::vec2(100.0, 100.0);
        let cases = [
            (egui::pos2(50.0, 50.0), Anchor::TopLeft),
            (egui::pos2(1700.0, 50.0), Anchor::TopRight),
            (egui::pos2(50.0, 900.0), Anchor::BottomLeft),
            (egui::pos2(1700.0, 900.0), Anchor::BottomRight),
        ];
        for (min, expected) in cases {
            let rect = egui::Rect::from_min_size(min, size);
            let (anchor, offset) = placement_from_rect(rect, screen());
            assert_eq!(anchor, expected, "{min:?}");
            assert_eq!(anchored_rect(anchor, offset, size, screen()), rect, "{min:?}");
        }
    }

    #[test]
    fn crossing_the_middle_changes_the_corner_without_moving_the_panel() {
        let size = egui::vec2(200.0, 100.0);
        // Two positions straddling the vertical middle of the screen. They are
        // re-anchored on opposite sides, and both have to resolve to the
        // rectangle they came from — that equality is what makes the corner
        // change invisible while dragging across the centre.
        for x in [855.0, 865.0] {
            let rect = egui::Rect::from_min_size(egui::pos2(x, 450.0), size);
            let (anchor, offset) = placement_from_rect(rect, screen());
            assert_eq!(anchored_rect(anchor, offset, size, screen()), rect, "{anchor:?}");
        }
    }

    #[test]
    fn a_rect_past_an_edge_is_pulled_back_inside() {
        let screen = screen();
        let size = egui::vec2(300.0, 200.0);

        // Off the top and the right at once: the shortest way in is down and
        // left, one edge each.
        let off = egui::Rect::from_min_size(egui::pos2(1900.0, -40.0), size);
        let inside = clamp_into(off, screen);
        assert_eq!(inside.top(), screen.top());
        assert_eq!(inside.right(), screen.right());
        assert!(screen.contains_rect(inside), "{inside:?}");

        // Already inside is left exactly alone.
        let already = egui::Rect::from_min_size(egui::pos2(24.0, 96.0), size);
        assert_eq!(clamp_into(already, screen), already);
    }

    #[test]
    fn a_placement_derived_from_an_off_screen_rect_still_lands_on_screen() {
        let screen = screen();
        let size = egui::vec2(300.0, 200.0);
        let off = egui::Rect::from_min_size(egui::pos2(5000.0, 5000.0), size);
        let (anchor, offset) = placement_from_rect(off, screen);
        let placed = anchored_rect(anchor, offset, size, screen);
        assert!(screen.contains_rect(placed), "{anchor:?} {placed:?}");
    }
}
