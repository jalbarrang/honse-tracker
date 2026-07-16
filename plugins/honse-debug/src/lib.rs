//! Debug Viewer Plugin (development only)
//!
//! Records game view transitions and is intended to grow into a live feed of
//! debug values during development. Off by default — enable it manually via
//! the host's plugin load list.

#[macro_use]
extern crate edge_sdk;

mod hooks;
#[cfg(windows)]
mod own_overlay;
mod state;
mod ui;

use edge_sdk::declare_plugin;

declare_plugin! {
    fn init() -> bool {
        plugin_init()
    }
}

fn plugin_init() -> bool {
    hlog_info!(
        target: "debug-viewer",
        "Debug Viewer v{} initializing",
        env!("CARGO_PKG_VERSION")
    );

    state::init();

    // Install view hook + frame source so VIEW_CHANGE / FRAME events fire.
    // Must run BEFORE register_ui: it names this plugin's surface window and
    // "Show <title>" host-menu item, which register_ui creates.
    honse_services::init(honse_services::InitOptions {
        surface_title: Some("Honse Debug".to_owned()),
    });

    ui::register_ui();

    // Capability checks deleted: single-version world — EVENTS always available.
    let _ = hooks::subscribe_events();

    // SPIKE (self-hosted-overlay initiative): own egui + DX11 renderer driven
    // from the edge present callback. Alt+9 toggles the demo window.
    #[cfg(windows)]
    own_overlay::install();

    hlog_info!(target: "debug-viewer", "Debug Viewer ready");
    if let Some(sdk) = edge_sdk::Sdk::try_get() {
        sdk.show_notification("Debug Viewer loaded");
    }

    true
}

/// Windows `DllMain`: on detach, dispatch SHUTDOWN so state resets before unload.
///
/// # Safety
///
/// Called only by the Windows loader with valid arguments; must not be called from user code. Runs under the loader lock, and `dispatch_shutdown` is loader-lock-safe (no thread joins or DLL loads).
#[cfg(target_os = "windows")]
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: *mut std::ffi::c_void,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> i32 {
    const DLL_PROCESS_DETACH: u32 = 0;
    if reason == DLL_PROCESS_DETACH {
        own_overlay::uninstall_wndproc(None);
        honse_services::dispatch_shutdown();
    }
    1
}
