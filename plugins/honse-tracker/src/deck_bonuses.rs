//! Reads deck support card bonuses once per career start.
//!
//! Walks `WorkSingleModeCharaData.EquipSupportCardArray` → per-card master
//! effect table → sums by `SupportCardEffectType`.
//!
//! The result is stored and cleared on career end. No UI — consumed by other
//! modules when they need bonus info.

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use crate::compat::Sdk;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Aggregated deck bonuses for the current career.
#[derive(Debug, Clone, Default)]
pub struct DeckBonuses {
    /// Per-stat training bonus (Speed/Stamina/Power/Guts/Wiz), summed across deck.
    pub training_speed: i32,
    pub training_stamina: i32,
    pub training_power: i32,
    pub training_guts: i32,
    pub training_wiz: i32,
    /// General training effect bonus (applies to all facilities).
    pub training_effect: i32,
    /// Initial stat bonuses.
    pub initial_speed: i32,
    pub initial_stamina: i32,
    pub initial_power: i32,
    pub initial_guts: i32,
    pub initial_wiz: i32,
    /// Initial friendship/evaluation bonus.
    pub initial_evaluation: i32,
    /// Race stat bonus % (the "Race Bonus" shown on deck screen).
    pub race_status: i32,
    /// Race fan gain bonus %.
    pub race_fan: i32,
    /// Skill hint level up bonus.
    pub skill_tips_lv_up: i32,
    /// Skill hint event rate up.
    pub skill_tips_event_rate: i32,
    /// Good training (rainbow) rate bonus.
    pub good_training_rate: i32,
    /// Stat cap bonuses.
    pub limit_speed: i32,
    pub limit_stamina: i32,
    pub limit_power: i32,
    pub limit_guts: i32,
    pub limit_wiz: i32,
    /// Event recovery amount bonus.
    pub event_recovery: i32,
    /// Event effect bonus.
    pub event_effect: i32,
    /// Training failure rate reduction.
    pub failure_rate_down: i32,
    /// Training HP consumption reduction.
    pub hp_consumption_down: i32,
    /// Minigame effect bonus.
    pub minigame_effect: i32,
    /// Special tag ("得意率") bonus.
    pub special_tag_effect: i32,
    /// Motivation bonus.
    pub motivation_up: i32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

static BONUSES: Mutex<Option<DeckBonuses>> = Mutex::new(None);
static RESOLVED: OnceLock<Resolved> = OnceLock::new();

/// Get the current deck bonuses (if a career is active and they've been read).
#[allow(dead_code)] // Will be consumed by UI/other modules later
pub fn current() -> Option<DeckBonuses> {
    BONUSES.lock().ok().and_then(|g| g.clone())
}

/// Clear stored bonuses (call when career ends).
pub fn clear() {
    if let Ok(mut g) = BONUSES.lock() {
        *g = None;
    }
}

/// Whether bonuses have been captured for the current career.
pub fn is_captured() -> bool {
    BONUSES.lock().ok().is_some_and(|g| g.is_some())
}

/// Read and store deck bonuses from the live career data.
/// Call from the Unity main thread when `is_playing` transitions to true.
/// Safe to call multiple times — only captures once until [`clear`].
pub fn try_capture(chara: *mut c_void) {
    if is_captured() || chara.is_null() {
        return;
    }

    let Some(r) = ensure_resolved() else {
        return;
    };

    match read_deck_bonuses(chara, r) {
        Some(bonuses) => {
            hlog_info!(
                "Deck bonuses captured: race_status={} race_fan={} training_effect={}",
                bonuses.race_status,
                bonuses.race_fan,
                bonuses.training_effect
            );
            if let Ok(mut g) = BONUSES.lock() {
                *g = Some(bonuses);
            }
        }
        None => {
            hlog_warn!("Deck bonuses: failed to read (EquipSupportCardArray null or empty)");
        }
    }
}

// ---------------------------------------------------------------------------
// IL2CPP resolution
// ---------------------------------------------------------------------------

struct Resolved {
    // WorkSingleModeCharaData
    m_get_equip_array: *const c_void, // get_EquipSupportCardArray() → EquipSupportCard[]

    // EquipSupportCard
    m_convert: *const c_void, // ConvertWorkSupportCardData() → WorkSupportCardData.SupportCardData

    // WorkSupportCardData.SupportCardData
    m_get_effect_list: *const c_void, // GetMasterSupportCardEffectList() → List<SupportCardEffectTable>

    // SupportCardEffectTable
    m_get_effect_type: *const c_void, // GetEffectType() → SupportCardEffectType (i32 enum)
    #[allow(dead_code)] // Available for alternative value lookup
    m_get_value: *const c_void, // GetValueEffect(2 args) → i32
}

// SAFETY: IL2CPP pointers are stable for process lifetime.
unsafe impl Send for Resolved {}
// SAFETY: IL2CPP pointers are stable for process lifetime.
unsafe impl Sync for Resolved {}

fn ensure_resolved() -> Option<&'static Resolved> {
    if let Some(r) = RESOLVED.get() {
        return Some(r);
    }
    match try_resolve() {
        Ok(r) => {
            let _ = RESOLVED.set(r);
            RESOLVED.get()
        }
        Err(e) => {
            hlog_warn!("Deck bonuses resolution failed: {}", e);
            None
        }
    }
}

fn try_resolve() -> Result<Resolved, &'static str> {
    let sdk = Sdk::get();
    let Some(img) = sdk.get_assembly_image("umamusume.dll") else {
        return Err("umamusume.dll not found");
    };

    // WorkSingleModeCharaData
    let wsmcd = sdk
        .get_class(img, "Gallop", "WorkSingleModeCharaData")
        .ok_or("WorkSingleModeCharaData not found")?;
    let m_get_equip = sdk
        .get_method(wsmcd, "get_EquipSupportCardArray", 0)
        .ok_or("get_EquipSupportCardArray not found")?;

    // EquipSupportCard (nested)
    let equip_sc = sdk
        .find_nested_class(wsmcd, "EquipSupportCard")
        .ok_or("EquipSupportCard not found")?;
    let m_convert = sdk
        .get_method(equip_sc, "ConvertWorkSupportCardData", 0)
        .ok_or("ConvertWorkSupportCardData not found")?;

    // WorkSupportCardData.SupportCardData
    let wsc_data_parent = sdk
        .get_class(img, "Gallop", "WorkSupportCardData")
        .ok_or("WorkSupportCardData not found")?;
    let wsc_data = sdk
        .find_nested_class(wsc_data_parent, "SupportCardData")
        .ok_or("WorkSupportCardData.SupportCardData not found")?;
    let m_get_effect_list = sdk
        .get_method(wsc_data, "GetMasterSupportCardEffectList", 0)
        .ok_or("GetMasterSupportCardEffectList not found")?;

    // MasterSupportCardEffectTable.SupportCardEffectTable
    let mscet_parent = sdk
        .get_class(img, "Gallop", "MasterSupportCardEffectTable")
        .ok_or("MasterSupportCardEffectTable not found")?;
    let scet = sdk
        .find_nested_class(mscet_parent, "SupportCardEffectTable")
        .ok_or("SupportCardEffectTable not found")?;
    let m_get_type = sdk
        .get_method(scet, "GetEffectType", 0)
        .ok_or("GetEffectType not found")?;
    let m_get_value = sdk
        .get_method(scet, "GetValueEffect", 2)
        .ok_or("GetValueEffect not found")?;

    Ok(Resolved {
        m_get_equip_array: m_get_equip.cast(),
        m_convert: m_convert.cast(),
        m_get_effect_list: m_get_effect_list.cast(),
        m_get_effect_type: m_get_type.cast(),
        m_get_value: m_get_value.cast(),
    })
}

// ---------------------------------------------------------------------------
// IL2CPP helpers
// ---------------------------------------------------------------------------

#[inline]
unsafe fn mptr(mi: *const c_void) -> usize {
    // SAFETY: MethodInfo starts with method_pointer.
    unsafe { *(mi as *const usize) }
}

#[inline]
unsafe fn call_obj(this: *mut c_void, mi: *const c_void) -> *mut c_void {
    // SAFETY: IL2CPP calling convention.
    let f: extern "C" fn(*mut c_void, *const c_void) -> *mut c_void = unsafe { std::mem::transmute(mptr(mi)) };
    f(this, mi)
}

#[inline]
unsafe fn call_i32(this: *mut c_void, mi: *const c_void) -> i32 {
    // SAFETY: IL2CPP calling convention.
    let f: extern "C" fn(*mut c_void, *const c_void) -> i32 = unsafe { std::mem::transmute(mptr(mi)) };
    f(this, mi)
}

#[allow(dead_code)] // Available for alternative value lookup via GetValueEffect(2)
#[inline]
unsafe fn call_i32_2(this: *mut c_void, mi: *const c_void, a: i32, b: i32) -> i32 {
    // SAFETY: IL2CPP calling convention.
    let f: extern "C" fn(*mut c_void, i32, i32, *const c_void) -> i32 = unsafe { std::mem::transmute(mptr(mi)) };
    f(this, a, b, mi)
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

fn read_deck_bonuses(chara: *mut c_void, r: &Resolved) -> Option<DeckBonuses> {
    // SAFETY: chara is a valid WorkSingleModeCharaData from the snapshot chain.
    let array = unsafe { call_obj(chara, r.m_get_equip_array) };
    if array.is_null() {
        return None;
    }

    // IL2CPP array: length at offset 0x18, elements start at 0x20 (pointer-sized).
    // SAFETY: array is a valid IL2CPP System.Array from get_EquipSupportCardArray.
    let len = unsafe { *(array.byte_add(0x18) as *const i32) };
    if len <= 0 || len > 8 {
        return None;
    }

    let mut bonuses = DeckBonuses::default();

    for i in 0..len {
        // SAFETY: Array element at index i (pointer-sized elements starting at 0x20).
        let equip = unsafe { *(array.byte_add(0x20 + (i as usize) * 8) as *const *mut c_void) };
        if equip.is_null() {
            continue;
        }
        // SAFETY: ConvertWorkSupportCardData on a valid EquipSupportCard.
        let scd = unsafe { call_obj(equip, r.m_convert) };
        if scd.is_null() {
            continue;
        }
        // SAFETY: GetMasterSupportCardEffectList on valid SupportCardData.
        let effect_list = unsafe { call_obj(scd, r.m_get_effect_list) };
        if effect_list.is_null() {
            continue;
        }
        accumulate_effects(&mut bonuses, effect_list, r);
    }

    Some(bonuses)
}

fn accumulate_effects(bonuses: &mut DeckBonuses, list: *mut c_void, r: &Resolved) {
    let sdk = Sdk::get();
    // SAFETY: list is an IL2CPP object; klass at offset 0.
    let list_klass = unsafe { *(list as *const *mut c_void) };
    let Some(m_count) = sdk.get_method(list_klass.cast(), "get_Count", 0) else {
        return;
    };
    let Some(m_item) = sdk.get_method(list_klass.cast(), "get_Item", 1) else {
        return;
    };
    // SAFETY: get_Count on valid List.
    let count = unsafe { call_i32(list, m_count.cast()) };
    if count <= 0 || count > 64 {
        return;
    }

    for i in 0..count {
        // SAFETY: get_Item on List<SupportCardEffectTable> returns an object reference.
        let item = unsafe {
            let f: extern "C" fn(*mut c_void, i32, *const c_void) -> *mut c_void =
                std::mem::transmute(mptr(m_item.cast()));
            f(list, i, m_item.cast())
        };
        if item.is_null() {
            continue;
        }
        // SAFETY: GetEffectType returns the SupportCardEffectType enum (Int32).
        let effect_type = unsafe { call_i32(item, r.m_get_effect_type) };
        // SAFETY: GetValueEffect(limitBreakCount?, level?) — we pass (0, 0) to get current value.
        // Actually, the row already has _currentEffectivenessValue set by SetValueArray.
        // GetValueEffect likely takes (level, limitBreakCount). Since the card data was
        // already initialized via ConvertWorkSupportCardData, we call GetDefaultCalcValue
        // equivalent. Pass 0,0 first — if the values look wrong we'll adjust.
        //
        // Looking at the fields: Init, LimitLv5..LimitLv50 and _currentEffectivenessValue.
        // GetValueEffect(2) likely takes (currentLevel, limitBreakLevel) and returns the
        // appropriate interpolated value.
        //
        // Since ConvertWorkSupportCardData sets up the SupportCardData with the card's
        // actual level/LB, the effect list rows should already be configured.
        // We read _currentEffectivenessValue directly as a field instead.
        let value = unsafe { read_current_value(item) };
        apply_effect(bonuses, effect_type, value);
    }
}

/// Read `_currentEffectivenessValue` directly from a `SupportCardEffectTable` instance.
unsafe fn read_current_value(effect_row: *mut c_void) -> i32 {
    let sdk = Sdk::get();
    // SAFETY: effect_row klass at offset 0.
    let klass = unsafe { *(effect_row as *const *mut c_void) };
    let Some(field) = sdk.get_field_from_name(klass.cast(), "_currentEffectivenessValue") else {
        return 0;
    };
    let mut value: i32 = 0;
    // SAFETY: Reading a plain Int32 field from a valid IL2CPP object.
    unsafe {
        sdk.get_field_value(effect_row.cast(), field, &mut value as *mut _ as *mut c_void);
    }
    value
}

// ---------------------------------------------------------------------------
// SupportCardEffectType enum mapping
// ---------------------------------------------------------------------------

// Values from GameDefine.SupportCardEffectType (class dump 2026-05-25).
// The enum is declared in order, so values are 0, 1, 2, ...
const EFFECT_SPECIAL_TAG: i32 = 1;
const EFFECT_MOTIVATION: i32 = 2;
const EFFECT_TRAINING_SPEED: i32 = 3;
const EFFECT_TRAINING_STAMINA: i32 = 4;
const EFFECT_TRAINING_POWER: i32 = 5;
const EFFECT_TRAINING_GUTS: i32 = 6;
const EFFECT_TRAINING_WIZ: i32 = 7;
const EFFECT_TRAINING_EFFECT: i32 = 8;
const EFFECT_INITIAL_SPEED: i32 = 9;
const EFFECT_INITIAL_STAMINA: i32 = 10;
const EFFECT_INITIAL_POWER: i32 = 11;
const EFFECT_INITIAL_GUTS: i32 = 12;
const EFFECT_INITIAL_WIZ: i32 = 13;
const EFFECT_INITIAL_EVALUATION: i32 = 14;
const EFFECT_RACE_STATUS: i32 = 15;
const EFFECT_RACE_FAN: i32 = 16;
const EFFECT_SKILL_TIPS_LV: i32 = 17;
const EFFECT_SKILL_TIPS_EVENT_RATE: i32 = 18;
const EFFECT_GOOD_TRAINING_RATE: i32 = 19;
const EFFECT_LIMIT_SPEED: i32 = 20;
const EFFECT_LIMIT_STAMINA: i32 = 21;
const EFFECT_LIMIT_POWER: i32 = 22;
const EFFECT_LIMIT_GUTS: i32 = 23;
const EFFECT_LIMIT_WIZ: i32 = 24;
const EFFECT_EVENT_RECOVERY: i32 = 25;
const EFFECT_EVENT_EFFECT: i32 = 26;
const EFFECT_FAILURE_RATE_DOWN: i32 = 27;
const EFFECT_HP_CONSUMPTION_DOWN: i32 = 28;
const EFFECT_MINIGAME_EFFECT: i32 = 29;

fn apply_effect(b: &mut DeckBonuses, effect_type: i32, value: i32) {
    match effect_type {
        EFFECT_SPECIAL_TAG => b.special_tag_effect += value,
        EFFECT_MOTIVATION => b.motivation_up += value,
        EFFECT_TRAINING_SPEED => b.training_speed += value,
        EFFECT_TRAINING_STAMINA => b.training_stamina += value,
        EFFECT_TRAINING_POWER => b.training_power += value,
        EFFECT_TRAINING_GUTS => b.training_guts += value,
        EFFECT_TRAINING_WIZ => b.training_wiz += value,
        EFFECT_TRAINING_EFFECT => b.training_effect += value,
        EFFECT_INITIAL_SPEED => b.initial_speed += value,
        EFFECT_INITIAL_STAMINA => b.initial_stamina += value,
        EFFECT_INITIAL_POWER => b.initial_power += value,
        EFFECT_INITIAL_GUTS => b.initial_guts += value,
        EFFECT_INITIAL_WIZ => b.initial_wiz += value,
        EFFECT_INITIAL_EVALUATION => b.initial_evaluation += value,
        EFFECT_RACE_STATUS => b.race_status += value,
        EFFECT_RACE_FAN => b.race_fan += value,
        EFFECT_SKILL_TIPS_LV => b.skill_tips_lv_up += value,
        EFFECT_SKILL_TIPS_EVENT_RATE => b.skill_tips_event_rate += value,
        EFFECT_GOOD_TRAINING_RATE => b.good_training_rate += value,
        EFFECT_LIMIT_SPEED => b.limit_speed += value,
        EFFECT_LIMIT_STAMINA => b.limit_stamina += value,
        EFFECT_LIMIT_POWER => b.limit_power += value,
        EFFECT_LIMIT_GUTS => b.limit_guts += value,
        EFFECT_LIMIT_WIZ => b.limit_wiz += value,
        EFFECT_EVENT_RECOVERY => b.event_recovery += value,
        EFFECT_EVENT_EFFECT => b.event_effect += value,
        EFFECT_FAILURE_RATE_DOWN => b.failure_rate_down += value,
        EFFECT_HP_CONSUMPTION_DOWN => b.hp_consumption_down += value,
        EFFECT_MINIGAME_EFFECT => b.minigame_effect += value,
        _ => {} // Unknown effect type — ignore
    }
}
