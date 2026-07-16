//! Unified on-disk plugin config (`hachimi/training_config.json`, next to
//! Hachimi's own `config.json`).
//!
//! This module owns the persisted struct and the load/persist path. Each feature
//! module keeps its own in-memory state and exposes a getter/setter; `config`
//! bridges those to disk so there is a single source of truth for the file format:
//! - [`crate::build_profile`] — active build profile (objective, per-stat
//!   targets, weights, course/strategy) + saved custom profiles
//! - [`crate::recommend`] — smart-recommendation tuning params
//!
//! Back-compat: every field is `#[serde(default)]`, so older configs (and configs
//! written by older plugin versions) load fine with sensible defaults. The legacy
//! flat `stat_targets` field is kept for one-way migration into the default
//! profile's `per_stat_target`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::hotkey_binds::{self, HotkeyBind};
use crate::{build_profile, overlay_prefs, planner, recommend};
use build_profile::BuildProfile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedConfigPublic {
    /// Legacy per-stat targets; migrated into the active profile when no
    /// `build_profile` is present (older configs).
    #[serde(default)]
    pub stat_targets: [i32; 5],
    /// Legacy overlay tab bitmask; ignored since tracker readouts are independent panels.
    #[serde(default, skip_serializing)]
    pub enabled_tabs: u8,
    #[serde(default)]
    pub recommend: recommend::RecommendParams,
    #[serde(default)]
    pub planner: planner::PlannerParams,
    #[serde(default = "overlay_prefs::default_zoom")]
    pub overlay_zoom: f32,
    /// The active build profile (objective + targets + weights + course/strategy).
    #[serde(default)]
    pub build_profile: Option<BuildProfile>,
    /// Legacy user-saved custom profiles. The save/load UI was removed; the field
    /// is kept (ignored) so older config files still deserialize cleanly.
    #[serde(default, skip_serializing)]
    pub saved_profiles: Vec<BuildProfile>,
    /// Hotkey chord overrides keyed by action id (see [`crate::hotkey_binds`]).
    /// Persist writes the full effective map so every rebindable action is
    /// visible in the JSON; missing/empty means "use the built-in defaults".
    #[serde(default)]
    pub hotkeys: BTreeMap<String, HotkeyBind>,
}

impl Default for PersistedConfigPublic {
    fn default() -> Self {
        Self {
            stat_targets: [0; 5],
            enabled_tabs: 0,
            recommend: recommend::RecommendParams::default(),
            planner: planner::PlannerParams::default(),
            overlay_zoom: overlay_prefs::default_zoom(),
            build_profile: None,
            saved_profiles: Vec::new(),
            hotkeys: BTreeMap::new(),
        }
    }
}

type PersistedConfig = PersistedConfigPublic;

fn config_path() -> std::path::PathBuf {
    // Prefer edge data path when available; fall back to exe-relative legacy path.
    if let Some(p) = crate::compat::Sdk::get().host_data_path("training_config.json") {
        return p;
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("hachimi").join("training_config.json")))
        .unwrap_or_else(|| std::path::PathBuf::from("training_config.json"))
}

/// Apply a deserialized config into the feature modules (used by PluginConfig load).
pub fn apply_persisted(cfg: &PersistedConfigPublic) {
    let active = cfg.build_profile.clone().unwrap_or_else(|| BuildProfile {
        per_stat_target: cfg.stat_targets,
        ..BuildProfile::default()
    });
    build_profile::set_active(active);
    let _ = &cfg.saved_profiles;
    let _ = cfg.enabled_tabs;
    recommend::set_params(cfg.recommend);
    planner::set_params(cfg.planner);
    overlay_prefs::set_zoom(cfg.overlay_zoom);
    hotkey_binds::apply_overrides(&cfg.hotkeys);
    let p = build_profile::active();
    hlog_info!(
        target: "training-tracker",
        "config applied: profile={:?} objective={:?} targets={:?}",
        p.name,
        p.objective,
        p.per_stat_target
    );
}

/// Load persisted config into the feature modules. Call once on plugin init.
/// A missing or invalid file leaves every module at its defaults.
pub fn load() {
    let path = config_path();
    let cfg = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<PersistedConfig>(&bytes) {
            Ok(cfg) => cfg,
            Err(e) => {
                hlog_warn!(target: "training-tracker", "training config parse failed: {e}");
                return;
            }
        },
        Err(_) => return,
    };
    apply_persisted(&cfg);
}

/// Gather the current state from every feature module and write it to disk.
/// Call when the user commits a settings edit.
pub fn persist() {
    let active = build_profile::active();
    let cfg = PersistedConfig {
        // Mirror the active targets into the legacy field for forward-compat.
        stat_targets: active.per_stat_target,
        enabled_tabs: 0,
        recommend: recommend::params(),
        planner: planner::params(),
        overlay_zoom: overlay_prefs::zoom(),
        build_profile: Some(active),
        saved_profiles: Vec::new(),
        hotkeys: hotkey_binds::all(),
    };
    let Ok(bytes) = serde_json::to_vec_pretty(&cfg) else {
        return;
    };
    let path = config_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, bytes) {
        hlog_warn!(target: "training-tracker", "config persist failed: {e}");
    }
}
