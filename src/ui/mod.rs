//! GUI rendering on the plugin's own egui context (`honse_services::overlay`).
//!
//! Panels are chromeless, draggable, fully window-independent Areas; the
//! Tracker config UI is a decorated window with a real close button, reopened
//! from the hachimi menu ("Show Honse Tracker"). Nothing here touches host
//! egui — no ABI lockstep applies to this UI.

use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};

use crate::compat::{egui, Sdk};
use honse_services::overlay;

mod career;
mod constants;
mod dimens;
// Race-condition icon toggles (weather/season/time). Currently hidden from the
// UI per product decision; kept dormant so it can be re-enabled cheaply.
#[allow(dead_code)]
mod icons;
mod menu;
mod overlay_panels;
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

pub(crate) struct TrackerPanel {
    pub(crate) id: &'static str,
    pub(crate) hotkey_id: &'static str,
    pub(crate) label: &'static str,
    callback: fn(&mut egui::Ui),
}

pub(crate) const PANELS: [TrackerPanel; 6] = [
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

/// Register the plugin's UI components on the self-hosted overlay.
pub fn register_ui() {
    let sdk = Sdk::get();

    // Tracker config window: decorated, user-closable ([X] is respected), and
    // reopenable via the auto-registered "Show Honse Tracker" hachimi-menu item.
    overlay::register_window("tracker_config", "Honse Tracker", |ui| {
        if panic::catch_unwind(AssertUnwindSafe(|| menu::draw(ui))).is_err() {
            hlog_error!("training-tracker config window draw PANICKED");
        }
    });

    for panel in PANELS {
        // Chromeless panel on our own context — completely independent of any
        // window; toggling it never opens anything else.
        let body = panel.callback;
        let panel_label = panel.label;
        overlay::register_panel(panel.id, move |ui| {
            if panic::catch_unwind(AssertUnwindSafe(|| body(ui))).is_err() {
                hlog_error!("draw_{panel_label}_overlay PANICKED");
            }
        });

        // Default chord (or the user's honseTrackerConfig.json override) —
        // config is loaded before register_ui in plugin_init.
        let bind = crate::hotkey_binds::effective(panel.hotkey_id);
        let hotkey_label = format!("Toggle {} Panel", panel.label);
        sdk.register_hotkey(
            panel.hotkey_id,
            &hotkey_label,
            bind.mods,
            bind.vk,
            toggle_panel_hotkey,
            panel.id.as_ptr().cast_mut().cast(),
        );
    }

    let bind = crate::hotkey_binds::effective("training-tracker.toggle_all");
    sdk.register_hotkey(
        "training-tracker.toggle_all",
        "Toggle All Tracker Panels",
        bind.mods,
        bind.vk,
        toggle_all_hotkey,
        std::ptr::null_mut(),
    );

    let bind = crate::hotkey_binds::effective("training-tracker.toggle_tracking");
    sdk.register_hotkey(
        "training-tracker.toggle_tracking",
        "Start/Stop Tracking",
        bind.mods,
        bind.vk,
        toggle_tracking_hotkey,
        std::ptr::null_mut(),
    );

    hlog_info!(target: "training-tracker", "UI registered (config window + 6 own-context panels)");
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
        overlay::toggle(id)
    })
    .is_err()
    {
        hlog_error!(target: "training-tracker", "toggle_panel_hotkey PANICKED");
    }
}

extern "C" fn toggle_all_hotkey(_userdata: *mut c_void) {
    if panic::catch_unwind(|| {
        let any_visible = constants::PANEL_IDS.iter().any(|id| overlay::is_visible(id));
        for id in constants::PANEL_IDS {
            overlay::set_visible(id, !any_visible);
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

fn draw_energy_overlay(ui: &mut egui::Ui) {
    overlay_panels::draw_energy_standalone(ui, career::draw_energy_panel);
}

fn draw_rank_overlay(ui: &mut egui::Ui) {
    overlay_panels::draw_energy_standalone(ui, career::draw_rank_panel);
}

fn draw_training_overlay(ui: &mut egui::Ui) {
    overlay_panels::draw_panel(
        ui,
        constants::TRAINING_BASE_WIDTH,
        constants::TRAINING_FIXED_HEIGHT,
        false,
        career::draw_training_panel,
    );
}

fn draw_bonds_overlay(ui: &mut egui::Ui) {
    overlay_panels::draw_panel(
        ui,
        constants::BONDS_BASE_WIDTH,
        constants::BONDS_FIXED_HEIGHT,
        false,
        career::draw_bonds_panel,
    );
}

fn draw_scenario_overlay(ui: &mut egui::Ui) {
    overlay_panels::draw_panel(
        ui,
        constants::SCENARIO_BASE_WIDTH,
        constants::SCENARIO_FIXED_HEIGHT,
        false,
        scenario::draw,
    );
}

fn draw_shop_overlay(ui: &mut egui::Ui) {
    overlay_panels::draw_panel(
        ui,
        constants::SHOP_BASE_WIDTH,
        constants::SHOP_FIXED_HEIGHT,
        false,
        |ui, _| skill_shop_tab::draw(ui),
    );
}

/// Render the Training panel directly into a caller-provided `Ui`. Used by the
/// desktop dev-harness to draw a representative tracker panel in eframe.
#[cfg(feature = "dev-harness")]
pub fn draw_overlay_for_harness(ui: &mut egui::Ui) {
    overlay_panels::draw_panel(
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
