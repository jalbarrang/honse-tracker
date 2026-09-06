//! Every trained character the account owns, read straight out of memory.
//!
//! ```text
//! WorkDataManager (singleton)
//!   → trainedCharaData        (WorkTrainedCharaData)
//!     → dataDic               (Dictionary<int, TrainedCharaData>)
//!       → each value          (one veteran)
//! ```
//!
//! The chain and the field selection are umadump's (`json_encoders.py`,
//! `game_structs.py`); what is *not* umadump's is how any of it is located.
//! umadump has to work from outside the process, so it declares byte-exact
//! ctypes layouts and trusts them. In here every field is asked for by name
//! through the IL2CPP metadata, so a build that inserts a field is followed
//! instead of misread — which is the failure mode that matters, because a
//! wrong offset does not error, it returns a plausible number.
//!
//! # No game code is called
//!
//! Only field reads and `List<T>.get_Item`. Nothing here can mutate account
//! state the way `ConvertWorkSupportCardData` does (see
//! [`super::support_deck`]) — an export must not be able to cost someone their
//! save.
//!
//! # Thread contract
//!
//! Unity main thread. These are live managed objects; reading them from
//! anywhere else is reading memory the GC may be moving.

use std::ffi::c_void;

use crate::compat::Sdk;
use crate::veterans_export::{
    Factor, FactorExtend, RaceResult, Skill, SuccessionChara, SupportCard, Veteran, SELF_POSITION,
};

use super::il2cpp::{
    call_obj_with_i32, dict_values, read_list_field, read_obj_array, read_obscured_bool, read_obscured_int,
    read_obscured_int_array, read_obscured_long, read_obscured_string, read_ref_field,
};

/// Every veteran on the account, or an empty list with the reason in the log.
///
/// Panic-caught like the other readers: a graph that has moved on costs the
/// export, never the game.
pub fn read_veterans() -> Vec<Veteran> {
    // SAFETY: every read below is on resolved metadata / live objects, on the
    // Unity main thread per the module contract.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { inner() })) {
        Ok(v) => v,
        Err(_) => {
            hlog_error!("read_veterans PANICKED");
            Vec::new()
        }
    }
}

unsafe fn inner() -> Vec<Veteran> {
    let sdk = Sdk::get();
    let Some(image) = sdk.get_assembly_image("umamusume.dll") else {
        hlog_warn!(target: "training-tracker", "veterans: umamusume.dll not found");
        return Vec::new();
    };
    let Some(wdm_klass) = sdk.get_class(image, "Gallop", "WorkDataManager") else {
        hlog_warn!(target: "training-tracker", "veterans: WorkDataManager not found");
        return Vec::new();
    };
    let Some(wdm) = sdk.get_singleton(wdm_klass) else {
        hlog_warn!(target: "training-tracker", "veterans: no live WorkDataManager (is the account loaded?)");
        return Vec::new();
    };
    let wdm = wdm.cast::<c_void>();

    // SAFETY: reading a managed reference field off the live manager.
    let work = unsafe { read_ref_field(wdm, &["trainedCharaData"]) };
    if work.is_null() {
        hlog_warn!(target: "training-tracker", "veterans: WorkDataManager.trainedCharaData is null");
        return Vec::new();
    }
    // `dataDic` and not `allDataDic`: the former is the account's own stable,
    // the latter also carries borrowed and ghost entries the player does not
    // own — which is what umadump exports too.
    // SAFETY: `work` is a live WorkTrainedCharaData.
    let dict = unsafe { read_ref_field(work, &["dataDic"]) };
    if dict.is_null() {
        hlog_warn!(target: "training-tracker", "veterans: WorkTrainedCharaData.dataDic is null");
        return Vec::new();
    }

    // SAFETY: `dict` is a live Dictionary<int, TrainedCharaData>.
    let entries = unsafe { dict_values(dict) };
    hlog_info!(target: "training-tracker", "veterans: {} entries in dataDic", entries.len());
    entries
        .into_iter()
        // SAFETY: each value is a live TrainedCharaData the dictionary holds.
        .map(|obj| unsafe { veteran(obj) })
        .collect()
}

/// One `Gallop.WorkTrainedCharaData.TrainedCharaData`.
unsafe fn veteran(obj: *mut c_void) -> Veteran {
    // SAFETY: `obj` is a live TrainedCharaData; every helper below reads one
    // named field off it. Repeated for each read rather than wrapping the
    // whole struct literal, which would hide which reads are the unsafe ones.
    let int = |name: &str| unsafe { read_obscured_int(obj, name) };
    let long = |name: &str| unsafe { read_obscured_long(obj, name) };
    let flag = |name: &str| i32::from(unsafe { read_obscured_bool(obj, name) });

    // SAFETY: as above — the wrapper is a live ObscuredString or null.
    let created = unsafe { read_obscured_string(obj, "createTime") };
    // SAFETY: as above.
    let factors = unsafe { read_ref_field(obj, &["factorDataArray"]) };
    // SAFETY: as above; a plain int32, not obscured.
    let use_type = unsafe { super::il2cpp::read_i32_field_any(obj, &["useType"]) };
    // SAFETY: as above.
    let favorite = unsafe { read_ref_field(obj, &["favoriteData"]) };

    Veteran {
        viewer_id: long("viewerId"),
        trained_chara_id: int("id"),
        owner_viewer_id: long("ownerViewerId"),
        owner_trained_chara_id: int("ownerTrainedCharaId"),
        single_mode_chara_id: 0,
        chara_seed: 0,
        card_id: int("cardId"),
        succession_trained_chara_id_1: 0,
        succession_trained_chara_id_2: 0,
        use_type,
        speed: int("speed"),
        stamina: int("stamina"),
        power: int("power"),
        wiz: int("wiz"),
        guts: int("guts"),
        fans: int("fans"),
        rank_score: int("rankScore"),
        rank: int("rank"),
        scenario_id: int("scenarioId"),
        route_id: 0,
        arrive_route_race_id: 0,
        proper_ground_turf: int("properGroundTurf"),
        proper_ground_dirt: int("properGroundDirt"),
        proper_running_style_nige: int("properRunningStyleNige"),
        proper_running_style_senko: int("properRunningStyleSenko"),
        proper_running_style_sashi: int("properRunningStyleSashi"),
        proper_running_style_oikomi: int("properRunningStyleOikomi"),
        proper_distance_short: int("properDistanceShort"),
        proper_distance_mile: int("properDistanceMile"),
        proper_distance_middle: int("properDistanceMiddle"),
        proper_distance_long: int("properDistanceLong"),
        succession_num: int("successionCount"),
        rarity: int("rarity"),
        is_saved: flag("isSaved"),
        is_locked: flag("isLock"),
        talent_level: int("talentLevel"),
        race_cloth_id: 0,
        chara_grade: int("charaGrade"),
        running_style: int("runningStyle"),
        nickname_id: int("nickNameId"),
        wins: int("singleWinNum"),
        register_time: created.clone(),
        create_time: created,
        // SAFETY: each is a named managed field on `obj`.
        skill_array: unsafe { map_array(obj, "acquiredSkillArray", skill) },
        support_card_list: unsafe { map_array(obj, "supportCardArray", support_card) },
        race_result_list: unsafe { map_array(obj, "singleModeRaceResultArray", race_result) },
        win_saddle_id_array: unsafe { obscured_ints(obj, "winSaddleIdArray") },
        nickname_id_array: unsafe { obscured_ints(obj, "nickNameIdArray") },
        // SAFETY: `factors` is a live FactorData[] or null.
        factor_info_array: unsafe { factor_infos(factors) },
        // SAFETY: as above.
        factor_extend_array: unsafe { factor_extends(SELF_POSITION, factors) },
        // SAFETY: `obj`'s successionCharaList is a List<SuccessionCharaData>.
        succession_chara_array: unsafe { succession_charas(obj) },
        // SAFETY: `favorite` is a live FavoriteData or null.
        icon_type: unsafe { super::il2cpp::read_i32_field_any(favorite, &["type"]) },
        // SAFETY: as above; `memo` is a plain System.String.
        memo: unsafe { super::il2cpp::read_il2cpp_string(read_ref_field(favorite, &["memo"])) }.unwrap_or_default(),
    }
}

/// Read a named reference-typed array field and decode each element.
unsafe fn map_array<T>(obj: *mut c_void, field: &str, decode: unsafe fn(*mut c_void) -> T) -> Vec<T> {
    // SAFETY: reading a named managed field off a live object.
    let array = unsafe { read_ref_field(obj, &[field]) };
    // SAFETY: `array` is a live reference-typed array or null.
    let Some((base, len)) = (unsafe { read_obj_array(array) }) else {
        return Vec::new();
    };
    (0..len)
        // SAFETY: i < len, and each slot holds an object pointer.
        .filter_map(|i| unsafe {
            let element = *base.add(i);
            (!element.is_null()).then(|| decode(element))
        })
        .collect()
}

/// Read a named `ObscuredInt[]` field.
unsafe fn obscured_ints(obj: *mut c_void, field: &str) -> Vec<i32> {
    // SAFETY: reading a named managed field off a live object.
    let array = unsafe { read_ref_field(obj, &[field]) };
    // SAFETY: `array` is a live ObscuredInt[] or null.
    unsafe { read_obscured_int_array(array) }
}

/// `Gallop.WorkSkillData.SkillDataBase.AcquiredSkill`.
unsafe fn skill(obj: *mut c_void) -> Skill {
    Skill {
        // SAFETY: named ObscuredInt fields on a live AcquiredSkill.
        skill_id: unsafe { read_obscured_int(obj, "masterId") },
        // SAFETY: as above.
        level: unsafe { read_obscured_int(obj, "level") },
    }
}

/// `Gallop.WorkTrainedCharaData.SupportCardData`.
unsafe fn support_card(obj: *mut c_void) -> SupportCard {
    // SAFETY: named ObscuredInt fields on a live SupportCardData.
    let int = |name: &str| unsafe { read_obscured_int(obj, name) };
    SupportCard {
        position: int("position"),
        support_card_id: int("supportCardId"),
        exp: int("exp"),
        limit_break_count: int("limitBreakCount"),
    }
}

/// `Gallop.SingleModeUtils.RaceHistoryInfo`.
unsafe fn race_result(obj: *mut c_void) -> RaceResult {
    // SAFETY: named ObscuredInt fields on a live RaceHistoryInfo.
    let int = |name: &str| unsafe { read_obscured_int(obj, name) };
    RaceResult {
        turn: int("turn"),
        program_id: int("programId"),
        weather: int("weather"),
        ground_condition: int("groundCondition"),
        running_style: int("runningStyle"),
        popularity: 0,
        result_rank: int("resultRank"),
        result_time: 0,
        prize_money: 0,
    }
}

/// The `factor_info_array` view of a `FactorData[]`: what each factor is now.
unsafe fn factor_infos(array: *mut c_void) -> Vec<Factor> {
    // SAFETY: `array` is a live FactorData[] or null.
    let Some((base, len)) = (unsafe { read_obj_array(array) }) else {
        return Vec::new();
    };
    (0..len)
        // SAFETY: i < len; each slot holds a FactorData pointer.
        .filter_map(|i| unsafe {
            let factor = *base.add(i);
            (!factor.is_null()).then(|| Factor {
                factor_id: read_obscured_int(factor, "factorId"),
                level: read_obscured_int(factor, "factorLv"),
            })
        })
        .collect()
}

/// The `factor_extend_array` view of the same `FactorData[]`: every upgrade
/// each factor has been through, flattened, tagged with the slot it sits in.
unsafe fn factor_extends(position_id: i32, array: *mut c_void) -> Vec<FactorExtend> {
    // SAFETY: `array` is a live FactorData[] or null.
    let Some((base, len)) = (unsafe { read_obj_array(array) }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in 0..len {
        // SAFETY: i < len; each slot holds a FactorData pointer.
        let factor = unsafe { *base.add(i) };
        if factor.is_null() {
            continue;
        }
        // SAFETY: named field on a live FactorData.
        let base_factor_id = unsafe { read_obscured_int(factor, "baseFactorId") };
        // SAFETY: as above — a List<UpgradeHistory> or nothing.
        let Some((list, count, item)) = (unsafe { read_list_field(factor, c"upgradeHistoryList") }) else {
            continue;
        };
        for index in 0..count {
            // SAFETY: index < count on a live List<T>; `get_Item` is a pure getter.
            let history = unsafe { call_obj_with_i32(list, item, index) };
            if history.is_null() {
                continue;
            }
            out.push(FactorExtend {
                position_id,
                base_factor_id,
                // SAFETY: named fields on a live UpgradeHistory.
                factor_id: unsafe { read_obscured_int(history, "factorId") },
                register_time: crate::veterans_export::jst_timestamp(unsafe {
                    read_obscured_long(history, "upgradeDate")
                }),
            });
        }
    }
    out
}

/// The inherited parents and grandparents, from `successionCharaList`.
unsafe fn succession_charas(obj: *mut c_void) -> Vec<SuccessionChara> {
    // SAFETY: a named List<SuccessionCharaData> field on a live TrainedCharaData.
    let Some((list, count, item)) = (unsafe { read_list_field(obj, c"successionCharaList") }) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for index in 0..count {
        // SAFETY: index < count on a live List<T>; `get_Item` is a pure getter.
        let chara = unsafe { call_obj_with_i32(list, item, index) };
        if chara.is_null() {
            continue;
        }
        // SAFETY: named fields on a live SuccessionCharaData.
        let int = |name: &str| unsafe { read_obscured_int(chara, name) };
        let position_id = int("positionId");
        // SAFETY: as above.
        let factors = unsafe { read_ref_field(chara, &["factorDataArray"]) };
        out.push(SuccessionChara {
            position_id,
            card_id: int("cardId"),
            rank: int("rank"),
            rarity: int("rarity"),
            talent_level: int("level"),
            // SAFETY: `factors` is a live FactorData[] or null.
            factor_info_array: unsafe { factor_infos(factors) },
            // SAFETY: as above.
            factor_extend_array: unsafe { factor_extends(position_id, factors) },
            // SAFETY: named ObscuredInt[] field on a live SuccessionCharaData.
            win_saddle_id_array: unsafe { obscured_ints(chara, "winSaddleIdArray") },
            // SAFETY: named ObscuredLong field, as above.
            owner_viewer_id: unsafe { read_obscured_long(chara, "ownerViewerId") },
        });
    }
    out
}
