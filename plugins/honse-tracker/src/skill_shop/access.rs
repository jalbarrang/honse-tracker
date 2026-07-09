//! Skill shop reconstruction from game memory.
//!
//! Reads `_skillTipsList` from `WorkSingleModeCharaData` and resolves
//! skill names and costs via the game's own master data accessor chain:
//!
//! ```text
//! MasterDataManager (singleton)
//!   → get_masterSkillData() → MasterSkillData
//!     → GetListWithGroupIdOrderByIdAsc(group_id) → List<SkillData>
//!       → each: .Id, .Rarity, get_Name(), .GradeValue
//!   → get_masterSingleModeSkillNeedPoint() → MasterSingleModeSkillNeedPoint
//!     → Get(skill_id) → SingleModeSkillNeedPoint
//!       → .NeedSkillPoint
//! ```
//!
//! Shop reads are invoked from [`crate::overlay_cache`] on the Unity main thread only.

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::OnceLock;

use crate::compat::Sdk;

use crate::memory_reader;
use crate::shop_hooks;

use super::il2cpp::{
    call_i32, call_obj, call_obj_i32, decrypt_obscured_int, read_field_i32, read_i32_list, read_string,
};
use super::logic::{pick_best_variant, sort_shop_entries, SkillCandidate};
use super::SkillShopEntry;

// ---------------------------------------------------------------------------
// Resolved IL2CPP pointers (all from one-time resolution)
// ---------------------------------------------------------------------------

struct Resolved {
    // MasterDataManager (singleton)
    mdm_klass: *mut c_void,
    m_get_master_skill_data: *const c_void, // → MasterSkillData
    m_get_skill_need_point: *const c_void,  // → MasterSingleModeSkillNeedPoint

    // MasterSkillData
    m_msd_get: *const c_void,               // Get(int) → SkillData
    m_msd_get_list_by_group: *const c_void, // GetListWithGroupIdOrderByIdAsc(int)

    // MasterSingleModeSkillNeedPoint
    m_snp_get: *const c_void, // Get(int) → SingleModeSkillNeedPoint

    // MasterSkillData.SkillData fields/methods
    f_sd_id: *mut c_void,
    f_sd_rarity: *mut c_void,
    f_sd_group_rate: *mut c_void,
    f_sd_group_id: *mut c_void,
    f_sd_filter_switch: *mut c_void,
    m_sd_get_name: *const c_void,
    m_sd_get_tag_ids: *const c_void, // GetTagIds() → List<Int32>

    // SingleModeSkillNeedPoint fields
    f_snp_need_skill_point: *mut c_void,

    // SkillTips backing fields
    f_tips_group_id: *mut c_void,
    f_tips_rarity: *mut c_void,
    f_tips_level: *mut c_void,

    // WorkSingleModeCharaData skill point
    f_skill_point: *mut c_void,

    // Innate (trainee) available-skill set — best-effort; null when unresolved.
    m_get_avail_skill_set: *const c_void, // MasterDataManager.get_masterAvailableSkillSet
    m_ass_get_list: *const c_void,        // GetListWithAvailableSkillSetIdOrderByIdAsc(int)
    f_ass_skill_id: *mut c_void,          // AvailableSkillSet.SkillId
    f_ass_need_rank: *mut c_void,         // AvailableSkillSet.NeedRank
    m_chara_get_card_data: *const c_void, // WorkSingleModeCharaData.get_CardData
    f_card_avail_set_id: *mut c_void,     // MasterCardData.CardData.AvailableSkillSetId
    f_talent_level: *mut c_void,          // chara <TalentLevel>k__BackingField (ObscuredInt)
}

impl Resolved {
    /// True when the full innate-skill chain resolved.
    fn has_innate(&self) -> bool {
        !self.m_get_avail_skill_set.is_null()
            && !self.m_ass_get_list.is_null()
            && !self.f_ass_skill_id.is_null()
            && !self.f_ass_need_rank.is_null()
            && !self.m_chara_get_card_data.is_null()
            && !self.f_card_avail_set_id.is_null()
    }
}

// SAFETY: IL2CPP pointers are stable for process lifetime.
unsafe impl Send for Resolved {}
// SAFETY: IL2CPP pointers are stable for process lifetime.
unsafe impl Sync for Resolved {}

static RESOLVED: OnceLock<Resolved> = OnceLock::new();

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

fn ensure_resolved() -> bool {
    if RESOLVED.get().is_some() {
        return true;
    }
    match try_resolve() {
        Ok(r) => {
            let _ = RESOLVED.set(r);
            true
        }
        Err(e) => {
            hlog_error!("Skill shop resolution failed: {}", e);
            false
        }
    }
}

macro_rules! resolve {
    (class $img:expr, $ns:literal, $name:literal) => {{
        let sdk = Sdk::get();
        let Some(k) = sdk.get_class($img, $ns, $name) else {
            return Err(concat!($name, " not found"));
        };
        k.cast::<c_void>()
    }};
    (nested $parent:expr, $name:literal) => {{
        let sdk = Sdk::get();
        let Some(k) = sdk.find_nested_class($parent.cast(), $name) else {
            return Err(concat!("nested ", $name, " not found"));
        };
        k.cast::<c_void>()
    }};
    (method $klass:expr, $name:literal, $args:expr) => {{
        let sdk = Sdk::get();
        let Some(m) = sdk.get_method($klass.cast(), $name, $args) else {
            return Err(concat!($name, " method not found"));
        };
        m.cast::<c_void>()
    }};
    (field $klass:expr, $name:literal) => {{
        let sdk = Sdk::get();
        let Some(f) = sdk.get_field_from_name($klass.cast(), $name) else {
            return Err(concat!($name, " field not found"));
        };
        f.cast::<c_void>()
    }};
    (field_opt $klass:expr, $name:literal) => {{
        Sdk::get()
            .get_field_from_name($klass.cast(), $name)
            .map(|f| f.cast::<c_void>())
            .unwrap_or(std::ptr::null_mut())
    }};
}

fn try_resolve() -> Result<Resolved, &'static str> {
    let sdk = Sdk::get();
    let Some(img) = sdk.get_assembly_image("umamusume.dll") else {
        return Err("umamusume.dll not found");
    };

    // MasterDataManager (singleton hub)
    let mdm = resolve!(class img, "Gallop", "MasterDataManager");
    let m_get_msd = resolve!(method mdm, "get_masterSkillData", 0);
    let m_get_snp = resolve!(method mdm, "get_masterSingleModeSkillNeedPoint", 0);

    // MasterSkillData
    let msd_klass = resolve!(class img, "Gallop", "MasterSkillData");
    let m_msd_get = resolve!(method msd_klass, "Get", 1);
    let m_msd_get_list = resolve!(method msd_klass, "GetListWithGroupIdOrderByIdAsc", 1);

    // MasterSkillData.SkillData (nested)
    let sd_klass = resolve!(nested msd_klass, "SkillData");
    let f_sd_id = resolve!(field sd_klass, "Id");
    let f_sd_rarity = resolve!(field sd_klass, "Rarity");
    let f_sd_grate = resolve!(field sd_klass, "GroupRate");
    let f_sd_gid = resolve!(field sd_klass, "GroupId");
    let f_sd_fswitch = resolve!(field_opt sd_klass, "FilterSwitch");
    let m_sd_name = resolve!(method sd_klass, "get_Name", 0);
    let m_sd_get_tag_ids = sdk
        .get_method(sd_klass.cast(), "GetTagIds", 0)
        .map(|m| m.cast::<c_void>())
        .unwrap_or(std::ptr::null());

    // MasterSingleModeSkillNeedPoint
    let snp_klass = resolve!(class img, "Gallop", "MasterSingleModeSkillNeedPoint");
    let m_snp_get = resolve!(method snp_klass, "Get", 1);
    let snp_row = resolve!(nested snp_klass, "SingleModeSkillNeedPoint");
    let f_snp_cost = resolve!(field snp_row, "NeedSkillPoint");

    // SkillTips
    let wsmcd = resolve!(class img, "Gallop", "WorkSingleModeCharaData");
    let tips = resolve!(nested wsmcd, "SkillTips");
    let f_gid = resolve!(field tips, "<GroupId>k__BackingField");
    let f_rar = resolve!(field tips, "<Rarity>k__BackingField");
    let f_lvl = resolve!(field tips, "<Level>k__BackingField");

    // SkillPoint
    let f_sp = resolve!(field_opt wsmcd, "<SkillPoint>k__BackingField");

    // Innate available-skill set (best-effort — additive, never fatal).
    let m_get_avail = sdk
        .get_method(mdm.cast(), "get_masterAvailableSkillSet", 0)
        .map(|m| m.cast::<c_void>())
        .unwrap_or(std::ptr::null());
    let (m_ass_get_list, f_ass_skill_id, f_ass_need_rank) =
        match sdk.get_class(img, "Gallop", "MasterAvailableSkillSet") {
            Some(ass) => (
                sdk.get_method(ass, "GetListWithAvailableSkillSetIdOrderByIdAsc", 1)
                    .map(|m| m.cast::<c_void>())
                    .unwrap_or(std::ptr::null()),
                sdk.find_nested_class(ass, "AvailableSkillSet")
                    .and_then(|row| sdk.get_field_from_name(row, "SkillId"))
                    .map(|f| f.cast::<c_void>())
                    .unwrap_or(std::ptr::null_mut()),
                sdk.find_nested_class(ass, "AvailableSkillSet")
                    .and_then(|row| sdk.get_field_from_name(row, "NeedRank"))
                    .map(|f| f.cast::<c_void>())
                    .unwrap_or(std::ptr::null_mut()),
            ),
            None => (std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut()),
        };
    let m_chara_get_card_data = sdk
        .get_method(wsmcd.cast(), "get_CardData", 0)
        .map(|m| m.cast::<c_void>())
        .unwrap_or(std::ptr::null());
    let f_card_avail_set_id = sdk
        .get_class(img, "Gallop", "MasterCardData")
        .and_then(|cd| sdk.find_nested_class(cd, "CardData"))
        .and_then(|row| sdk.get_field_from_name(row, "AvailableSkillSetId"))
        .map(|f| f.cast::<c_void>())
        .unwrap_or(std::ptr::null_mut());
    let f_talent_level = resolve!(field_opt wsmcd, "<TalentLevel>k__BackingField");

    hlog_info!("Skill shop: full IL2CPP chain resolved (MasterDataManager → SkillData + NeedPoint)");
    Ok(Resolved {
        mdm_klass: mdm as _,
        m_get_master_skill_data: m_get_msd,
        m_get_skill_need_point: m_get_snp,
        m_msd_get,
        m_msd_get_list_by_group: m_msd_get_list,
        m_snp_get,
        f_sd_id,
        f_sd_rarity,
        f_sd_group_rate: f_sd_grate,
        f_sd_group_id: f_sd_gid,
        f_sd_filter_switch: f_sd_fswitch,
        m_sd_get_name: m_sd_name,
        m_sd_get_tag_ids,
        f_snp_need_skill_point: f_snp_cost,
        f_tips_group_id: f_gid,
        f_tips_rarity: f_rar,
        f_tips_level: f_lvl,
        f_skill_point: f_sp,
        m_get_avail_skill_set: m_get_avail,
        m_ass_get_list,
        f_ass_skill_id,
        f_ass_need_rank,
        m_chara_get_card_data,
        f_card_avail_set_id,
        f_talent_level,
    })
}

// ---------------------------------------------------------------------------
// Public: read current SP
// ---------------------------------------------------------------------------

pub(crate) fn read_skill_points() -> Option<i32> {
    let r = RESOLVED.get()?;
    if r.f_skill_point.is_null() {
        return None;
    }
    let chara = memory_reader::get_chara_ptr()?;
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    Some(unsafe { decrypt_obscured_int(chara, r.f_skill_point) })
}

// ---------------------------------------------------------------------------
// Core read
// ---------------------------------------------------------------------------

/// Full skill-shop reconstruction (main thread only — via overlay cache).
pub(crate) fn read_skill_shop() -> Vec<SkillShopEntry> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(read_skill_shop_inner)) {
        Ok(v) => v,
        Err(_) => {
            hlog_error!("read_skill_shop PANICKED");
            Vec::new()
        }
    }
}

fn read_skill_shop_inner() -> Vec<SkillShopEntry> {
    if !ensure_resolved() {
        return Vec::new();
    }
    let r = match RESOLVED.get() {
        Some(r) => r,
        None => return Vec::new(),
    };

    let chara = match memory_reader::get_chara_ptr() {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mdm = Sdk::get()
        .get_singleton(r.mdm_klass.cast())
        .map(|p| p.cast::<c_void>())
        .unwrap_or(std::ptr::null_mut());
    if mdm.is_null() {
        hlog_warn!("MasterDataManager singleton is null");
        return Vec::new();
    }

    // Get master data table instances
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let msd = unsafe { call_obj(mdm, r.m_get_master_skill_data) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let snp = unsafe { call_obj(mdm, r.m_get_skill_need_point) };
    if msd.is_null() {
        hlog_warn!("MasterSkillData is null");
        return Vec::new();
    }

    // Learned IDs
    let learned = memory_reader::read_acquired_skills();
    let learned_ids: Vec<i32> = learned.iter().map(|s| s.master_id).collect();

    // Read tips
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let (list_ptr, count, m_get_item) = match unsafe { memory_reader::read_list_field(chara, c"_skillTipsList") } {
        Some(v) => v,
        None => return Vec::new(),
    };
    if count <= 0 || count > 500 {
        return Vec::new();
    }

    let mut entries = Vec::new();

    for i in 0..count {
        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let item = unsafe { call_obj_i32(list_ptr, m_get_item, i) };
        if item.is_null() {
            continue;
        }

        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let group_id = unsafe { decrypt_obscured_int(item, r.f_tips_group_id) };
        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let tip_rarity = unsafe { decrypt_obscured_int(item, r.f_tips_rarity) };
        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let level = unsafe { decrypt_obscured_int(item, r.f_tips_level) };

        // Pick the next-buyable variant in this group: the lowest (rarity,
        // group_rate) not yet learned. This walks the purchase sequence
        // ○→◎→🌟 (or ○→🌟), so a hinted upper skill whose prerequisites are
        // unlearned resolves to the next prerequisite that must be bought.
        // SAFETY: msd is a valid MasterSkillData; group_id comes from the tip.
        let Some((sd, skill_id, picked_rarity, is_learned)) =
            (unsafe { pick_group_variant(msd, group_id, &learned_ids, r) })
        else {
            continue;
        };

        // The hint discount applies only to the variant the tip targets; a lower
        // prerequisite in the sequence is shown at full price.
        let (has_hint, hint_level) = if picked_rarity == tip_rarity {
            (true, level)
        } else {
            (false, 0)
        };

        // SAFETY: sd is a valid SkillData from the group's list.
        let name = unsafe { read_string(call_obj(sd, r.m_sd_get_name)) }.unwrap_or_default();
        // SAFETY: snp table + skill row are valid master-data pointers.
        let base_cost = unsafe { skill_need_point(skill_id, snp, r) };
        // SAFETY: sd is a valid SkillData from the group's list.
        let (tags, filter_switch) = unsafe { read_skill_tags(sd, r) };

        entries.push(SkillShopEntry {
            skill_id,
            group_id,
            rarity: picked_rarity,
            hint_level,
            name,
            base_cost,
            is_learned,
            has_hint,
            tags,
            filter_switch,
        });
    }

    // Always merge the trainee's innate available skills (no in-game shop visit
    // needed), so the panel shows both hinted and innate-buyable skills.
    merge_innate_entries(&mut entries, msd, snp, r, &learned_ids, chara);

    let prefs = crate::skill_shop_prefs::prefs();
    if prefs.show_hintless {
        merge_hintless_entries(&mut entries, msd, snp, r, &learned_ids);
    }

    sort_shop_entries(&mut entries, prefs.sort_mode);
    entries
}

// ---------------------------------------------------------------------------
// Per-skill master data reads
// ---------------------------------------------------------------------------

unsafe fn read_skill_tags(sd: *mut c_void, r: &Resolved) -> (Vec<i32>, i32) {
    let filter_switch = if r.f_sd_filter_switch.is_null() {
        0
    } else {
        // SAFETY: Plain Int32 field on master SkillData.
        unsafe { read_field_i32(sd, r.f_sd_filter_switch) }
    };

    let tags = if r.m_sd_get_tag_ids.is_null() {
        Vec::new()
    } else {
        // SAFETY: GetTagIds on master SkillData row.
        let list = unsafe { call_obj(sd, r.m_sd_get_tag_ids) };
        // SAFETY: List pointer from GetTagIds on valid SkillData.
        unsafe { read_i32_list(list) }
    };

    (tags, filter_switch)
}

unsafe fn skill_need_point(skill_id: i32, snp: *mut c_void, r: &Resolved) -> i32 {
    if snp.is_null() {
        return 0;
    }
    // SAFETY: MasterSingleModeSkillNeedPoint.Get(skill_id).
    let row = unsafe { call_obj_i32(snp, r.m_snp_get, skill_id) };
    if row.is_null() {
        return 0;
    }
    // SAFETY: NeedSkillPoint field on row.
    unsafe { read_field_i32(row, r.f_snp_need_skill_point) }
}

unsafe fn build_entry_from_skill_data(
    sd: *mut c_void,
    snp: *mut c_void,
    r: &Resolved,
    learned_ids: &[i32],
    has_hint: bool,
    hint_level: i32,
) -> Option<SkillShopEntry> {
    if sd.is_null() {
        return None;
    }
    // SAFETY: Master SkillData fields.
    let skill_id = unsafe { read_field_i32(sd, r.f_sd_id) };
    // SAFETY: sd is valid MasterSkillData.SkillData from Get or list item.
    let (rarity, group_id, group_rate) = unsafe {
        (
            read_field_i32(sd, r.f_sd_rarity),
            read_field_i32(sd, r.f_sd_group_id),
            read_field_i32(sd, r.f_sd_group_rate),
        )
    };
    if group_rate <= 0 {
        return None;
    }
    let is_learned = learned_ids.contains(&skill_id);
    // SAFETY: get_Name on SkillData.
    let name = unsafe { read_string(call_obj(sd, r.m_sd_get_name)) }.unwrap_or_default();
    // SAFETY: snp table and sd row are valid master-data pointers.
    let base_cost = unsafe { skill_need_point(skill_id, snp, r) };
    // SAFETY: Tag list from GetTagIds on the same SkillData row.
    let (tags, filter_switch) = unsafe { read_skill_tags(sd, r) };
    Some(SkillShopEntry {
        skill_id,
        group_id,
        rarity,
        hint_level,
        name,
        base_cost,
        is_learned,
        has_hint,
        tags,
        filter_switch,
    })
}

/// Collect a group's buyable variants (`group_rate > 0`) as `(sd, candidate)`.
unsafe fn group_candidates(msd: *mut c_void, group_id: i32, r: &Resolved) -> Vec<(*mut c_void, SkillCandidate)> {
    // SAFETY: GetListWithGroupIdOrderByIdAsc(group_id) → List<SkillData>.
    let skill_list = unsafe { call_obj_i32(msd, r.m_msd_get_list_by_group, group_id) };
    if skill_list.is_null() {
        return Vec::new();
    }
    // SAFETY: IL2CPP list object layout — klass pointer at object head.
    let list_klass = unsafe { *(skill_list as *const *mut c_void) };
    let sdk = Sdk::get();
    let (Some(m_cnt), Some(m_itm)) = (
        sdk.get_method(list_klass.cast(), "get_Count", 0),
        sdk.get_method(list_klass.cast(), "get_Item", 1),
    ) else {
        return Vec::new();
    };
    if m_cnt.is_null() || m_itm.is_null() {
        return Vec::new();
    }
    // SAFETY: get_Count on the resolved list.
    let count = unsafe { call_i32(skill_list, m_cnt) };

    let mut candidates: Vec<(*mut c_void, SkillCandidate)> = Vec::new();
    for j in 0..count.min(20) {
        // SAFETY: get_Item(j) for j in [0, count) returns a SkillData row.
        let sd = unsafe { call_obj_i32(skill_list, m_itm, j) };
        if sd.is_null() {
            continue;
        }
        // SAFETY: group_rate is a plain Int32 field on SkillData.
        let group_rate = unsafe { read_field_i32(sd, r.f_sd_group_rate) };
        if group_rate <= 0 {
            continue; // skip × debuff variants
        }
        // SAFETY: rarity is a plain Int32 field on SkillData.
        let rarity = unsafe { read_field_i32(sd, r.f_sd_rarity) };
        // SAFETY: id is a plain Int32 field on SkillData.
        let skill_id = unsafe { read_field_i32(sd, r.f_sd_id) };
        candidates.push((
            sd,
            SkillCandidate {
                skill_id,
                rarity,
                group_rate,
            },
        ));
    }
    candidates
}

/// Pick the next-buyable variant in a group: the lowest `(rarity, group_rate)`
/// not yet learned (else the top, marked learned). This walks the purchase
/// sequence ○→◎→🌟 (3-variant) or ○→🌟 (2-variant) — each tier is the
/// prerequisite for the next, derived from the group itself (the middle ◎ is
/// part of the chain even when it is absent from `available_skill_set`).
/// Returns `(sd, skill_id, rarity, is_learned)`.
unsafe fn pick_group_variant(
    msd: *mut c_void,
    group_id: i32,
    learned_ids: &[i32],
    r: &Resolved,
) -> Option<(*mut c_void, i32, i32, bool)> {
    // SAFETY: group expansion on a valid MasterSkillData.
    let candidates = unsafe { group_candidates(msd, group_id, r) };
    let pure: Vec<SkillCandidate> = candidates.iter().map(|(_, c)| c.clone()).collect();
    let (skill_id, is_learned) = pick_best_variant(&pure, learned_ids)?;
    let &(sd, ref c) = candidates.iter().find(|(_, c)| c.skill_id == skill_id)?;
    Some((sd, skill_id, c.rarity, is_learned))
}

/// Merge the trainee's innate available skills (from `MasterAvailableSkillSet`,
/// gated by the trainee's talent level via each row's `NeedRank`). Sourced from
/// master data, so it does not require opening the in-game skill shop. Dedups by
/// skill id and group id so hinted entries (which carry the discount) win.
fn merge_innate_entries(
    entries: &mut Vec<SkillShopEntry>,
    msd: *mut c_void,
    snp: *mut c_void,
    r: &Resolved,
    learned_ids: &[i32],
    chara: *mut c_void,
) {
    if !r.has_innate() {
        return;
    }

    // Trainee card → available-skill-set id.
    // SAFETY: get_CardData on the live chara returns the cached MasterCardData.CardData.
    let card_data = unsafe { call_obj(chara, r.m_chara_get_card_data) };
    if card_data.is_null() {
        return;
    }
    // SAFETY: AvailableSkillSetId is a plain Int32 field on CardData.
    let avail_set_id = unsafe { read_field_i32(card_data, r.f_card_avail_set_id) };
    if avail_set_id <= 0 {
        return;
    }
    // Talent level gates which innate skills are unlocked (NeedRank <= talent).
    let talent_level = if r.f_talent_level.is_null() {
        i32::MAX
    } else {
        // SAFETY: <TalentLevel>k__BackingField is an ObscuredInt on the chara.
        unsafe { decrypt_obscured_int(chara, r.f_talent_level) }
    };

    // MasterDataManager singleton → MasterAvailableSkillSet table.
    let mdm = Sdk::get()
        .get_singleton(r.mdm_klass.cast())
        .map(|p| p.cast::<c_void>())
        .unwrap_or(std::ptr::null_mut());
    if mdm.is_null() {
        return;
    }
    // SAFETY: get_masterAvailableSkillSet on the MasterDataManager singleton.
    let ass = unsafe { call_obj(mdm, r.m_get_avail_skill_set) };
    if ass.is_null() {
        return;
    }
    // SAFETY: GetListWithAvailableSkillSetIdOrderByIdAsc(int) → List<AvailableSkillSet>.
    let list = unsafe { call_obj_i32(ass, r.m_ass_get_list, avail_set_id) };
    if list.is_null() {
        return;
    }

    // SAFETY: IL2CPP list object layout — klass pointer at object head.
    let list_klass = unsafe { *(list as *const *mut c_void) };
    let sdk = Sdk::get();
    let (Some(m_cnt), Some(m_itm)) = (
        sdk.get_method(list_klass.cast(), "get_Count", 0),
        sdk.get_method(list_klass.cast(), "get_Item", 1),
    ) else {
        return;
    };
    let (m_cnt, m_itm) = (m_cnt.cast::<c_void>(), m_itm.cast::<c_void>());
    if m_cnt.is_null() || m_itm.is_null() {
        return;
    }
    // SAFETY: get_Count on the resolved list.
    let count = unsafe { call_i32(list, m_cnt) };
    if count <= 0 || count > 512 {
        return;
    }

    let mut seen_ids: HashSet<i32> = entries.iter().map(|e| e.skill_id).collect();
    let mut seen_groups: HashSet<i32> = entries.iter().map(|e| e.group_id).collect();

    for i in 0..count {
        // SAFETY: get_Item(i) for i in [0, count) returns an AvailableSkillSet row.
        let row = unsafe { call_obj_i32(list, m_itm, i) };
        if row.is_null() {
            continue;
        }
        // SAFETY: NeedRank / SkillId are plain Int32 fields on AvailableSkillSet.
        let need_rank = unsafe { read_field_i32(row, r.f_ass_need_rank) };
        if need_rank > talent_level {
            continue;
        }
        // SAFETY: SkillId field on the same row.
        let skill_id = unsafe { read_field_i32(row, r.f_ass_skill_id) };
        if skill_id <= 0 || seen_ids.contains(&skill_id) {
            continue;
        }
        // SAFETY: MasterSkillData.Get(skill_id) → SkillData row (for its group).
        let sd0 = unsafe { call_obj_i32(msd, r.m_msd_get, skill_id) };
        if sd0.is_null() {
            continue;
        }
        // SAFETY: GroupId on SkillData.
        let group_id = unsafe { read_field_i32(sd0, r.f_sd_group_id) };
        if seen_groups.contains(&group_id) {
            continue;
        }
        // Walk the group's purchase sequence (○→◎→🌟): show the next-buyable
        // tier, so an innate gold whose prerequisites are unlearned shows the
        // prerequisite first (and the gold resurfaces once they are bought).
        // SAFETY: msd valid; group_id from the resolved SkillData.
        let Some((sd, _pid, _rarity, _learned)) = (unsafe { pick_group_variant(msd, group_id, learned_ids, r) }) else {
            continue;
        };
        // SAFETY: build full (no-hint) entry from the picked variant.
        if let Some(entry) = unsafe { build_entry_from_skill_data(sd, snp, r, learned_ids, false, 0) } {
            seen_ids.insert(entry.skill_id);
            seen_groups.insert(entry.group_id);
            entries.push(entry);
        }
    }
}

fn merge_hintless_entries(
    entries: &mut Vec<SkillShopEntry>,
    msd: *mut c_void,
    snp: *mut c_void,
    r: &Resolved,
    learned_ids: &[i32],
) {
    let hinted_groups: HashSet<i32> = entries.iter().map(|e| e.group_id).collect();
    let hinted_ids: HashSet<i32> = entries.iter().map(|e| e.skill_id).collect();
    let visible = shop_hooks::visible_skill_ids();
    if visible.is_empty() {
        return;
    }

    for skill_id in visible {
        if hinted_ids.contains(&skill_id) {
            continue;
        }
        // SAFETY: MasterSkillData.Get(skill_id).
        let sd = unsafe { call_obj_i32(msd, r.m_msd_get, skill_id) };
        if sd.is_null() {
            continue;
        }
        // SAFETY: GroupId on SkillData.
        let group_id = unsafe { read_field_i32(sd, r.f_sd_group_id) };
        if hinted_groups.contains(&group_id) {
            continue;
        }
        // SAFETY: Build full-price row from master data.
        if let Some(entry) = unsafe { build_entry_from_skill_data(sd, snp, r, learned_ids, false, 0) } {
            entries.push(entry);
        }
    }
}
