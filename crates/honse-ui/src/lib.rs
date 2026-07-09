//! Shared egui theme tokens and low-level painters for Hachimi surfaces.
//!
//! This crate is intentionally pure UI code: no host, game, IL2CPP, or plugin
//! runtime dependencies.

pub use egui;
pub use egui_taffy;

pub mod components;
pub mod paint;
pub mod theme;
