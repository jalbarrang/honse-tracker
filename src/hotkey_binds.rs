//! Default hotkey chords + config-file overrides.
//!
//! hachimi-redux had a host Hotkeys tab with persisted rebinds; edge has no such
//! tab, so the port shipped every hotkey unbound (0/0) — 5 of 6 panels were
//! unreachable. This module gives each action a sensible default chord and lets
//! the user override it in `honseTrackerConfig.json` under `"hotkeys"`:
//!
//! ```json
//! { "hotkeys": { "training-tracker.toggle_energy": { "mods": 4, "vk": 49 } } }
//! ```
//!
//! `mods` is a bitmask (Ctrl=1, Shift=2, Alt=4); `vk` is a Windows virtual-key
//! code (0 = unbound). Binds are applied at plugin init — restart the game after
//! editing the file.

use std::collections::BTreeMap;
use std::sync::Mutex;

use honse_services::{MOD_ALT, MOD_CTRL, MOD_SHIFT};
use serde::{Deserialize, Serialize};

/// One persisted key chord. Field layout mirrors redux's `HotkeyBind`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct HotkeyBind {
    /// Modifier bitmask: Ctrl=1, Shift=2, Alt=4.
    #[serde(default)]
    pub mods: u8,
    /// Windows virtual-key code of the primary key; 0 = unbound.
    #[serde(default)]
    pub vk: u16,
}

/// Default chords: Alt+digit for panels (game ignores Alt+digit), Alt+0 for
/// toggle-all, Alt+T for tracking. Order matches `ui::PANELS`.
pub const DEFAULTS: [(&str, HotkeyBind); 8] = [
    (
        "training-tracker.toggle_energy",
        HotkeyBind {
            mods: MOD_ALT,
            vk: 0x31,
        },
    ), // Alt+1
    (
        "training-tracker.toggle_training",
        HotkeyBind {
            mods: MOD_ALT,
            vk: 0x32,
        },
    ), // Alt+2
    (
        "training-tracker.toggle_bonds",
        HotkeyBind {
            mods: MOD_ALT,
            vk: 0x33,
        },
    ), // Alt+3
    (
        "training-tracker.toggle_scenario",
        HotkeyBind {
            mods: MOD_ALT,
            vk: 0x34,
        },
    ), // Alt+4
    (
        "training-tracker.toggle_shop",
        HotkeyBind {
            mods: MOD_ALT,
            vk: 0x35,
        },
    ), // Alt+5
    (
        "training-tracker.toggle_rank",
        HotkeyBind {
            mods: MOD_ALT,
            vk: 0x36,
        },
    ), // Alt+6
    (
        "training-tracker.toggle_all",
        HotkeyBind {
            mods: MOD_ALT,
            vk: 0x30,
        },
    ), // Alt+0
    (
        "training-tracker.toggle_tracking",
        HotkeyBind {
            mods: MOD_ALT,
            vk: 0x54,
        },
    ), // Alt+T
];

static BINDS: Mutex<BTreeMap<&'static str, HotkeyBind>> = Mutex::new(BTreeMap::new());

fn with_defaults(map: &mut BTreeMap<&'static str, HotkeyBind>) {
    if map.is_empty() {
        map.extend(DEFAULTS);
    }
}

/// The effective chord for an action id (config override, else default, else unbound).
#[must_use]
pub fn effective(id: &str) -> HotkeyBind {
    let mut binds = BINDS.lock().expect("hotkey binds lock poisoned");
    with_defaults(&mut binds);
    binds.get(id).copied().unwrap_or_default()
}

/// Apply config-file overrides on top of the defaults (unknown ids are ignored
/// with a warning so typos in the JSON are visible in the log).
pub fn apply_overrides(overrides: &BTreeMap<String, HotkeyBind>) {
    let mut binds = BINDS.lock().expect("hotkey binds lock poisoned");
    with_defaults(&mut binds);
    for (id, bind) in overrides {
        match DEFAULTS.iter().find(|(known, _)| known == id) {
            Some((known, _)) => {
                binds.insert(known, *bind);
            }
            None => {
                hlog_warn!(target: "training-tracker", "hotkeys config: unknown action id {id:?} ignored");
            }
        }
    }
}

/// Snapshot of every bind (defaults + overrides) for `config::persist`, so the
/// written JSON always lists all rebindable actions.
#[must_use]
pub fn all() -> BTreeMap<String, HotkeyBind> {
    let mut binds = BINDS.lock().expect("hotkey binds lock poisoned");
    with_defaults(&mut binds);
    binds.iter().map(|(id, bind)| ((*id).to_owned(), *bind)).collect()
}

/// Human-readable chord label, e.g. `"Alt+1"`, `"Ctrl+Shift+F2"`, `"Unbound"`.
#[must_use]
pub fn display(bind: HotkeyBind) -> String {
    if bind.vk == 0 {
        return "Unbound".to_owned();
    }
    let mut parts: Vec<&str> = Vec::new();
    if bind.mods & MOD_CTRL != 0 {
        parts.push("Ctrl");
    }
    if bind.mods & MOD_SHIFT != 0 {
        parts.push("Shift");
    }
    if bind.mods & MOD_ALT != 0 {
        parts.push("Alt");
    }
    let key = vk_name(bind.vk);
    let mut out = parts.join("+");
    if !out.is_empty() {
        out.push('+');
    }
    out.push_str(&key);
    out
}

fn vk_name(vk: u16) -> String {
    match vk {
        0x30..=0x39 => char::from(b'0' + (vk - 0x30) as u8).to_string(),
        0x41..=0x5A => char::from(b'A' + (vk - 0x41) as u8).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x20 => "Space".to_owned(),
        0x09 => "Tab".to_owned(),
        0x0D => "Enter".to_owned(),
        0x21 => "PageUp".to_owned(),
        0x22 => "PageDown".to_owned(),
        0x23 => "End".to_owned(),
        0x24 => "Home".to_owned(),
        0x25 => "Left".to_owned(),
        0x26 => "Up".to_owned(),
        0x27 => "Right".to_owned(),
        0x28 => "Down".to_owned(),
        0x2D => "Insert".to_owned(),
        0x2E => "Delete".to_owned(),
        other => format!("VK{other:#04X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_all_panel_actions_and_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (id, bind) in DEFAULTS {
            assert!(bind.vk != 0, "{id} must have a bound default");
            assert!(seen.insert((bind.mods, bind.vk)), "duplicate default chord for {id}");
        }
    }

    #[test]
    fn override_applies_and_unknown_id_ignored() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "training-tracker.toggle_energy".to_owned(),
            HotkeyBind {
                mods: MOD_CTRL,
                vk: 0x70,
            },
        );
        overrides.insert(
            "training-tracker.no_such_action".to_owned(),
            HotkeyBind { mods: 0, vk: 1 },
        );
        apply_overrides(&overrides);
        assert_eq!(
            effective("training-tracker.toggle_energy"),
            HotkeyBind {
                mods: MOD_CTRL,
                vk: 0x70
            }
        );
        // Unknown id is not inserted.
        assert_eq!(effective("training-tracker.no_such_action"), HotkeyBind::default());
        // Untouched action keeps its default.
        assert_eq!(
            effective("training-tracker.toggle_rank"),
            HotkeyBind {
                mods: MOD_ALT,
                vk: 0x36
            }
        );
    }

    #[test]
    fn display_labels() {
        assert_eq!(
            display(HotkeyBind {
                mods: MOD_ALT,
                vk: 0x31
            }),
            "Alt+1"
        );
        assert_eq!(
            display(HotkeyBind {
                mods: MOD_CTRL | MOD_SHIFT,
                vk: 0x71
            }),
            "Ctrl+Shift+F2"
        );
        assert_eq!(display(HotkeyBind { mods: 0, vk: 0 }), "Unbound");
        assert_eq!(display(HotkeyBind { mods: 0, vk: 0x54 }), "T");
    }
}
