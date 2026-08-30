//! Every overlay keybinding, in one table.
//!
//! # The leader
//!
//! All of them are **Ctrl+Shift+key**. The game has text fields and its own
//! shortcuts, so an overlay chord must be something a player cannot type.
//! Ctrl+Alt is not an option — it is AltGr on Windows, and on non-US layouts
//! AltGr types everyday characters. `register_hotkey` refuses anything without
//! Ctrl or Alt outright, so the rule holds even if someone adds a binding here
//! without reading this.
//!
//! # Why polling is enough
//!
//! `honse_services::hotkeys` reads `GetAsyncKeyState` from the present job
//! list, edge-triggered on the down-transition and gated on the game window
//! being foreground. No WndProc, so this works today, and a chord held while
//! you alt-tab away cannot fire on the way back.

use honse_services::MOD_OVERLAY;

/// Virtual-key codes. Named rather than inlined so the table below reads as
/// keys instead of hex.
mod vk {
    pub const SPACE: u16 = 0x20;
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const TAB: u16 = 0x09;
    pub const A: u16 = 0x41;
    pub const D: u16 = 0x44;
    pub const L: u16 = 0x4C;
    pub const O: u16 = 0x4F;
    pub const P: u16 = 0x50;
    pub const R: u16 = 0x52;
}

/// One binding: id, the label a rebind UI would show, its key, and what it does.
struct Binding {
    id: &'static str,
    label: &'static str,
    vk: u16,
    action: extern "C" fn(*mut std::ffi::c_void),
}

extern "C" fn toggle_debug(_: *mut std::ffi::c_void) {
    super::debug::toggle();
}

extern "C" fn toggle_overlay(_: *mut std::ffi::c_void) {
    let on = !honse_services::overlay::is_enabled();
    honse_services::overlay::set_enabled(on);
    hlog_info!(target: "training-tracker", "Overlay: {}", if on { "shown" } else { "hidden" });
}

// The arrows and R are shared: layout mode owns them while it is on, the
// planner otherwise, and neither when both are closed. Sharing rather than
// binding more chords keeps the whole scheme inside Ctrl+Shift, and only one
// of the two can be open at a time by construction.
extern "C" fn nav_up(_: *mut std::ffi::c_void) {
    if super::layout::is_active() {
        super::layout::nudge(0.0, -1.0);
    } else {
        super::plan::move_cursor(-1);
    }
}
extern "C" fn nav_down(_: *mut std::ffi::c_void) {
    if super::layout::is_active() {
        super::layout::nudge(0.0, 1.0);
    } else {
        super::plan::move_cursor(1);
    }
}
extern "C" fn nav_left(_: *mut std::ffi::c_void) {
    if super::layout::is_active() {
        super::layout::nudge(-1.0, 0.0);
    } else {
        super::plan::change_window(-1);
    }
}
extern "C" fn nav_right(_: *mut std::ffi::c_void) {
    if super::layout::is_active() {
        super::layout::nudge(1.0, 0.0);
    } else {
        super::plan::change_window(1);
    }
}
extern "C" fn reset(_: *mut std::ffi::c_void) {
    if super::layout::is_active() {
        super::layout::reset_selected();
    } else {
        super::plan::reset_window();
    }
}

extern "C" fn plan_toggle_open(_: *mut std::ffi::c_void) {
    // Layout mode and the planner both own the arrows; opening one closes the
    // other rather than leaving the keys ambiguous.
    if super::layout::is_active() {
        super::layout::toggle();
    }
    super::plan::toggle_open();
}
extern "C" fn plan_toggle_song(_: *mut std::ffi::c_void) {
    super::plan::toggle_selected();
}

extern "C" fn layout_toggle(_: *mut std::ffi::c_void) {
    if super::plan::is_open() {
        super::plan::toggle_open();
    }
    super::layout::toggle();
}
extern "C" fn layout_next_panel(_: *mut std::ffi::c_void) {
    super::layout::select_next();
}
extern "C" fn layout_cycle_anchor(_: *mut std::ffi::c_void) {
    super::layout::cycle_anchor();
}

const BINDINGS: &[Binding] = &[
    Binding {
        id: "overlay.toggle",
        label: "Show/hide the overlay",
        vk: vk::O,
        action: toggle_overlay,
    },
    Binding {
        id: "overlay.debug",
        label: "Show/hide the screen debug readout",
        vk: vk::D,
        action: toggle_debug,
    },
    Binding {
        id: "plan.open",
        label: "Open/close the song planner",
        vk: vk::P,
        action: plan_toggle_open,
    },
    Binding {
        id: "layout.toggle",
        label: "Enter/leave layout mode",
        vk: vk::L,
        action: layout_toggle,
    },
    Binding {
        id: "layout.next_panel",
        label: "Layout mode: select the next panel",
        vk: vk::TAB,
        action: layout_next_panel,
    },
    Binding {
        id: "layout.cycle_anchor",
        label: "Layout mode: send the panel to the next corner",
        vk: vk::A,
        action: layout_cycle_anchor,
    },
    // Shared between layout mode and the planner. Repeating, because nudging a
    // panel one step per press would mean fifty presses to cross the screen.
    Binding {
        id: "nav.up",
        label: "Previous song / nudge panel up",
        vk: vk::UP,
        action: nav_up,
    },
    Binding {
        id: "nav.down",
        label: "Next song / nudge panel down",
        vk: vk::DOWN,
        action: nav_down,
    },
    Binding {
        id: "nav.left",
        label: "Previous concert / nudge panel left",
        vk: vk::LEFT,
        action: nav_left,
    },
    Binding {
        id: "nav.right",
        label: "Next concert / nudge panel right",
        vk: vk::RIGHT,
        action: nav_right,
    },
    Binding {
        id: "plan.toggle_song",
        label: "Song planner: plan/skip the selected song",
        vk: vk::SPACE,
        action: plan_toggle_song,
    },
    Binding {
        id: "nav.reset",
        label: "Reset this concert / this panel's position",
        vk: vk::R,
        action: reset,
    },
];

/// Bindings that fire repeatedly while held. Only the nudge/cursor keys — a
/// toggle that repeats is a toggle that flickers.
const REPEATING: &[&str] = &["nav.up", "nav.down", "nav.left", "nav.right"];

/// Register every binding. Called once from plugin init.
pub fn install() {
    let mut bound = 0;
    for b in BINDINGS {
        let handle = honse_services::register_hotkey(b.id, b.label, MOD_OVERLAY, b.vk, b.action, std::ptr::null_mut());
        if handle == 0 {
            hlog_warn!(target: "training-tracker", "Hotkey '{}' was refused", b.id);
        } else {
            bound += 1;
            if REPEATING.contains(&b.id) {
                honse_services::hotkeys::set_repeat(handle, true);
            }
        }
    }
    hlog_info!(
        target: "training-tracker",
        "Hotkeys: {bound}/{} bound \u{2014} Ctrl+Shift+O overlay, +D debug, +P planner, +L layout",
        BINDINGS.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_binding_uses_the_overlay_leader() {
        // A binding the player could type would be refused at registration;
        // catching it here says which one, rather than at runtime.
        assert!(!honse_services::hotkeys::mods_are_typable(MOD_OVERLAY));
    }

    #[test]
    fn binding_ids_and_keys_are_unique() {
        for (i, a) in BINDINGS.iter().enumerate() {
            for b in &BINDINGS[i + 1..] {
                assert_ne!(a.id, b.id, "duplicate binding id {}", a.id);
                assert_ne!(a.vk, b.vk, "'{}' and '{}' share a key", a.id, b.id);
            }
        }
    }

    #[test]
    fn every_repeating_id_is_a_real_binding() {
        for id in REPEATING {
            assert!(BINDINGS.iter().any(|b| b.id == *id), "'{id}' repeats but is unbound");
        }
    }
}
