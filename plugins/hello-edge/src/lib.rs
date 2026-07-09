//! Scaffold smoke plugin — registers one menu section and logs on load.
//! Removed in plan 4 once debug-viewer exists.

use edge_sdk::{declare_plugin, hlog_info};

declare_plugin! {
    fn init() -> bool {
        hlog_info!("hello-edge loaded");
        // `register_menu_section` trampolines through `ui_from_ptr` each frame.
        let _ = edge_sdk::register_menu_section(|ui| {
            ui.label("hello-edge");
        });
        true
    }
}
