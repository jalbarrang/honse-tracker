//! The plugin's section of the Hachimi menu.
//!
//! # One section, not a pile of buttons
//!
//! This used to be seven flat menu items, three of which were one-off
//! reverse-engineering tools and two of which duplicated hotkeys. Worse, the
//! two settings were *buttons* labelled "Toggle …", so the menu never showed
//! whether the thing was on — you pressed it and read the log to find out.
//! They are checkboxes now, which is what they always were.
//!
//! # Drawn by the host, so no egui in sight
//!
//! The widgets come from [`edge_sdk::gui::ui`]: the host's `Ui` stays an
//! opaque pointer we hand straight back, and the host draws. That keeps this
//! module out of the egui/rustc lockstep contract entirely — see the header of
//! `crates/edge-sdk/src/gui.rs`. The overlay panels are unaffected either way;
//! they render on the plugin's own egui.
//!
//! # Settings are saved as they land, not as they were asked for
//!
//! Turning cut-in skipping on can fail if the hook will not install, and the
//! export can fail if its hooks did not. Both setters read back what actually
//! took before persisting it — a config claiming otherwise would be a lie that
//! survives a restart.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use edge_sdk::gui::ui;

use crate::{class_dump, idle_export, race_cutin, veterans_export};

/// Set while a class dump is in flight. The dump takes a while and writes a
/// large file; a second one running alongside the first helps nobody.
static DUMPING: AtomicBool = AtomicBool::new(false);

/// Register the section, or fall back to flat items.
///
/// `register_menu_section_raw` returns `false` on a host that does not export
/// `gui_register_menu_section`. An empty Plugins menu would look like the
/// plugin failed to load, so an older host gets the same actions as separate
/// entries instead.
pub fn install() {
    if edge_sdk::gui::register_menu_section_raw(draw) {
        return;
    }
    hlog_warn!(
        target: "training-tracker",
        "Host has no menu sections; falling back to plain menu items"
    );
    install_flat_items();
}

/// The section body, redrawn by the host every frame the menu is open.
fn draw(ui: *mut c_void) {
    ui::heading(ui, "honse-tracker");

    if ui::button(ui, "Export Veterans") {
        veterans_export::request();
    }

    let (idle_on, idle_changed) = ui::checkbox(ui, "Independent Training export", idle_export::is_enabled());
    if idle_changed {
        set_idle_export(idle_on);
    }

    let (skip_on, skip_changed) = ui::checkbox(ui, "Skip race cut-ins", race_cutin::is_enabled());
    if skip_changed {
        set_cutin_skip(skip_on);
    }

    ui::separator(ui);
    ui::small(ui, "Debug");
    if ui::button(ui, "Dump IL2CPP classes") {
        start_class_dump();
    }
}

/// Persist the Independent Training export setting, and say where the files go
/// — the part worth being able to check without opening the config file.
fn set_idle_export(enabled: bool) {
    idle_export::set_enabled(enabled);
    let took = idle_export::is_enabled();
    crate::config::edit(|file| file.settings.save_idle_careers = took);
    if took {
        hlog_info!(
            target: "training-tracker",
            "Idle career export directory: {}",
            idle_export::output_dir().display()
        );
    }
}

/// Persist the race cut-in setting. Skipping cut-ins is a setting rather than a
/// hotkey: it is something you decide once, and the menu is where the game's
/// own equivalent lives.
fn set_cutin_skip(enabled: bool) {
    race_cutin::set_enabled(enabled);
    let took = race_cutin::is_enabled();
    crate::config::edit(|file| file.settings.skip_race_skill_cutins = took);
}

/// Dump every IL2CPP class to disk, off the render thread.
fn start_class_dump() {
    if DUMPING.swap(true, Ordering::AcqRel) {
        hlog_info!(target: "training-tracker", "IL2CPP class dump already running");
        return;
    }
    hlog_info!(target: "training-tracker", "IL2CPP class dump requested");
    std::thread::spawn(|| {
        class_dump::dump_all_classes();
        DUMPING.store(false, Ordering::Release);
    });
}

/// The same four actions as separate menu items, for a host with no sections.
///
/// The two settings lose their checkbox here and go back to being toggles, so
/// the labels say so.
fn install_flat_items() {
    edge_sdk::gui::register_menu_item("honse-tracker: Export Veterans", veterans_export::request);
    edge_sdk::gui::register_menu_item("honse-tracker: Toggle Independent Training export", || {
        set_idle_export(!idle_export::is_enabled());
    });
    edge_sdk::gui::register_menu_item("honse-tracker: Toggle race cut-in skip", || {
        set_cutin_skip(!race_cutin::is_enabled());
    });
    edge_sdk::gui::register_menu_item("honse-tracker: Dump IL2CPP classes", start_class_dump);
}
