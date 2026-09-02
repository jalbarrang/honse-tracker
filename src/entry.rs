//! Plugin lifecycle: `declare_plugin!` + services init + game-initialized hooks.

use std::ffi::c_void;
use std::sync::Mutex;

use edge_sdk::declare_plugin;
use honse_services::PluginConfig;
use serde::{Deserialize, Serialize};

use crate::compat::Sdk;
use crate::{apply_hooks, class_dump, command_hooks, gametora_data, hooks, idle_export, race_cutin};

/// On-disk plugin config (`honseTrackerConfig.json` under edge base dir).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HonseTrackerFile {
    /// Hosted-data URL overrides.
    #[serde(default)]
    hosted_data: honse_services::HostedDataUrls,
    /// Drop race skill cut-ins so the race never stops for them. Off unless
    /// asked for: it is the one setting here that changes the game rather than
    /// reporting on it.
    #[serde(default)]
    skip_race_skill_cutins: bool,
    /// Write each finished Independent Training's server response to disk. On
    /// by default: it only reports, and the data is gone once the game has
    /// shown you its summary screen.
    #[serde(default = "default_true")]
    save_idle_careers: bool,
    /// Where those files go. Empty means
    /// `%USERPROFILE%\Documents\SavedIdleCareers`; a relative path resolves
    /// under the user profile, never under the game folder.
    #[serde(default)]
    idle_career_dir: String,
}

/// serde needs a function for a non-`false` bool default.
const fn default_true() -> bool {
    true
}

impl Default for HonseTrackerFile {
    fn default() -> Self {
        Self {
            hosted_data: honse_services::HostedDataUrls::default(),
            skip_race_skill_cutins: false,
            save_idle_careers: true,
            idle_career_dir: String::new(),
        }
    }
}

/// The loaded config, kept so a menu toggle can write back to the same file it
/// was read from.
static CONFIG: Mutex<Option<PluginConfig<HonseTrackerFile>>> = Mutex::new(None);

/// The config as loaded, or defaults when there is no base dir to read from.
fn config_value() -> HonseTrackerFile {
    CONFIG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(|c| c.value.clone())
        .unwrap_or_default()
}

/// Turn Independent Training export on or off and remember it.
///
/// Saves what actually took: asked to turn it on with no hooks installed, the
/// answer is no, and a config claiming otherwise would survive a restart as a
/// lie about where the player's data is going.
fn set_idle_export(enabled: bool) {
    crate::idle_export::set_enabled(enabled);
    let mut guard = CONFIG.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(config) = guard.as_mut() else {
        return;
    };
    config.value.save_idle_careers = crate::idle_export::is_enabled();
    if let Err(e) = config.save() {
        hlog_warn!(target: "training-tracker", "Could not save honseTrackerConfig.json: {e}");
    }
}

/// Turn cut-in skipping on or off and remember it.
///
/// Saves what actually took, not what was asked for: turning it on can fail if
/// the hook will not install, and a config claiming otherwise would be a lie
/// that survives a restart.
fn set_cutin_skip(enabled: bool) {
    race_cutin::set_enabled(enabled);
    let mut guard = CONFIG.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(config) = guard.as_mut() else {
        return;
    };
    config.value.skip_race_skill_cutins = race_cutin::is_enabled();
    if let Err(e) = config.save() {
        hlog_warn!(target: "training-tracker", "Could not save honseTrackerConfig.json: {e}");
    }
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

    // Telemetry (fork gating: disabled unless telemetry.json enables it).
    // Misconfiguration fails visibly-disabled: the DLL never posts requests the
    // sidecar would reject forever, and the token itself is never logged.
    let sdk = Sdk::get();
    let telem_path = sdk.host_data_path("telemetry.json").or_else(|| {
        // Fallback: hachimi_get_data_path may not be exposed by this host version.
        // Try base_dir (the hachimi/ folder next to the game exe).
        let fallback = edge_sdk::Sdk::get().base_dir()?.join("telemetry.json");
        if fallback.exists() {
            hlog_info!(target: "training-tracker", "telemetry.json found via base_dir fallback: {}", fallback.display());
            Some(fallback)
        } else {
            hlog_warn!(target: "training-tracker", "telemetry.json not found at data_path (unavailable) or base_dir ({})", fallback.display());
            None
        }
    });
    hlog_info!(target: "training-tracker", "Telemetry config path: {:?}", telem_path);
    match hachimi_telemetry::init(telem_path) {
        hachimi_telemetry::InitOutcome::Disabled => {
            hlog_info!(target: "training-tracker", "Telemetry disabled (no telemetry.json or enabled=false)");
        }
        hachimi_telemetry::InitOutcome::Enabled { endpoint } => {
            hlog_info!(target: "training-tracker", "Telemetry enabled \u{2192} {endpoint}");
        }
        hachimi_telemetry::InitOutcome::InvalidEndpoint(url) => {
            hlog_error!(
                target: "training-tracker",
                "Telemetry stays OFF: telemetry.json endpoint is unusable: {url}"
            );
        }
        hachimi_telemetry::InitOutcome::MissingToken(tried) => {
            hlog_error!(
                target: "training-tracker",
                "Telemetry stays OFF: no auth token readable at {tried}"
            );
            sdk.show_notification("Training Tracker: telemetry off \u{2014} auth token unavailable");
        }
    }

    // Read before anything consults it. Hooks that depend on IL2CPP are
    // applied later, at game-ready; this only decides what to apply.
    *CONFIG.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
        PluginConfig::<HonseTrackerFile>::load("honseTrackerConfig.json");

    // (1) Services init: frame source and the game-ready bootstrap.
    honse_services::init(honse_services::InitOptions);

    // Overlay panels. Registration only — the first paint happens on the
    // first present, and each panel decides for itself whether it has
    // anything true to draw.
    crate::ui::install();

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

    // Register Hachimi menu button for IL2CPP class dump.
    edge_sdk::gui::register_menu_item("Dump IL2CPP classes", || {
        hlog_info!(target: "training-tracker", "IL2CPP class dump requested");
        std::thread::spawn(|| {
            class_dump::dump_all_classes();
        });
    });

    // Toggle for the screen/debug readout. `register_menu_item` is label +
    // callback only, so it carries no egui types across the boundary and none of
    // the ABI-lockstep rules apply.
    edge_sdk::gui::register_menu_item("Toggle debug overlay", || {
        crate::ui::debug::toggle();
    });

    edge_sdk::gui::register_menu_item("Toggle Independent Training timer", || {
        crate::ui::idle::toggle();
    });

    // Where the exported runs go is the part worth being able to check without
    // opening the config file, so the toggle logs the directory too.
    edge_sdk::gui::register_menu_item("Toggle Independent Training export", || {
        set_idle_export(!crate::idle_export::is_enabled());
        hlog_info!(
            target: "training-tracker",
            "Idle career export directory: {}",
            crate::idle_export::output_dir().display()
        );
    });

    // Skipping race cut-ins is a setting, not a hotkey: it is something you
    // decide once, and the menu is where the game's own equivalent lives.
    edge_sdk::gui::register_menu_item("Toggle race cut-in skip", || {
        set_cutin_skip(!race_cutin::is_enabled());
    });

    hlog_info!(target: "training-tracker", "Training Tracker ready");
    sdk.show_notification("Training Tracker loaded!");
    true
}

/// Install IL2CPP hooks + kick hosted-data sync once the game runtime is up.
///
/// The view hook is installed by `honse_services::init`'s own callback.
unsafe extern "C" fn on_game_initialized(_userdata: *mut c_void) {
    if command_hooks::install() {
        hlog_info!(target: "training-tracker", "Career lifecycle hooks installed");
    }
    if apply_hooks::install() {
        hlog_info!(target: "training-tracker", "Apply response hooks installed");
    }

    let config = config_value();

    // Hooks first, then the flag: `configure` refuses to enable an export that
    // has nowhere to come from, so it has to know whether the hooks took.
    if idle_export::install() {
        hlog_info!(target: "training-tracker", "Idle career export hooks installed");
    }
    idle_export::configure(config.save_idle_careers, Some(config.idle_career_dir.as_str()));
    if idle_export::is_enabled() {
        hlog_info!(
            target: "training-tracker",
            "Idle careers will be saved to {}",
            idle_export::output_dir().display()
        );
    }

    // Only if asked for. `set_enabled` installs the hook on first use, so a
    // config that leaves this off never patches the game at all.
    if config.skip_race_skill_cutins {
        race_cutin::set_enabled(true);
    }

    // (2) Hosted-data sync_all on a background thread post-game-initialized.
    let urls = config.hosted_data;
    std::thread::spawn(move || {
        honse_services::sync_all_from_config(&urls, true);
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
