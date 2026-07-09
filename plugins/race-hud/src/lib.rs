//! Race HUD Plugin
//!
//! Surfaces a live per-runner heads-up display during races. It captures the race
//! SimData and decodes it; the live per-runner feed is built on top of the decoded
//! frames.

#![allow(
    clippy::as_underscore,
    clippy::fn_to_numeric_cast,
    clippy::fn_to_numeric_cast_any,
    clippy::ptr_as_ptr,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::needless_pass_by_value,
    clippy::missing_safety_doc,
    clippy::missing_transmute_annotations,
    clippy::useless_transmute,
    clippy::transmute_undefined_repr,
    clippy::type_complexity,
    clippy::len_without_is_empty,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::module_name_repetitions,
    clippy::too_many_arguments,
    clippy::wildcard_imports,
    clippy::cast_lossless,
    clippy::used_underscore_binding,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::doc_markdown,
    clippy::undocumented_unsafe_blocks,
    unnecessary_transmutes,
    function_casts_as_integer
)]

#[macro_use]
extern crate edge_sdk;

mod capture;
mod settings;
mod sim;
mod state;
mod telemetry;
mod tick;
mod ui;

use std::ffi::c_void;

use edge_sdk::declare_plugin;

declare_plugin! {
    fn init() -> bool {
        plugin_init()
    }
}

fn plugin_init() -> bool {
    hlog_info!(
        target: "race-hud",
        "Race HUD v{} initializing",
        env!("CARGO_PKG_VERSION")
    );

    state::init();
    settings::load();

    // Side-channel telemetry (default disabled via telemetry.json).
    if let Some(sdk) = edge_sdk::Sdk::try_get() {
        hachimi_telemetry::init(sdk.data_path("telemetry.json"));
    } else {
        hachimi_telemetry::init(None);
    }

    ui::register_ui();

    // Install view hook + frame source (surface window renders every frame).
    honse_services::init(honse_services::InitOptions::default());

    // Capability checks deleted: single-version world — EVENTS always available.
    let _ = tick::subscribe_events();

    // IL2CPP race-scene hooks require the game runtime — install on game-initialized.
    if let Some(edge) = edge_sdk::Sdk::try_get() {
        let _ = edge.register_on_game_initialized(on_game_initialized, std::ptr::null_mut());
    }

    hlog_info!(target: "race-hud", "Race HUD ready");
    if let Some(sdk) = edge_sdk::Sdk::try_get() {
        sdk.show_notification("Race HUD loaded");
    }

    true
}

/// Install SimData / live-feed IL2CPP hooks once the game runtime is up.
unsafe extern "C" fn on_game_initialized(_userdata: *mut c_void) {
    if !capture::install() {
        hlog_warn!(
            target: "race-hud",
            "SimData capture hook not installed; overlay will stay idle"
        );
    }
}

/// Windows `DllMain`: on detach, dispatch SHUTDOWN so hooks unhook before unload.
#[cfg(target_os = "windows")]
#[no_mangle]
pub unsafe extern "system" fn DllMain(_hinst: *mut c_void, reason: u32, _reserved: *mut c_void) -> i32 {
    const DLL_PROCESS_DETACH: u32 = 0;
    if reason == DLL_PROCESS_DETACH {
        honse_services::dispatch_shutdown();
    }
    1
}
