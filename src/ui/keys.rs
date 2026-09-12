//! Every overlay keybinding, in one table.
//!
//! # The leader
//!
//! All of them are **Ctrl+Shift+key**. The game has text fields, so an overlay
//! chord must be something a player cannot type. Ctrl+Alt is not an option —
//! it is AltGr on Windows, and on non-US layouts AltGr types everyday
//! characters. `register_hotkey` refuses anything without Ctrl or Alt outright,
//! so the rule holds even if someone adds a binding without reading this.
//!
//! # The modifier is not what stops the game seeing them
//!
//! It stops the player *typing* the key. The game reads keys without checking
//! modifiers at all, so Ctrl+Shift+Down once moved the planner cursor and the
//! game's, and Ctrl+Shift+D arrived as `D`.
//!
//! [`honse_services::input_block`] is what actually stops that, by subclassing
//! the game window and dropping the messages — Raw Input included, which is the
//! path the game turned out to read. Confirmed in play: bound chords no longer
//! reach it. That is why navigation uses the arrows and Space, which read as
//! what they do; before swallowing worked they had to be letters, because a
//! stray arrow moves a selection and a stray Space commits it.
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
    pub const B: u16 = 0x42;
    pub const D: u16 = 0x44;
    pub const I: u16 = 0x49;
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

extern "C" fn toggle_idle(_: *mut std::ffi::c_void) {
    super::idle::toggle();
}

extern "C" fn toggle_overlay(_: *mut std::ffi::c_void) {
    let on = !honse_services::overlay::is_enabled();
    honse_services::overlay::set_enabled(on);
    hlog_info!(target: "training-tracker", "Overlay: {}", if on { "shown" } else { "hidden" });
}

// The arrows and R belong to the planner. They used to be shared with layout
// mode; a panel is now moved by dragging its title bar with the mouse, so these
// keys have one owner again.
extern "C" fn nav_up(_: *mut std::ffi::c_void) {
    super::plan::move_cursor(-1);
}
extern "C" fn nav_down(_: *mut std::ffi::c_void) {
    super::plan::move_cursor(1);
}
extern "C" fn nav_left(_: *mut std::ffi::c_void) {
    super::plan::change_window(-1);
}
extern "C" fn nav_right(_: *mut std::ffi::c_void) {
    super::plan::change_window(1);
}
extern "C" fn reset(_: *mut std::ffi::c_void) {
    super::plan::reset_window();
}

extern "C" fn plan_toggle_open(_: *mut std::ffi::c_void) {
    super::plan::toggle_open();
}
extern "C" fn plan_toggle_song(_: *mut std::ffi::c_void) {
    super::plan::toggle_selected();
}
extern "C" fn plan_toggle_bought(_: *mut std::ffi::c_void) {
    super::plan::toggle_bought_selected();
}

/// Put every panel back where it registered. Not a toggle and not a mode: it is
/// the way out of a layout that has ended up somewhere unhelpful.
extern "C" fn layout_reset(_: *mut std::ffi::c_void) {
    super::layout::reset_all();
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
        id: "overlay.idle",
        label: "Show/hide the Independent Training timer",
        vk: vk::I,
        action: toggle_idle,
    },
    Binding {
        id: "layout.reset",
        label: "Put every panel back in its default position",
        vk: vk::L,
        action: layout_reset,
    },
    Binding {
        id: "plan.open",
        label: "Open/close the song planner",
        vk: vk::P,
        action: plan_toggle_open,
    },
    // The planner's keys. Repeating, because a cursor that needs one press per
    // song is not navigation.
    Binding {
        id: "nav.up",
        label: "Previous song",
        vk: vk::UP,
        action: nav_up,
    },
    Binding {
        id: "nav.down",
        label: "Next song",
        vk: vk::DOWN,
        action: nav_down,
    },
    Binding {
        id: "nav.left",
        label: "Previous concert",
        vk: vk::LEFT,
        action: nav_left,
    },
    Binding {
        id: "nav.right",
        label: "Next concert",
        vk: vk::RIGHT,
        action: nav_right,
    },
    Binding {
        id: "plan.toggle_song",
        label: "Plan/skip the selected song",
        vk: vk::SPACE,
        action: plan_toggle_song,
    },
    Binding {
        id: "plan.toggle_bought",
        label: "Mark the selected song bought / not bought",
        vk: vk::B,
        action: plan_toggle_bought,
    },
    Binding {
        id: "nav.reset",
        label: "Reset this concert to the default plan",
        vk: vk::R,
        action: reset,
    },
];

/// Bindings that fire repeatedly while held. Only the cursor keys — a toggle
/// that repeats is a toggle that flickers.
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
        "Hotkeys: {bound}/{} bound \u{2014} Ctrl+Shift: O overlay, D debug, I idle timer, L reset panels, P planner, arrows nav, Space plan, B bought",
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
