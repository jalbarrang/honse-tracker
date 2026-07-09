//! GUI rendering via the Hachimi plugin menu system.
//!
//! With API v9 the host hands plugins the real `egui::Ui`, so we draw with egui
//! directly. Registers a Control Center tab and independent tracker overlay panels.

use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};

use crate::compat::{egui, ui_from_ptr, Sdk};

mod career;
mod constants;
mod dimens;
// Race-condition icon toggles (weather/season/time). Currently hidden from the
// UI per product decision; kept dormant so it can be re-enabled cheaply.
#[allow(dead_code)]
mod icons;
mod menu;
mod overlay;
mod scenario;
mod skill_shop_tab;
pub(crate) mod textures;
// Shared formatting/color helpers; several were consumed only by the removed
// Training/Skills tabs but are kept for reuse.
#[allow(dead_code)]
mod util;

// Public API re-export (unused within crate; kept for external callers).
#[allow(unused_imports)]
pub use util::bond_color;

type PanelBody = fn(&mut egui::Ui, &crate::memory_reader::CareerSnapshot);

struct TrackerPanel {
    id: &'static str,
    hotkey_id: &'static str,
    label: &'static str,
    callback: edge_sdk::ffi::GuiMenuSectionCallback,
}

const PANELS: [TrackerPanel; 6] = [
    TrackerPanel {
        id: constants::ENERGY_OVERLAY_ID,
        hotkey_id: "training-tracker.toggle_energy",
        label: "Energy",
        callback: draw_energy_overlay,
    },
    TrackerPanel {
        id: constants::TRAINING_OVERLAY_ID,
        hotkey_id: "training-tracker.toggle_training",
        label: "Training",
        callback: draw_training_overlay,
    },
    TrackerPanel {
        id: constants::BONDS_OVERLAY_ID,
        hotkey_id: "training-tracker.toggle_bonds",
        label: "Bonds",
        callback: draw_bonds_overlay,
    },
    TrackerPanel {
        id: constants::SCENARIO_OVERLAY_ID,
        hotkey_id: "training-tracker.toggle_scenario",
        label: "Scenario",
        callback: draw_scenario_overlay,
    },
    TrackerPanel {
        id: constants::SHOP_OVERLAY_ID,
        hotkey_id: "training-tracker.toggle_shop",
        label: "Shop",
        callback: draw_shop_overlay,
    },
    TrackerPanel {
        id: constants::RANK_OVERLAY_ID,
        hotkey_id: "training-tracker.toggle_rank",
        label: "Rank",
        callback: draw_rank_overlay,
    },
];

/// Register the plugin's UI components with the Hachimi GUI.
pub fn register_ui() {
    let sdk = Sdk::get();

    // Top-level Control Center tab (was an L1 page under the Plugins tab). The host
    // already hands us a live `egui::Ui` inside its own native slot, and the page
    // body is pure egui — so draw it directly.
    sdk.register_tab(|ui| {
        if panic::catch_unwind(AssertUnwindSafe(|| menu::draw(ui))).is_err() {
            hlog_error!("training-tracker tab draw PANICKED");
        }
    });

    let mut registered = 0;
    for panel in PANELS {
        if sdk.register_panel_chromeless(panel.id, panel.callback, std::ptr::null_mut()) != 0 {
            registered += 1;
            crate::compat::set_overlay_visible_if_unset(panel.id, false);
        } else {
            hlog_warn!(target: "training-tracker", "L2 panel registration declined by host: {}", panel.id);
        }

        let hotkey_label = format!("Toggle {} Panel", panel.label);
        sdk.register_hotkey(
            panel.hotkey_id,
            &hotkey_label,
            0,
            0,
            toggle_panel_hotkey,
            panel.id.as_ptr().cast_mut().cast(),
        );
    }

    sdk.register_hotkey(
        "training-tracker.toggle_all",
        "Toggle All Tracker Panels",
        0,
        0,
        toggle_all_hotkey,
        std::ptr::null_mut(),
    );

    sdk.register_hotkey(
        "training-tracker.toggle_tracking",
        "Start/Stop Tracking",
        0,
        0,
        toggle_tracking_hotkey,
        std::ptr::null_mut(),
    );

    hlog_info!(target: "training-tracker", "UI registered (L1 page + {registered} chromeless L2 panels)");
}

extern "C" fn toggle_tracking_hotkey(_userdata: *mut c_void) {
    use std::sync::atomic::Ordering;

    use crate::{memory_reader, overlay_cache};

    if panic::catch_unwind(|| {
        let sdk = Sdk::get();
        if memory_reader::TRACKING.load(Ordering::Relaxed) {
            memory_reader::stop_tracking();
            overlay_cache::reset_career_state();
            sdk.show_notification("Memory tracking stopped");
        } else {
            match memory_reader::start_tracking() {
                Ok(()) => sdk.show_notification("Memory tracking started!"),
                Err(e) => {
                    sdk.show_notification(&format!("Failed: {e}"));
                    hlog_error!(target: "training-tracker", "start_tracking failed: {e}");
                    false
                }
            };
        }
    })
    .is_err()
    {
        hlog_error!(target: "training-tracker", "toggle_tracking_hotkey PANICKED");
    }
}

extern "C" fn toggle_panel_hotkey(userdata: *mut c_void) {
    if panic::catch_unwind(|| {
        let id = panel_id_from_userdata(userdata);
        Sdk::get().toggle_overlay(id)
    })
    .is_err()
    {
        hlog_error!(target: "training-tracker", "toggle_panel_hotkey PANICKED");
    }
}

extern "C" fn toggle_all_hotkey(_userdata: *mut c_void) {
    if panic::catch_unwind(|| {
        let sdk = Sdk::get();
        let any_visible = constants::PANEL_IDS.iter().any(|id| sdk.overlay_visible(id));
        for id in constants::PANEL_IDS {
            sdk.set_overlay_visible(id, !any_visible);
        }
    })
    .is_err()
    {
        hlog_error!(target: "training-tracker", "toggle_all_hotkey PANICKED");
    }
}

fn panel_id_from_userdata(userdata: *mut c_void) -> &'static str {
    let ptr = userdata.cast::<u8>().cast_const();
    PANELS
        .iter()
        .find(|panel| panel.id.as_ptr() == ptr)
        .map(|panel| panel.id)
        .unwrap_or(constants::TRAINING_OVERLAY_ID)
}

extern "C" fn draw_energy_overlay(ui: *mut c_void, _userdata: *mut c_void) {
    // SAFETY: host passes its live `&mut egui::Ui` for this callback.
    let ui = unsafe { ui_from_ptr(ui) };
    if panic::catch_unwind(AssertUnwindSafe(|| {
        overlay::draw_energy_standalone(ui, career::draw_energy_panel)
    }))
    .is_err()
    {
        hlog_error!("draw_energy_overlay PANICKED");
    }
}

extern "C" fn draw_rank_overlay(ui: *mut c_void, _userdata: *mut c_void) {
    // SAFETY: host passes its live `&mut egui::Ui` for this callback.
    let ui = unsafe { ui_from_ptr(ui) };
    if panic::catch_unwind(AssertUnwindSafe(|| {
        overlay::draw_energy_standalone(ui, career::draw_rank_panel)
    }))
    .is_err()
    {
        hlog_error!("draw_rank_overlay PANICKED");
    }
}

extern "C" fn draw_training_overlay(ui: *mut c_void, _userdata: *mut c_void) {
    draw_overlay(
        ui,
        "training",
        constants::TRAINING_BASE_WIDTH,
        constants::TRAINING_FIXED_HEIGHT,
        false,
        career::draw_training_panel,
    );
}

extern "C" fn draw_bonds_overlay(ui: *mut c_void, _userdata: *mut c_void) {
    draw_overlay(
        ui,
        "bonds",
        constants::BONDS_BASE_WIDTH,
        constants::BONDS_FIXED_HEIGHT,
        false,
        career::draw_bonds_panel,
    );
}

extern "C" fn draw_scenario_overlay(ui: *mut c_void, _userdata: *mut c_void) {
    draw_overlay(
        ui,
        "scenario",
        constants::SCENARIO_BASE_WIDTH,
        constants::SCENARIO_FIXED_HEIGHT,
        false,
        scenario::draw,
    );
}

extern "C" fn draw_shop_overlay(ui: *mut c_void, _userdata: *mut c_void) {
    draw_overlay(
        ui,
        "shop",
        constants::SHOP_BASE_WIDTH,
        constants::SHOP_FIXED_HEIGHT,
        false,
        |ui, _| skill_shop_tab::draw(ui),
    );
}

fn draw_overlay(
    ui: *mut c_void,
    name: &str,
    base_width: f32,
    fixed_height: Option<f32>,
    chromeless: bool,
    body: PanelBody,
) {
    // SAFETY: host passes its live `&mut egui::Ui` for this callback.
    let ui = unsafe { ui_from_ptr(ui) };
    if panic::catch_unwind(AssertUnwindSafe(|| {
        overlay::draw_panel(ui, base_width, fixed_height, chromeless, body)
    }))
    .is_err()
    {
        hlog_error!("draw_{name}_overlay PANICKED");
    }
}

/// Render the Training panel directly into a caller-provided `Ui`. Used by the
/// desktop dev-harness to draw a representative tracker panel in eframe.
#[cfg(feature = "dev-harness")]
pub fn draw_overlay_for_harness(ui: &mut egui::Ui) {
    overlay::draw_panel(
        ui,
        constants::TRAINING_BASE_WIDTH,
        constants::TRAINING_FIXED_HEIGHT,
        false,
        career::draw_training_panel,
    );
}

/// Re-export so the dev-harness can point the texture loader at an on-disk icon
/// root (the `textures` submodule is otherwise private to `ui`).
#[allow(unused_imports)]
#[cfg(feature = "dev-harness")]
pub(crate) use textures::set_harness_icon_root;
