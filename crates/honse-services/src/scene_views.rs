//! Single source of truth for `Gallop.SceneDefine.ViewId` labels.
//!
//! Documentation/diagnostics only — these names come from manual in-game
//! observation on the Windows/Steam build and do not classify gameplay state.
//! Copied verbatim from fork `apps/hachimi/src/core/scene_views.rs`.

use std::ffi::CStr;

/// Known view-id → label pairs. NUL-terminated literals so the host can hand a
/// stable `'static` pointer straight to plugins over FFI.
const KNOWN_VIEW_IDS: &[(i32, &CStr)] = &[
    (1, c"Launch (disclaimer)"),
    (2, c"Start View (Press Start)"),
    (101, c"Home View"),
    (400, c"Race Playback"),
    (1100, c"Intermission (turns-left)"),
    (1101, c"Career View (training loop)"),
    (1200, c"Paddock View (pre-race)"),
];

/// Host-owned label for a known view id as a C string, if catalogued.
#[must_use]
pub fn view_name_cstr(view_id: i32) -> Option<&'static CStr> {
    KNOWN_VIEW_IDS
        .iter()
        .find(|(id, _)| *id == view_id)
        .map(|(_, name)| *name)
}

/// Host-owned label for a known view id, if catalogued.
#[must_use]
pub fn view_name(view_id: i32) -> Option<&'static str> {
    view_name_cstr(view_id).and_then(|c| c.to_str().ok())
}

#[cfg(test)]
mod tests {
    use super::{view_name, view_name_cstr};

    #[test]
    fn known_ids_resolve() {
        assert_eq!(view_name(1), Some("Launch (disclaimer)"));
        assert_eq!(view_name(400), Some("Race Playback"));
        assert_eq!(view_name(1101), Some("Career View (training loop)"));
    }

    #[test]
    fn unknown_ids_are_none() {
        assert!(view_name(0).is_none());
        assert!(view_name_cstr(424_242).is_none());
    }
}
