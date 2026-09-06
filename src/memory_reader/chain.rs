//! IL2CPP singleton method-chain resolution and tracking lifecycle.
//!
//! Resolves and caches the `WorkDataManager → WorkSingleModeData →
//! WorkSingleModeCharaData` accessor chain used by every entity reader.

use std::ffi::c_void;
use std::sync::OnceLock;

use crate::compat::Sdk;

use super::il2cpp::{call_bool, call_obj, read_i32_field_any, read_ref_field};

/// All resolved MethodInfo pointers for the singleton chain.
pub(super) struct ResolvedChain {
    pub(super) wdm_klass: *mut c_void,

    // WorkDataManager → WorkSingleModeData
    pub(super) m_get_single_mode: *const c_void,

    // WorkSingleModeData getters
    pub(super) m_get_is_playing: *const c_void,
    pub(super) m_get_character: *const c_void,
    /// Field handle for `_totalTurnNum` (ObscuredInt) — read directly instead
    /// of calling `GetCurrentTurn()` which does master-data lookups and crashes
    /// when called outside the main thread's safe window.
    pub(super) f_total_turn_num: *mut c_void,
    pub(super) m_get_month: *const c_void,
    pub(super) m_get_total_races: *const c_void,
    pub(super) m_get_win_count: *const c_void,

    // WorkSingleModeCharaData getters
    pub(super) m_get_speed: *const c_void,
    pub(super) m_get_stamina: *const c_void,
    pub(super) m_get_power: *const c_void,
    pub(super) m_get_guts: *const c_void,
    pub(super) m_get_wiz: *const c_void,
    pub(super) m_get_all_total: *const c_void,
    pub(super) m_get_hp: *const c_void,
    pub(super) m_get_max_hp: *const c_void,
    pub(super) m_get_motivation: *const c_void,
    pub(super) m_get_fan_count: *const c_void,
    /// get_CardId() -> Int32 (the trained outfit/card id; matches gametora card_id).
    pub(super) m_get_card_id: *const c_void,
    pub(super) m_get_training_level: *const c_void, // GetTrainingLevel(1 arg: commandId)
    pub(super) m_get_scenario_id: *const c_void,    // get_ScenarioId(0 args)
    /// get_CharaEffectIdArray() -> ObscuredInt[] (active career conditions/状態).
    pub(super) m_get_chara_effect_id_array: *const c_void,

    // Aptitudes (RaceDefine.ProperGrade getters, 0 args) — for evaluation estimate.
    pub(super) m_apt_dist_short: *const c_void,
    pub(super) m_apt_dist_mile: *const c_void,
    pub(super) m_apt_dist_middle: *const c_void,
    pub(super) m_apt_dist_long: *const c_void,
    pub(super) m_apt_style_nige: *const c_void,
    pub(super) m_apt_style_senko: *const c_void,
    pub(super) m_apt_style_sashi: *const c_void,
    pub(super) m_apt_style_oikomi: *const c_void,
    pub(super) m_apt_ground_turf: *const c_void,
    pub(super) m_apt_ground_dirt: *const c_void,
    /// get_CardRarityData() -> MasterCardRarityData.CardRarityData (read `Rarity` field).
    pub(super) m_get_card_rarity_data: *const c_void,
}

// SAFETY: IL2CPP class/method pointers are stable for process lifetime.
unsafe impl Send for ResolvedChain {}
// SAFETY: IL2CPP pointers are stable for process lifetime.
unsafe impl Sync for ResolvedChain {}

pub(super) static CHAIN: OnceLock<ResolvedChain> = OnceLock::new();

// ---------------------------------------------------------------------------
// Resolution helpers
// ---------------------------------------------------------------------------

fn resolve_class(
    image: *const c_void,
    ns: &std::ffi::CStr,
    name: &std::ffi::CStr,
) -> Result<*mut c_void, &'static str> {
    let sdk = Sdk::get();
    let ns_s = ns.to_str().map_err(|_| "invalid namespace")?;
    let name_s = name.to_str().map_err(|_| "invalid class name")?;
    let Some(klass) = sdk.get_class(image.cast(), ns_s, name_s) else {
        hlog_error!("Class not found: {}", name_s);
        return Err("IL2CPP class not found");
    };
    Ok(klass.cast())
}

fn resolve_method(klass: *mut c_void, name: &std::ffi::CStr, args: i32) -> Result<*const c_void, &'static str> {
    let sdk = Sdk::get();
    let name_s = name.to_str().map_err(|_| "invalid method name")?;
    let Some(mi) = sdk.get_method(klass.cast(), name_s, args) else {
        hlog_error!("Method not found: {} (args={})", name_s, args);
        return Err("IL2CPP method not found");
    };
    Ok(mi.cast())
}

fn try_resolve() -> Result<ResolvedChain, &'static str> {
    hlog_info!("try_resolve: resolving IL2CPP assembly...");
    let Some(image) = Sdk::get().get_assembly_image("umamusume.dll") else {
        hlog_error!("try_resolve: umamusume.dll assembly not found");
        return Err("Assembly umamusume.dll not found");
    };
    let image = image.cast::<c_void>();
    // Resolve classes
    hlog_info!("try_resolve: resolving classes...");
    let wdm = resolve_class(image, c"Gallop", c"WorkDataManager")?;
    let wsmd = resolve_class(image, c"Gallop", c"WorkSingleModeData")?;
    let wsmcd = resolve_class(image, c"Gallop", c"WorkSingleModeCharaData")?;

    hlog_info!(
        "Resolved classes: WorkDataManager={:?} WorkSingleModeData={:?} WorkSingleModeCharaData={:?}",
        wdm,
        wsmd,
        wsmcd
    );

    hlog_info!("try_resolve: resolving methods...");

    // Resolve methods
    let chain = ResolvedChain {
        wdm_klass: wdm,
        m_get_single_mode: resolve_method(wdm, c"get_SingleMode", 0)?,

        m_get_is_playing: resolve_method(wsmd, c"get_IsPlaying", 0)?,
        m_get_character: resolve_method(wsmd, c"get_Character", 0)?,
        f_total_turn_num: {
            let sdk = Sdk::get();
            let f = sdk.get_field_from_name(wsmd.cast(), "_totalTurnNum");
            match f {
                Some(ptr) => ptr.cast(),
                None => {
                    hlog_error!("Field not found: _totalTurnNum on WorkSingleModeData");
                    return Err("IL2CPP field _totalTurnNum not found");
                }
            }
        },

        m_get_month: resolve_method(wsmd, c"get_Month", 0)?,
        m_get_total_races: resolve_method(wsmd, c"get_TotalRaceCount", 0)?,
        m_get_win_count: resolve_method(wsmd, c"get_WinCount", 0)?,

        m_get_speed: resolve_method(wsmcd, c"get_Speed", 0)?,
        m_get_stamina: resolve_method(wsmcd, c"get_Stamina", 0)?,
        m_get_power: resolve_method(wsmcd, c"get_Power", 0)?,
        m_get_guts: resolve_method(wsmcd, c"get_Guts", 0)?,
        m_get_wiz: resolve_method(wsmcd, c"get_Wiz", 0)?,
        m_get_all_total: resolve_method(wsmcd, c"GetAllTotalParameterValue", 0)?,
        m_get_hp: resolve_method(wsmcd, c"get_Hp", 0)?,
        m_get_max_hp: resolve_method(wsmcd, c"get_MaxHp", 0)?,
        m_get_motivation: resolve_method(wsmcd, c"get_Motivation", 0)?,
        m_get_fan_count: resolve_method(wsmcd, c"get_FanCount", 0)?,
        m_get_card_id: resolve_method(wsmcd, c"get_CardId", 0)?,
        m_get_training_level: resolve_method(wsmcd, c"GetTrainingLevel", 1)?,
        m_get_scenario_id: resolve_method(wsmcd, c"get_ScenarioId", 0)?,
        m_get_chara_effect_id_array: resolve_method(wsmcd, c"get_CharaEffectIdArray", 0)?,

        m_apt_dist_short: resolve_method(wsmcd, c"get_ProperDistanceShort", 0)?,
        m_apt_dist_mile: resolve_method(wsmcd, c"get_ProperDistanceMile", 0)?,
        m_apt_dist_middle: resolve_method(wsmcd, c"get_ProperDistanceMiddle", 0)?,
        m_apt_dist_long: resolve_method(wsmcd, c"get_ProperDistanceLong", 0)?,
        m_apt_style_nige: resolve_method(wsmcd, c"get_ProperRunningStyleNige", 0)?,
        m_apt_style_senko: resolve_method(wsmcd, c"get_ProperRunningStyleSenko", 0)?,
        m_apt_style_sashi: resolve_method(wsmcd, c"get_ProperRunningStyleSashi", 0)?,
        m_apt_style_oikomi: resolve_method(wsmcd, c"get_ProperRunningStyleOikomi", 0)?,
        m_apt_ground_turf: resolve_method(wsmcd, c"get_ProperGroundTurf", 0)?,
        m_apt_ground_dirt: resolve_method(wsmcd, c"get_ProperGroundDirt", 0)?,
        m_get_card_rarity_data: resolve_method(wsmcd, c"get_CardRarityData", 0)?,
    };

    hlog_info!("All 34 methods resolved for memory-read chain");
    Ok(chain)
}

// ---------------------------------------------------------------------------
// Lazy resolution lifecycle
// ---------------------------------------------------------------------------

/// Idempotent lazy resolution of the IL2CPP method chain (the same `CHAIN`
/// cell every entity reader uses). Returns whether the chain is resolved.
///
/// Caller contract: Unity main thread only. Resolution is metadata lookup —
/// no career state is read — so any main-thread reader that needs the chain
/// may call it, and the first one to do so pays for it. DLL load resolves
/// nothing.
pub(crate) fn ensure_resolved() -> bool {
    if CHAIN.get().is_some() {
        return true;
    }
    match try_resolve() {
        Ok(chain) => {
            let _ = CHAIN.set(chain); // ignore if race
            true
        }
        Err(e) => {
            hlog_warn!("memory-read chain not resolvable yet: {e}");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// t-001 settled-turn diagnostics (TEMPORARY — removed once the runtime
// coverage matrix is recorded).
// ---------------------------------------------------------------------------

/// Diagnostic turn read for the settled-edge coverage matrix.
///
/// Reads `_totalTurnNum` directly as an ObscuredInt field — safe on any thread
/// since it never calls methods that do master-data lookups.
///
/// Returns `None` when the chain cannot resolve yet (game runtime not up) or
/// when no career is active (`get_IsPlaying` false).
pub(crate) fn diag_read_current_turn() -> Option<i32> {
    if !ensure_resolved() {
        return None;
    }
    let chain = CHAIN.get()?;
    let wsmd = get_single_mode_data()?;
    // SAFETY: non-null WorkSingleModeData; reading ObscuredInt backing field.
    Some(unsafe { super::il2cpp::read_obscured_int_field(wsmd, chain.f_total_turn_num) })
}

// ---------------------------------------------------------------------------
// Shared helper
// ---------------------------------------------------------------------------

/// Get the `WorkSingleModeData` object pointer if a career is active.
pub fn get_single_mode_data() -> Option<*mut c_void> {
    let chain = CHAIN.get()?;
    let sdk = Sdk::get();
    let singleton = sdk.get_singleton(chain.wdm_klass.cast())?.cast::<c_void>();
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let wsmd = unsafe { call_obj(singleton, chain.m_get_single_mode) };
    if wsmd.is_null() {
        return None;
    }
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    if !unsafe { call_bool(wsmd, chain.m_get_is_playing) } {
        return None;
    }
    Some(wsmd)
}

/// The trainee whose skills the shop screens are spending points on, from
/// whichever mode is holding one.
///
/// # Every hop is a field read, not a call
///
/// Calling a property getter runs game code, and game code can throw. A
/// managed exception unwinding through an `extern "C"` Rust frame is not
/// something Rust can catch — it aborts the process, which is what
/// `0xc0000409` was in the two crashes this path caused. So this walks
/// backing fields instead: a read cannot throw, cannot allocate, and cannot
/// have side effects.
///
/// ```text
/// WorkDataManager (singleton)
///   <SingleMode>k__BackingField        → WorkSingleModeData
///       _isPlaying, <Character>k__BackingField
///   <IdleSingleModeData>k__BackingField → WorkIdleSingleModeData
///       _workCharaData
/// ```
/// Both end at a `WorkSingleModeCharaData`, so callers cannot tell which mode
/// answered. Names from `il2cpp_classes.txt` (Global 2026-08-30), resolved by
/// their logical form so the decoration can change without this having to.
///
/// Only for reads about the trainee. The capture path stays on the live-career
/// chain: an Independent Training run has no turns to settle.
pub fn get_skills_chara_ptr() -> Option<*mut c_void> {
    // The capture path resolves the chain on its first settled turn, and an
    // Independent Training run never has one.
    if !ensure_resolved() {
        return None;
    }
    let chain = CHAIN.get()?;
    let singleton = Sdk::get().get_singleton(chain.wdm_klass.cast())?.cast::<c_void>();

    // SAFETY: reading named managed fields off live work-data objects. Each
    // read yields null rather than dereferencing anything it did not find.
    unsafe {
        let single_mode = read_ref_field(singleton, &["singleMode"]);
        // `_isPlaying` is a `System.Boolean`, so only its first byte is
        // written into the wider read; the rest stays zero.
        if !single_mode.is_null() && read_i32_field_any(single_mode, &["isPlaying"]) != 0 {
            let chara = read_ref_field(single_mode, &["character"]);
            if !chara.is_null() {
                log_source("live career");
                return Some(chara);
            }
        }

        let idle = read_ref_field(singleton, &["idleSingleModeData"]);
        let chara = read_ref_field(idle, &["workCharaData"]);
        if !chara.is_null() {
            log_source("Independent Training");
            return Some(chara);
        }

        // The result screens after an Independent Training run reported
        // neither of the above: `_isPlaying` false, `_workCharaData` null. The
        // last place a trainee can be is `Character` with `_isPlaying` off —
        // possibly stale after a finished career, which the log makes visible.
        let chara = read_ref_field(single_mode, &["character"]);
        log_source(if chara.is_null() {
            "neither mode"
        } else {
            "live career object, not playing"
        });
        (!chara.is_null()).then_some(chara)
    }
}

/// Say once where the trainee came from. Which of the two paths answered is
/// the first thing worth knowing when this screen misbehaves.
fn log_source(source: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        hlog_info!("skills chara resolved from: {source}");
    }
}

pub fn get_chara_ptr() -> Option<*mut c_void> {
    let chain = CHAIN.get()?;
    let sdk = Sdk::get();

    let singleton = sdk.get_singleton(chain.wdm_klass.cast())?.cast::<c_void>();

    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let wsmd = unsafe { call_obj(singleton, chain.m_get_single_mode) };
    if wsmd.is_null() {
        return None;
    }

    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let is_playing = unsafe { call_bool(wsmd, chain.m_get_is_playing) };
    if !is_playing {
        return None;
    }

    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let chara = unsafe { call_obj(wsmd, chain.m_get_character) };
    if chara.is_null() {
        return None;
    }

    Some(chara)
}
