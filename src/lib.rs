//! Training Tracker — edge plugin port of the former in-core `training_tracker` module.
//!
//! Tracks training-facility visits and publishes career analytics data.
//! Source moved near-verbatim against [`compat`], which bridges the old
//! `legacy plugin SDK` surface to `edge-sdk` + `honse-services`. Plugin entry
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
    clippy::nonminimal_bool,
    clippy::undocumented_unsafe_blocks,
    unexpected_cfgs,
    dead_code,
    unnecessary_transmutes,
    function_casts_as_integer,
    non_snake_case
)]

#[macro_use]
pub mod compat;

mod apply_hooks;
mod command_hooks;
mod entry;
pub mod read_gate;

pub use read_gate::{
    career_state_for_view, read_gate, read_state, reads_permitted, transition, ApplyEvent, CareerEvent, CareerState,
    ReadState, View,
};

/// Hiker `Assignment` sort (compat method → provider). Used by generated property tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub method: u32,
    pub provider: i64,
}

/// Hiker `unique_provider` relation: same method ⇒ same provider.
#[must_use]
pub fn unique_provider(a: &Assignment, b: &Assignment) -> bool {
    a.method != b.method || a.provider == b.provider
}

/// Hiker `assigned` relation: provider ∈ {1,2,3}.
#[must_use]
pub fn assigned(a: &Assignment) -> bool {
    a.provider >= 1 && a.provider <= 3
}

#[allow(dead_code)]
mod bond_progress;
mod career_meta;
mod career_poll;
mod chara_effects;
pub(crate) mod class_dump;
mod deck_bonuses;
mod diagnostics;
mod eval_data;
mod evaluation;
mod gametora_data;
mod hooks;
mod memory_reader;
mod rank_table;
mod song_catalog;
mod song_plan;
mod telemetry;
mod ui;

/// Mark a career command in flight before the original submit method runs.
pub(crate) fn suspend_reads_for_command() {
    career_poll::enter_command();
}

/// Mark command select actionable after the original setup method returned.
pub(crate) fn resume_reads_on_command_select() {
    career_poll::command_select_settled();
}

/// Mark the initial/resumed command view actionable after play-in completed.
pub(crate) fn reads_on_command_view_play_in_completed() {
    career_poll::command_view_play_in_completed();
}
