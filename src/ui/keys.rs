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
    pub const D: u16 = 0x44;
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

// The planner's navigation keys no-op while it is closed, so Ctrl+Shift+Space
// outside it does nothing rather than something surprising.
extern "C" fn plan_toggle_open(_: *mut std::ffi::c_void) {
    super::plan::toggle_open();
}
extern "C" fn plan_up(_: *mut std::ffi::c_void) {
    super::plan::move_cursor(-1);
}
extern "C" fn plan_down(_: *mut std::ffi::c_void) {
    super::plan::move_cursor(1);
}
extern "C" fn plan_prev_window(_: *mut std::ffi::c_void) {
    super::plan::change_window(-1);
}
extern "C" fn plan_next_window(_: *mut std::ffi::c_void) {
    super::plan::change_window(1);
}
extern "C" fn plan_toggle_song(_: *mut std::ffi::c_void) {
    super::plan::toggle_selected();
}
extern "C" fn plan_reset(_: *mut std::ffi::c_void) {
    super::plan::reset_window();
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
        id: "plan.up",
        label: "Song planner: previous song",
        vk: vk::UP,
        action: plan_up,
    },
    Binding {
        id: "plan.down",
        label: "Song planner: next song",
        vk: vk::DOWN,
        action: plan_down,
    },
    Binding {
        id: "plan.prev_window",
        label: "Song planner: previous concert",
        vk: vk::LEFT,
        action: plan_prev_window,
    },
    Binding {
        id: "plan.next_window",
        label: "Song planner: next concert",
        vk: vk::RIGHT,
        action: plan_next_window,
    },
    Binding {
        id: "plan.toggle_song",
        label: "Song planner: plan/skip the selected song",
        vk: vk::SPACE,
        action: plan_toggle_song,
    },
    Binding {
        id: "plan.reset",
        label: "Song planner: reset this concert to guide defaults",
        vk: vk::R,
        action: plan_reset,
    },
];

/// Register every binding. Called once from plugin init.
pub fn install() {
    let mut bound = 0;
    for b in BINDINGS {
        let handle = honse_services::register_hotkey(b.id, b.label, MOD_OVERLAY, b.vk, b.action, std::ptr::null_mut());
        if handle == 0 {
            hlog_warn!(target: "training-tracker", "Hotkey '{}' was refused", b.id);
        } else {
            bound += 1;
        }
    }
    hlog_info!(
        target: "training-tracker",
        "Hotkeys: {bound}/{} bound \u{2014} Ctrl+Shift+O overlay, +D debug, +P planner",
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
}
