//! Plugin init orchestrator: game-initialized → hooks + frame source.

use std::ffi::c_void;

use edge_sdk::Sdk;

use crate::{frame::install_frame_source, view_hook::install_view_hook};

/// Options for [`init`]. Reserved for future toggles; currently empty.
#[derive(Debug, Clone, Default)]
pub struct InitOptions {}

/// Register the game-initialized callback that installs the view hook and frame source.
///
/// Plugins call this from their `init()` after `Api::init` / edge entry setup.
/// Idempotent at the registration level; the deferred installers are themselves
/// idempotent.
pub fn init(_opts: InitOptions) {
    let Some(sdk) = Sdk::try_get() else {
        log::warn!("honse-services::init called before Sdk init");
        return;
    };
    if !sdk.register_on_game_initialized(on_game_initialized, std::ptr::null_mut()) {
        log::warn!("honse-services: hachimi_register_on_game_initialized failed");
    }
}

/// # Safety
/// Called by the host after IL2CPP is ready; `userdata` is unused (null).
unsafe extern "C" fn on_game_initialized(_userdata: *mut c_void) {
    install_view_hook();
    install_frame_source();
}
