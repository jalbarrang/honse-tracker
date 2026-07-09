//! Safe Rust wrapper over upstream hachimi-edge's C `get_api` plugin surface.

pub mod api;
pub mod entry;
pub mod ffi;
pub mod gui;
pub mod log;
pub mod sdk;

pub use api::Api;
pub use egui;
pub use gui::{
    close_window, new_window_id, register_menu_section, register_menu_section_with_icon, reshow_window, show_window,
    ui_from_ptr,
};
pub use sdk::Sdk;
