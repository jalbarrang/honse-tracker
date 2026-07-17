//! Plugin bootstrap: present-driven "game ready" signal + frame source.
//!
//! # Why not `hachimi_register_on_game_initialized`?
//!
//! Edge fires `GameSystem::on_game_initialized()` (which drains the plugin
//! game-init callback list) from `on_hooking_finished` **before** it calls
//! `plugin.init()` on `load_libraries` plugins — and the registration function
//! only pushes to a Vec, it never calls a late registrant. The only other
//! trigger (`InitializeGame_MoveNext`) is armed solely when `ui_scale != 1.0`.
//! So with the default `ui_scale == 1.0`, a plugin that registers a game-init
//! callback from its own `init()` is **never called back**. That silently
//! killed our view hook, frame source (→ hotkey polling), and IL2CPP hooks.
//!
//! Instead we register the present-callback frame source immediately (it fires
//! every frame regardless of game-init timing — the same path the debug spike
//! proved works) and derive a reliable "game ready" edge from the first present
//! where IL2CPP metadata (`umamusume.dll`) is resolvable.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use edge_sdk::Sdk;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// Callback shape for [`register_on_game_ready`] — matches edge's
/// `GameInitializedCallback` so plugin handlers port over unchanged.
pub type GameReadyCallback = unsafe extern "C" fn(*mut c_void);

/// Options for [`init`].
#[derive(Debug, Clone, Default)]
pub struct InitOptions {
    /// File name (under edge's base dir) for self-hosted overlay layout
    /// persistence (panel/window positions). Each plugin should pass a
    /// distinct name (e.g. `"honseTrackerLayout.json"`); `None` disables
    /// position persistence.
    pub overlay_layout_file: Option<String>,
}

static GAME_READY: AtomicBool = AtomicBool::new(false);
static ON_GAME_READY: Lazy<Mutex<Vec<(usize, usize)>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Arm this plugin's services: install the present-callback frame source now.
///
/// The frame source drives frame jobs (hotkey polling), the first-present
/// bootstrap ([`poll_bootstrap`]), the view-id poll, and the self-hosted
/// overlay render pass. Call once from plugin `init()`.
pub fn init(opts: InitOptions) {
    if let Some(file) = opts.overlay_layout_file {
        crate::overlay::set_layout_file(&file);
    }
    if Sdk::try_get().is_none() {
        log::warn!("honse-services::init called before Sdk init");
        return;
    }
    // Register the present frame source immediately — NOT deferred to edge's
    // game-initialized (see module docs for why that never fires for us).
    crate::frame::install_frame_source();
}

/// Register a callback fired once the game runtime is ready (IL2CPP metadata up).
///
/// Replacement for `edge_sdk::Sdk::register_on_game_initialized`, which is
/// unreliable for `load_libraries` plugins (see module docs). If the game is
/// already ready when called, the callback fires immediately.
// userdata is an opaque token handed straight back to the caller's own
// callback (mirrors edge's register_on_game_initialized); we never deref it.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn register_on_game_ready(callback: GameReadyCallback, userdata: *mut c_void) {
    if GAME_READY.load(Ordering::Acquire) {
        // SAFETY: caller-provided C callback; ready edge already passed.
        unsafe { callback(userdata) };
        return;
    }
    ON_GAME_READY.lock().push((callback as usize, userdata as usize));
}

/// Whether the game-ready edge has fired.
#[must_use]
pub fn is_game_ready() -> bool {
    GAME_READY.load(Ordering::Acquire)
}

/// Run every present tick until the game is ready. On the first present where
/// `umamusume.dll` resolves, install the view-id poll and fire all
/// [`register_on_game_ready`] listeners. No-op afterwards.
pub(crate) fn poll_bootstrap() {
    if GAME_READY.load(Ordering::Acquire) {
        return;
    }
    let Some(sdk) = Sdk::try_get() else {
        return;
    };
    if sdk.get_assembly_image("umamusume.dll").is_none() {
        return; // IL2CPP metadata not loaded yet
    }
    GAME_READY.store(true, Ordering::Release);
    crate::view_hook::install_view_poll();
    let listeners = std::mem::take(&mut *ON_GAME_READY.lock());
    log::info!(
        "honse-services: game ready (first present, IL2CPP up); firing {} on-ready listener(s)",
        listeners.len()
    );
    for (cb, ud) in listeners {
        if cb == 0 {
            continue;
        }
        // SAFETY: pushed by register_on_game_ready as a GameReadyCallback + userdata.
        let cb: GameReadyCallback = unsafe { std::mem::transmute::<usize, GameReadyCallback>(cb) };
        // SAFETY: firing the registered C callback on the render thread; IL2CPP is up.
        unsafe { cb(ud as *mut c_void) };
    }
}
