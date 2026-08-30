//! Grand Live / Our Grand Concert performance points reader.
//!
//! Path (all ObscuredInt backing fields — safe direct reads):
//! ```text
//! WorkSingleModeData.get_ScenarioLive() -> WorkSingleModeScenarioLive
//!   ._performance (PerformanceData value-type field, inline struct)
//!     fields: Dance, Passion, Vocal, Visual, Mental (each ObscuredInt)
//! ```
//!
//! PerformanceData is a value type (struct) stored inline in the parent object.
//! We read the ObscuredInt fields directly from the parent using field offsets.

use std::ffi::c_void;
use std::sync::Mutex;

use crate::compat::Sdk;

use super::super::il2cpp::{
    call_bool, call_bool_with_i32, call_i32, call_obj, call_obj_with_i32, read_i32_field, read_il2cpp_string,
    read_obj_array, read_obscured_int_field, resolve_obj_method,
};

/// One value per performance token.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PerformanceTokens {
    pub dance: i32,
    pub passion: i32,
    pub vocal: i32,
    pub visual: i32,
    /// Called "Composure" in EN, "Mental" internally.
    pub mental: i32,
}

impl PerformanceTokens {
    /// Token values in the game's display order, paired with their EN labels.
    #[must_use]
    pub const fn labelled(&self) -> [(&'static str, i32); 5] {
        [
            ("Dance", self.dance),
            ("Passion", self.passion),
            ("Vocal", self.vocal),
            ("Visual", self.visual),
            ("Composure", self.mental),
        ]
    }
}

/// One square currently offered on the Lessons tree: a technique or a song.
#[derive(Debug, Clone)]
pub struct GrandLiveSquare {
    pub square_id: i32,
    /// The game's own list position (`GetSortId`). Squares are returned in this
    /// order so the panel reads top-to-bottom against the screen behind it.
    pub sort_id: i32,
    /// Localized name, or `None` if the master row or its text did not resolve.
    pub name: Option<String>,
    /// Songs and techniques share the tree; songs also raise the concert.
    pub is_music: bool,
    /// Token cost, assembled from the master row's `PerfType`/`PerfValue` pairs.
    pub cost: PerformanceTokens,
    /// The game's own answer to "can this be taken right now"
    /// (`CanGetTreeSquare`), not a cost comparison of ours.
    pub affordable: bool,
}

/// Live performance points for the Grand Live scenario.
#[derive(Debug, Clone, Default)]
pub struct GrandLivePerformance {
    /// Points banked so far.
    pub tokens: PerformanceTokens,
    /// Per-token ceiling. **Not a constant** — it rises as the run progresses,
    /// so it is read live rather than hardcoded. All-zero means
    /// `GetPerformanceMax` did not resolve and the ceiling is unknown; callers
    /// must not print `0` as a denominator.
    pub caps: PerformanceTokens,
    /// Squares on offer this turn. Empty when the tree is unavailable or the
    /// master data did not resolve.
    pub squares: Vec<GrandLiveSquare>,
}

impl PerformanceTokens {
    /// Per-token shortfall of `self` against `cost`: how much more of each is
    /// needed. Zero where the tokens already cover it.
    #[must_use]
    pub fn shortfall(&self, cost: &Self) -> Self {
        Self {
            dance: (cost.dance - self.dance).max(0),
            passion: (cost.passion - self.passion).max(0),
            vocal: (cost.vocal - self.vocal).max(0),
            visual: (cost.visual - self.visual).max(0),
            mental: (cost.mental - self.mental).max(0),
        }
    }

    /// Whether every token is zero — an empty cost, or nothing missing.
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        self.dance == 0 && self.passion == 0 && self.vocal == 0 && self.visual == 0 && self.mental == 0
    }

    /// Add `value` to the token named by a master row's `PerfType`
    /// (the same 1..=5 enum the getters use). Unknown types are ignored.
    fn add(&mut self, perf_type: i32, value: i32) {
        match perf_type {
            x if x == PerformanceKind::Dance as i32 => self.dance += value,
            x if x == PerformanceKind::Passion as i32 => self.passion += value,
            x if x == PerformanceKind::Vocal as i32 => self.vocal += value,
            x if x == PerformanceKind::Visual as i32 => self.visual += value,
            x if x == PerformanceKind::Mental as i32 => self.mental += value,
            _ => {}
        }
    }
}

/// `SingleModeScenarioLive.Performance` includes `None = 0`; token slots start at 1.
#[derive(Debug, Clone, Copy)]
#[repr(i32)]
enum PerformanceKind {
    Dance = 1,
    Passion = 2,
    Vocal = 3,
    Visual = 4,
    Mental = 5,
}

fn tokens_from_getter(mut get: impl FnMut(i32) -> i32) -> PerformanceTokens {
    PerformanceTokens {
        dance: get(PerformanceKind::Dance as i32),
        passion: get(PerformanceKind::Passion as i32),
        vocal: get(PerformanceKind::Vocal as i32),
        visual: get(PerformanceKind::Visual as i32),
        mental: get(PerformanceKind::Mental as i32),
    }
}

/// Read Grand Live performance points from `WorkSingleModeData`.
/// Returns `None` if the Grand Live scenario is not active.
///
/// # Safety
/// `wsmd` must be a valid non-null `WorkSingleModeData` IL2CPP object pointer.
pub(in crate::memory_reader) unsafe fn read_performance(wsmd: *mut c_void) -> Option<GrandLivePerformance> {
    unsafe {
        let m_get_live = resolve_obj_method(wsmd, "get_ScenarioLive", 0)?;
        let live = call_obj(wsmd, m_get_live);
        if live.is_null() {
            return None;
        }

        // PerformanceData is a value type. Use GetPerformance(enum) instead,
        // which takes a Performance enum (int-backed) and returns the decrypted i32.
        // None=0, Dance=1, Passion=2, Vocal=3, Visual=4, Mental=5.
        let m_get = resolve_obj_method(live, "GetPerformance", 1)?;

        let fp: extern "C" fn(*mut c_void, i32, *const c_void) -> i32 =
            std::mem::transmute(super::super::il2cpp::method_ptr(m_get));

        let tokens = tokens_from_getter(|kind| fp(live, kind, m_get));

        // The ceiling rises over the run, so read it rather than assume it.
        // Its absence is not fatal: the tokens are still true without it.
        let caps = resolve_obj_method(live, "GetPerformanceMax", 1).map_or_else(PerformanceTokens::default, |m_max| {
            let fp_max: extern "C" fn(*mut c_void, i32, *const c_void) -> i32 =
                std::mem::transmute(super::super::il2cpp::method_ptr(m_max));
            tokens_from_getter(|kind| fp_max(live, kind, m_max))
        });

        let squares = read_squares(live);

        let result = GrandLivePerformance { tokens, caps, squares };
        log_on_change(&result);
        Some(result)
    }
}

/// Read the squares currently offered on the Lessons tree.
///
/// Affordability comes from the game's own `CanGetTreeSquare(squareId)` rather
/// than from comparing our cost numbers against our token numbers. The game
/// knows about prerequisites, per-turn limits and anything else we have not
/// modelled; a cost comparison would silently disagree with it.
///
/// # Safety
/// `live` must be a valid non-null `WorkSingleModeScenarioLive`.
unsafe fn read_squares(live: *mut c_void) -> Vec<GrandLiveSquare> {
    unsafe {
        let Some(m_tree) = resolve_obj_method(live, "get_TreeSquareInfoArray", 0) else {
            return Vec::new();
        };
        let array = call_obj(live, m_tree);
        let Some((base, len)) = read_obj_array(array) else {
            return Vec::new();
        };
        // Absent `CanGetTreeSquare`, report the squares without claiming any is
        // affordable — better an under-claim than a wrong one.
        let m_can_get = resolve_obj_method(live, "CanGetTreeSquare", 1);

        let mut out = Vec::with_capacity(len);
        let mut f_square_id: *mut c_void = std::ptr::null_mut();
        let mut m_is_music: Option<*const c_void> = None;
        let mut m_sort_id: Option<*const c_void> = None;

        for i in 0..len {
            let info = *base.add(i);
            if info.is_null() {
                continue;
            }
            if f_square_id.is_null() {
                // SAFETY: IL2CPP object header — klass pointer at offset 0.
                let klass = *(info as *const *mut c_void);
                let Some(field) = Sdk::get().get_field_from_name(klass.cast(), "<SquareId>k__BackingField") else {
                    hlog_warn!("grand_live: TreeSquareInfo.SquareId field not found");
                    return Vec::new();
                };
                f_square_id = field.cast();
                m_is_music = resolve_obj_method(info, "get_IsMusic", 0);
                m_sort_id = resolve_obj_method(info, "GetSortId", 0);
            }
            let square_id = read_obscured_int_field(info, f_square_id);
            if square_id <= 0 {
                continue;
            }
            let (name, cost) = master_square(square_id);
            out.push(GrandLiveSquare {
                square_id,
                sort_id: m_sort_id.map_or(i32::MAX, |mi| call_i32(info, mi)),
                name,
                is_music: m_is_music.is_some_and(|mi| call_bool(info, mi)),
                cost,
                affordable: m_can_get.is_some_and(|mi| call_bool_with_i32(live, mi, square_id)),
            });
        }
        // The game's order, so the panel and the screen behind it agree row for
        // row. Reordering by affordability broke that correspondence.
        out.sort_by_key(|s| (s.sort_id, s.square_id));
        out
    }
}

/// Look up a square's name and token cost in `MasterSingleModeLiveSquare`.
///
/// The cost is stored as five `(PerfType, PerfValue)` pairs rather than one
/// field per token, so it is assembled rather than read.
fn master_square(square_id: i32) -> (Option<String>, PerformanceTokens) {
    let mut cost = PerformanceTokens::default();
    let Some(mdm) = super::master_shop::master_data_manager() else {
        return (None, cost);
    };
    // SAFETY: `mdm` is the live MasterDataManager singleton; every call below
    // is a resolved getter on a non-null object, checked before use.
    unsafe {
        let Some(m_table) = resolve_obj_method(mdm, "get_masterSingleModeLiveSquare", 0) else {
            return (None, cost);
        };
        let table = call_obj(mdm, m_table);
        if table.is_null() {
            return (None, cost);
        }
        let Some(m_get) = resolve_obj_method(table, "Get", 1) else {
            return (None, cost);
        };
        let row = call_obj_with_i32(table, m_get, square_id);
        if row.is_null() {
            return (None, cost);
        }
        for slot in 1..=5 {
            let perf_type = read_i32_field(row, &format!("PerfType{slot}"));
            let value = read_i32_field(row, &format!("PerfValue{slot}"));
            if value > 0 {
                cost.add(perf_type, value);
            }
        }
        let name = resolve_obj_method(row, "get_SquareTitleText", 0)
            .map(|mi| call_obj(row, mi))
            .filter(|s| !s.is_null())
            .and_then(|s| read_il2cpp_string(s))
            .filter(|s| !s.is_empty());
        (name, cost)
    }
}

/// Log performance points when they change (deduped). The cap is logged too:
/// it moves on its own schedule, and a jump in it explains a sudden change in
/// what the panel says without any token having moved.
fn log_on_change(p: &GrandLivePerformance) {
    static LAST: Mutex<Option<(PerformanceTokens, PerformanceTokens)>> = Mutex::new(None);
    let cur = (p.tokens, p.caps);
    if let Ok(mut guard) = LAST.lock() {
        if guard.as_ref() == Some(&cur) {
            return;
        }
        *guard = Some(cur);
    }
    let t = p.tokens;
    hlog_info!(
        "Grand Live performance: Da={} Pa={} Vo={} Vi={} Co={} (cap {}) squares={} affordable={}",
        t.dance,
        t.passion,
        t.vocal,
        t.visual,
        t.mental,
        p.caps.dance,
        p.squares.len(),
        p.squares.iter().filter(|s| s.affordable).count()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn performance_tokens_use_enum_slots_one_through_five() {
        let mut requested = Vec::new();
        let performance = tokens_from_getter(|kind| {
            requested.push(kind);
            kind * 10
        });

        assert_eq!(requested, vec![1, 2, 3, 4, 5]);
        assert_eq!(performance.dance, 10);
        assert_eq!(performance.passion, 20);
        assert_eq!(performance.vocal, 30);
        assert_eq!(performance.visual, 40);
        assert_eq!(performance.mental, 50);
    }

    #[test]
    fn labels_follow_the_games_display_order() {
        let t = PerformanceTokens {
            dance: 1,
            passion: 2,
            vocal: 3,
            visual: 4,
            mental: 5,
        };
        assert_eq!(
            t.labelled(),
            [
                ("Dance", 1),
                ("Passion", 2),
                ("Vocal", 3),
                ("Visual", 4),
                ("Composure", 5),
            ]
        );
    }
}
