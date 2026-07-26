//! Plugin lifecycle: `declare_plugin!` + services init + game-initialized hooks.

use std::ffi::c_void;

use edge_sdk::declare_plugin;
use serde::{Deserialize, Serialize};

use crate::compat::Sdk;
use crate::{command_hooks, config, gametora_data, hooks, shop_hooks, ui};

/// On-disk plugin config (`honseTrackerConfig.json` under edge base dir).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HonseTrackerFile {
    /// Tracker feature settings (build profile, recommend, planner, zoom).
    #[serde(flatten)]
    tracker: config::PersistedConfigPublic,
    /// Hosted-data URL overrides.
    #[serde(default)]
    hosted_data: honse_services::HostedDataUrls,
}

declare_plugin! {
    fn init() -> bool {
        plugin_init()
    }
}

fn plugin_init() -> bool {
    hlog_info!(
        target: "training-tracker",
        "Training Tracker (edge plugin) v{} initializing",
        env!("CARGO_PKG_VERSION")
    );

    // (1) Load tracker config via PluginConfig as honseTrackerConfig.json.
    // Falls back to legacy training_config.json fields via flatten defaults.
    let file_cfg = honse_services::PluginConfig::<HonseTrackerFile>::load("honseTrackerConfig.json");
    if let Some(ref cfg) = file_cfg {
        config::apply_persisted(&cfg.value.tracker);
    } else {
        // Sdk not ready for base_dir — try legacy path.
        config::load();
    }

    // Telemetry (fork gating: disabled unless telemetry.json enables it).
    let sdk = Sdk::get();
    hachimi_telemetry::init(sdk.host_data_path("telemetry.json"));

    // (2) Services init: frame source (drives the self-hosted overlay), the
    // game-ready bootstrap, and overlay layout persistence. Must run BEFORE
    // register_ui so saved panel/window positions are loaded first.
    honse_services::init(honse_services::InitOptions {
        overlay_layout_file: Some("honseTrackerLayout.json".to_owned()),
    });

    // Surface registrations (tabs / panels / hotkeys) — no IL2CPP required.
    ui::register_ui();

    // Event subscriptions (FRAME / VIEW_CHANGE / SHUTDOWN).
    hooks::subscribe_events();

    // Tracker IL2CPP hooks + hosted-data sync once the game runtime is ready.
    // Uses honse-services' present-driven game-ready signal, NOT edge's
    // register_on_game_initialized (which never fires for load_libraries plugins
    // when ui_scale==1.0 — see honse_services::init docs).
    honse_services::register_on_game_ready(on_game_initialized, std::ptr::null_mut());

    // Warm GameTora catalog off-thread (may be empty until sync completes).
    std::thread::spawn(|| {
        if gametora_data::is_available() {
            hlog_info!(target: "training-tracker", "GameTora catalog ready");
        } else {
            hlog_warn!(
                target: "training-tracker",
                "GameTora catalog unavailable (no cache yet)"
            );
        }
    });

    hlog_info!(target: "training-tracker", "Training Tracker ready");
    sdk.show_notification("Training Tracker loaded!");
    true
}

/// Install IL2CPP hooks + kick hosted-data sync once the game runtime is up.
///
/// Order matches the fork: shop visibility hooks, then command-suspend hooks.
/// View hook is installed by `honse_services::init`'s own callback.
unsafe extern "C" fn on_game_initialized(_userdata: *mut c_void) {
    if shop_hooks::try_install_shop_hooks() {
        hlog_info!(target: "training-tracker", "Skill shop visibility hooks installed");
    }
    if command_hooks::install() {
        hlog_info!(target: "training-tracker", "Command-suspend hooks installed");
    }

    // (3) Hosted-data sync_all on a background thread post-game-initialized.
    let urls = honse_services::PluginConfig::<HonseTrackerFile>::load("honseTrackerConfig.json")
        .map(|c| c.value.hosted_data)
        .unwrap_or_default();
    std::thread::spawn(move || {
        honse_services::sync_all_from_config(&urls, true);
        // Icons finished — drop negative cache so the Career panel picks them up.
        crate::clear_icon_cache();
    });
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
