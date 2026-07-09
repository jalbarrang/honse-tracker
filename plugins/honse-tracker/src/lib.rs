//! Training Tracker — edge plugin port of the former in-core `training_tracker` module.
//!
//! Tracks training-facility visits and surfaces career analytics overlays/pages.
//! Source moved near-verbatim against [`compat`], which bridges the old
//! `hachimi-plugin-sdk` surface to `edge-sdk` + `honse-services`. Plugin entry
//! wiring lands in t-004 (`edge_sdk::declare_plugin!`).
//!
//! The `#![allow(...)]` block below carries the lint posture the tracker shipped with
//! as a standalone crate (its `[lints]` table) so the ~15k lines of moved source
//! satisfy the clippy floor without per-line churn.
#![allow(
    clippy::unwrap_in_result,
    clippy::panic_in_result_fn,
    clippy::as_underscore,
    clippy::fn_to_numeric_cast,
    clippy::fn_to_numeric_cast_any,
    clippy::ptr_as_ptr,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::needless_pass_by_value,
    clippy::missing_safety_doc,
    clippy::missing_transmute_annotations,
    clippy::useless_transmute,
    clippy::transmute_undefined_repr,
    clippy::type_complexity,
    clippy::len_without_is_empty,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::module_name_repetitions,
    clippy::too_many_arguments,
    clippy::wildcard_imports,
    clippy::cast_lossless,
    clippy::used_underscore_binding,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::doc_markdown,
    clippy::map_unwrap_or,
    clippy::manual_let_else,
    clippy::unnested_or_patterns,
    clippy::redundant_closure_for_method_calls,
    unnecessary_transmutes,
    function_casts_as_integer
)]

#[macro_use]
pub mod compat;

#[allow(dead_code)]
mod bond_progress;
#[allow(dead_code)]
mod build_profile;
mod career_meta;
mod chara_effects;
mod class_dump;
#[allow(dead_code)]
mod cm_model;
mod config;
#[allow(dead_code)]
mod course_data;
mod deck_bonuses;
mod diagnostics;
mod eval_data;
mod evaluation;
mod gametora_data;
mod hooks;
mod memory_reader;
mod overlay_cache;
mod overlay_prefs;
#[allow(dead_code)]
mod planner;
#[allow(dead_code)]
mod race_context;
mod rank_table;
#[allow(dead_code)]
mod recommend;
mod shop_hooks;
mod skill_shop;
mod skill_shop_prefs;
#[allow(dead_code)]
mod stat_targets;
mod telemetry;
pub(crate) mod ui;

/// Buy a career skill by id (affordability-gated, server-validated). Entry point
/// for out-of-module callers (e.g. the IPC server). `level` is the target skill
/// level (1 for normal skills). Returns the SP cost on success.
pub(crate) fn buy_skill(skill_id: i32, level: i32) -> Result<i32, String> {
    skill_shop::buy_skill(skill_id, level)
}

/// Drop the Career-panel texture negative-cache. Crate-visible entry point for
/// the hosted-data icon sync (`ICONS`), called after icons finish downloading so
/// the panel picks them up without a restart.
pub(crate) fn clear_icon_cache() {
    ui::textures::clear_missing();
}

/// Suspend the memory reader while a career command (training / rest / infirmary /
/// outing) plays out. Crate-visible entry point for the `SingleModeMainViewController`
/// command-submit IL2CPP hooks. See `overlay_cache::suspend_reads`.
pub(crate) fn suspend_reads_for_command() {
    overlay_cache::suspend_reads();
}

/// Resume the memory reader once the command-select screen has been rebuilt.
/// Crate-visible entry point for the `SingleModeMainViewController` command-select
/// IL2CPP hooks. See `overlay_cache::resume_reads`.
pub(crate) fn resume_reads_on_command_select() {
    overlay_cache::resume_reads();
}

// TODO(t-004): replace CoreModule lifecycle with `edge_sdk::declare_plugin!` +
// `honse_services::init` (config load, game-initialized hooks, surfaces, hosted-data).
