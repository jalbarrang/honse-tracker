//! Self-hosted services that upstream hachimi-edge does not expose to plugins.
//!
//! Layering: this crate depends on `edge-sdk`. `edge-sdk` must never depend on
//! this crate (hiker law `sdk_depends_on_services`).

pub mod config;
pub mod event;
pub mod events;
pub mod frame;
pub mod hosted_data;
pub mod hotkeys;
pub mod init;
pub mod scene_views;
pub mod surface;
pub mod view_hook;

pub use config::{HostedDataUrls, PluginConfig};
pub use event::{EventFn, ViewChangeEvent, FRAME, SHUTDOWN, VIEW_CHANGE};
pub use events::{dispatch, dispatch_shutdown, dispatch_view_change, off, on};
pub use frame::{install_frame_source, register_frame_job, FrameJob};
pub use hosted_data::{gametora_data_dir, host_data_path, sync_all};
pub use hotkeys::{register_hotkey, Chord, MOD_ALT, MOD_CTRL, MOD_SHIFT};
pub use init::{init, is_game_ready, register_on_game_ready, GameReadyCallback, InitOptions};
pub use scene_views::view_name;
pub use surface::{
    overlay_set_visible, overlay_visible, register_menu_section, register_menu_section_with_icon, register_overlay,
    set_surface_title,
    register_page, register_page_with_icon, register_panel, register_panel_chromeless, register_panel_chromeless_fixed,
    register_tab, set_overlay_visible, toggle_overlay, Surface,
};
pub use view_hook::{install_view_poll, poll_view_change, set_view_poll_enabled};

/// Unregister a handle from hotkeys and/or surface registries (shared handle space).
pub fn unregister(handle: u64) -> bool {
    hotkeys::unregister(handle)
}

/// Sync all hosted-data sets using URL overrides from a [`HostedDataUrls`] config block.
pub fn sync_all_from_config(urls: &HostedDataUrls, notify: bool) {
    sync_all(urls.as_overrides(), notify);
}

use std::sync::atomic::{AtomicU64, Ordering};

/// Shared monotonic handle allocator for event subscriptions and (later) GUI/hotkey regs.
/// `0` is reserved for failure / unused.
static HANDLE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Allocate a fresh non-zero registration handle.
#[must_use]
pub fn next_handle() -> u64 {
    HANDLE_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) static TEST_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
