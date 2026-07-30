//! Desktop launch wiring: window configuration and context injection.
//!
//! Everything here touches the Dioxus/WebView layer, so it lives in the binary
//! (not the library) and is never exercised by automated tests.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use dioxus::LaunchBuilder;
use tokio::sync::mpsc::UnboundedReceiver;

use honse_dashboard::storage::Storage;
use honse_dashboard::ui::{self, AppCtx};
use honse_dashboard::{AppEvent, APP_VERSION};

/// Runtime wiring handed to the UI process.
pub struct LaunchContext {
    pub storage: Storage,
    pub events: UnboundedReceiver<AppEvent>,
    pub listen_addr: SocketAddr,
    pub data_root: PathBuf,
}

/// Open the single application window and run the Dioxus event loop until the
/// user closes it. Blocks the calling (main) thread.
pub fn run(ctx: LaunchContext) {
    let window = WindowBuilder::new()
        .with_title(format!("Honse Tracker — sidecar {APP_VERSION}"))
        .with_inner_size(LogicalSize::new(1280.0, 860.0))
        .with_min_inner_size(LogicalSize::new(900.0, 640.0))
        .with_resizable(true);

    let config = Config::new()
        .with_window(window)
        .with_menu(None)
        .with_disable_context_menu(true)
        // Pre-paint close to the light ground color to avoid a white flash.
        .with_background_color((237, 239, 244, 255))
        .with_data_directory(ctx.data_root.join("webview"));

    let app_ctx = AppCtx {
        storage: ctx.storage,
        events: Arc::new(Mutex::new(Some(ctx.events))),
        listen_addr: ctx.listen_addr,
        data_root: ctx.data_root,
    };

    LaunchBuilder::new()
        .with_cfg(config)
        .with_context(app_ctx)
        .launch(ui::root);
}
