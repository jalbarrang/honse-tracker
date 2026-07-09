//! Host→plugin event ids and payload structs.
//!
//! Copied from fork `hachimi-plugin-abi` event module so tracker code ports
//! without depending on the fork ABI crate (hiker law `fork_abi_import`).

use std::ffi::c_void;

/// Plugin event callback shape from the fork ABI (`PluginEventFn`).
pub type EventFn = extern "C" fn(event_id: u32, data: *const c_void, userdata: *mut c_void);

/// Fired once per rendered frame on the render thread. `data` is null.
pub const FRAME: u32 = 1;
/// Fired before the host unloads (process detach). `data` is null.
pub const SHUTDOWN: u32 = 3;
/// Fired when the game changes view/scene. `data` → [`ViewChangeEvent`].
pub const VIEW_CHANGE: u32 = 4;

/// Payload for [`VIEW_CHANGE`]. Valid for the callback duration only.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ViewChangeEvent {
    /// The game's next view id (`Gallop.ViewId`). `1` is the splash view.
    pub view_id: i32,
}
