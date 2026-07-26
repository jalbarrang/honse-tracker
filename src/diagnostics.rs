//! Runtime IL2CPP class/field diagnostics.
//! Dumps field and method info for career-related classes to the log.
//!
//! Dev-only tooling: call [`run_diagnostics`] / [`dump_skill_classes`] from a menu hook when needed.
#![allow(dead_code)]

use std::ffi::{c_void, CStr};

use crate::compat::Sdk;

/// Minimal FieldInfo layout for reading field names and types.
#[repr(C)]
struct FieldInfoCompat {
    name: *const std::ffi::c_char,
    type_: *const c_void, // Il2CppType*
}

/// Minimal MethodInfo layout (same as in hooks.rs).
#[repr(C)]
struct MethodInfoCompat {
    method_pointer: usize,
    virtual_method_pointer: usize,
    invoker_method: usize,
    name: *const std::ffi::c_char,
    klass: *mut c_void,
    return_type: *const c_void,
    parameters: *mut c_void,
    _union1: usize,
    _union2: usize,
    token: u32,
    flags: u16,
    iflags: u16,
    slot: u16,
    parameters_count: u8,
}

/// Resolved IL2CPP runtime functions for type introspection.
struct TypeIntrospection {
    type_get_name: unsafe extern "C" fn(type_: *const c_void) -> *mut std::ffi::c_char,
    class_get_name: unsafe extern "C" fn(klass: *mut c_void) -> *const std::ffi::c_char,
    class_get_fields: unsafe extern "C" fn(klass: *mut c_void, iter: *mut *mut c_void) -> *mut c_void,
    il2cpp_free: unsafe extern "C" fn(ptr: *mut c_void),
}

impl TypeIntrospection {
    /// Resolve IL2CPP runtime functions via il2cpp_resolve_symbol.
    /// Returns None if any symbol fails to resolve.
    fn resolve() -> Option<Self> {
        let sdk = Sdk::get();
        let type_get_name = sdk.resolve_symbol("il2cpp_type_get_name")?;
        let class_get_name = sdk.resolve_symbol("il2cpp_class_get_name")?;
        let class_get_fields = sdk.resolve_symbol("il2cpp_class_get_fields")?;
        let free_fn = sdk.resolve_symbol("il2cpp_free")?;

        // SAFETY: Resolved symbols match il2cpp runtime export signatures.
        Some(unsafe {
            Self {
                type_get_name: std::mem::transmute(type_get_name),
                class_get_name: std::mem::transmute(class_get_name),
                class_get_fields: std::mem::transmute(class_get_fields),
                il2cpp_free: std::mem::transmute(free_fn),
            }
        })
    }

    /// Get the name of an Il2CppType. Returns "?" on failure.
    fn type_name(&self, type_ptr: *const c_void) -> String {
        if type_ptr.is_null() {
            return "void".to_string();
        }
        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        unsafe {
            let name_ptr = (self.type_get_name)(type_ptr);
            if name_ptr.is_null() {
                return "?".to_string();
            }
            let name = CStr::from_ptr(name_ptr).to_str().unwrap_or("?").to_string();
            // il2cpp_type_get_name returns an allocated string, free it
            (self.il2cpp_free)(name_ptr as *mut c_void);
            name
        }
    }
}

/// Classes to probe for career/training state.
const PROBE_CLASSES: &[(&CStr, &CStr, &CStr)] = &[
    // (assembly, namespace, class)
    (c"umamusume.dll", c"Gallop", c"SingleModeMainViewController"),
    (c"umamusume.dll", c"Gallop", c"TrainingView"),
    (c"umamusume.dll", c"Gallop", c"TrainingController"),
    (c"umamusume.dll", c"Gallop", c"SingleModeChara"),
    (c"umamusume.dll", c"Gallop", c"SingleModeHomeInfo"),
    (c"umamusume.dll", c"Gallop", c"SingleModeCommandInfo"),
    (c"umamusume.dll", c"Gallop", c"TrainingLevelInfo"),
    (c"umamusume.dll", c"Gallop", c"WorkSingleModeData"),
    (c"umamusume.dll", c"Gallop", c"WorkSingleModeCharaData"),
    (c"umamusume.dll", c"Gallop", c"WorkSingleModeHomeInfo"),
    // Manager / controller singletons that might hold WorkSingleMode* references
    (c"umamusume.dll", c"Gallop", c"SingleModeSceneController"),
    (c"umamusume.dll", c"Gallop", c"GameSystem"),
    (c"umamusume.dll", c"Gallop", c"GameManager"),
    (c"umamusume.dll", c"Gallop", c"SingleModeManager"),
    (c"umamusume.dll", c"Gallop", c"SingleModeWorkDataManager"),
    (c"umamusume.dll", c"Gallop", c"WorkDataManager"),
    (c"umamusume.dll", c"Gallop", c"WorkManager"),
    (c"umamusume.dll", c"Gallop", c"ViewManager"),
    (c"umamusume.dll", c"Gallop", c"SceneManager"),
    (c"umamusume.dll", c"Gallop", c"UIManager"),
    (c"umamusume.dll", c"Gallop", c"MasterDataManager"),
    (c"umamusume.dll", c"Gallop", c"SingleModeGameSystem"),
    (c"umamusume.dll", c"Gallop", c"SingleModeContext"),
    (c"umamusume.dll", c"Gallop", c"SingleModeDataManager"),
];

/// Known field names worth probing.
/// The plugin API does not expose field iteration, so this is a targeted probe.
const PROBE_FIELD_NAMES: &[&CStr] = &[
    // SingleModeMainViewController likely fields
    c"_instance",
    c"_commandId",
    c"_commandType",
    c"_currentCommandId",
    c"_trainingCommandId",
    c"_selectedCommandId",
    c"_singleModeData",
    c"_singleModeCharaData",
    c"_trainingLevelDic",
    c"_trainingPartnerInfoArray",
    c"_turnInfo",
    c"_currentTurn",
    c"_turn",
    // SingleModeChara / HomeInfo / CommandInfo fields
    c"turn",
    c"command_id",
    c"command_type",
    c"level",
    c"training_level_info_array",
    c"command_info_array",
    c"disable_command_id_array",
    c"training_partner_array",
    c"failure_rate",
    c"chara_id",
    c"scenario_id",
    c"speed",
    c"stamina",
    c"power",
    c"guts",
    c"wiz",
    c"skill_point",
    c"vital",
    c"max_vital",
    c"motivation",
    c"fans",
    c"is_playing",
    // Manager fields that might hold WorkSingleMode* references
    c"_data",
    c"_workData",
    c"_singleModeWorkData",
    c"_workSingleModeData",
    c"_singleModeData",
    c"_charaData",
    c"_workCharaData",
    c"_homeInfo",
    c"_workHomeInfo",
    c"_mainViewController",
    c"_controller",
    c"_viewController",
    c"_model",
    c"_context",
    c"_currentData",
    // Common property backing fields
    c"<SelectedTrainingCommandId>k__BackingField",
    c"<TrainingCommandId>k__BackingField",
    c"<IsInTraining>k__BackingField",
    c"<TrainingStatus>k__BackingField",
    c"<TrainingLevel>k__BackingField",
    c"<TrainingRank>k__BackingField",
    c"<Instance>k__BackingField",
    c"<Data>k__BackingField",
    c"<CurrentData>k__BackingField",
    c"<WorkData>k__BackingField",
    c"<Character>k__BackingField",
    c"<HomeInfo>k__BackingField",
];

/// Dump all methods on a class to the log, including return type names.
fn dump_methods(class_label: &str, klass: *mut c_void, introspect: Option<&TypeIntrospection>) {
    let sdk = Sdk::get();
    let mut iter: *mut c_void = std::ptr::null_mut();
    let mut count = 0u32;

    hlog_info!("  Methods on {}:", class_label);
    loop {
        let method = sdk.class_get_methods(klass.cast(), &mut iter);
        if method.is_null() {
            break;
        }

        // SAFETY: MethodInfoCompat matches the leading MethodInfo fields used here.
        unsafe {
            let mi = &*(method as *const MethodInfoCompat);
            if !mi.name.is_null() {
                if let Ok(name) = CStr::from_ptr(mi.name).to_str() {
                    let ret_type = introspect
                        .map(|i| i.type_name(mi.return_type))
                        .unwrap_or_else(|| "?".to_string());
                    hlog_info!("    method: {} {}({} args)", ret_type, name, mi.parameters_count);
                }
            }
        }

        count += 1;
        if count > 300 {
            hlog_warn!("    ... truncated at 300 methods");
            break;
        }
    }

    hlog_info!("  {} total methods on {}", count, class_label);
}

/// Dump ALL fields on a class via il2cpp_class_get_fields iteration.
fn dump_all_fields(class_label: &str, klass: *mut c_void, introspect: &TypeIntrospection) {
    let mut iter: *mut c_void = std::ptr::null_mut();
    let mut count = 0u32;

    hlog_info!("  All fields on {}:", class_label);
    loop {
        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let field = unsafe { (introspect.class_get_fields)(klass, &mut iter) };
        if field.is_null() {
            break;
        }

        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        unsafe {
            let fi = &*(field as *const FieldInfoCompat);
            let field_name = if fi.name.is_null() {
                "?"
            } else {
                CStr::from_ptr(fi.name).to_str().unwrap_or("?")
            };
            let type_name = introspect.type_name(fi.type_);
            hlog_info!("    field: {} {} (FieldInfo={:?})", type_name, field_name, field);
        }

        count += 1;
        if count > 200 {
            hlog_warn!("    ... truncated at 200 fields");
            break;
        }
    }

    hlog_info!("  {} total fields on {}", count, class_label);
}

fn field_name_from_info(field: *mut c_void) -> &'static str {
    // SAFETY: FieldInfoCompat matches the leading FieldInfo field used here.
    unsafe {
        let fi = &*(field as *const FieldInfoCompat);
        if fi.name.is_null() {
            return "?";
        }
        CStr::from_ptr(fi.name).to_str().unwrap_or("?")
    }
}

fn probe_fields(class_label: &str, klass: *mut c_void) {
    let sdk = Sdk::get();
    let mut found = 0u32;

    hlog_info!("  Probing fields on {}:", class_label);
    for name_bytes in PROBE_FIELD_NAMES {
        let Some(field) = name_bytes
            .to_str()
            .ok()
            .and_then(|n| sdk.get_field_from_name(klass.cast(), n))
        else {
            continue;
        };
        let field_name = field_name_from_info(field.cast());
        hlog_info!("    ✓ field found: {} (FieldInfo={:?})", field_name, field);
        found += 1;
    }

    hlog_info!(
        "  {}/{} probe fields found on {}",
        found,
        PROBE_FIELD_NAMES.len(),
        class_label
    );
}

/// Check if a class has a singleton-like `_instance` static field and try to get the instance.
fn probe_singleton(class_label: &str, klass: *mut c_void) {
    if let Some(instance) = Sdk::get().get_singleton(klass.cast()) {
        hlog_info!("  ★ {} has LIVE singleton instance at {:?}", class_label, instance);
    } else {
        hlog_info!("  {} — no singleton instance (null or no _instance field)", class_label);
    }
}

/// Deep-dive dump for a specific class: full field iteration + methods with return types.
fn deep_dive_class(label: &str, klass: *mut c_void, introspect: &TypeIntrospection) {
    hlog_info!("╔══ DEEP DIVE: {} (klass={:?}) ══╗", label, klass);
    probe_singleton(label, klass);
    dump_all_fields(label, klass, introspect);
    dump_methods(label, klass, Some(introspect));
    hlog_info!("╚══ END DEEP DIVE: {} ══╝", label);
}

/// Classes that get the full deep-dive treatment (field iteration + return types).
const DEEP_DIVE_CLASSES: &[(&CStr, &CStr, &CStr)] = &[
    (c"umamusume.dll", c"Gallop", c"WorkDataManager"),
    (c"umamusume.dll", c"Gallop", c"WorkSingleModeData"),
    (c"umamusume.dll", c"Gallop", c"WorkSingleModeCharaData"),
    (c"umamusume.dll", c"Gallop", c"WorkSingleModeHomeInfo"),
    (c"umamusume.dll", c"Gallop", c"SingleModeManager"),
    (c"umamusume.dll", c"Gallop", c"SingleModeWorkDataManager"),
    (c"umamusume.dll", c"Gallop", c"SingleModeContext"),
    (c"umamusume.dll", c"Gallop", c"SingleModeDataManager"),
    (c"umamusume.dll", c"Gallop", c"AcquiredSkill"),
    (c"umamusume.dll", c"Gallop", c"SkillTips"),
    (c"umamusume.dll", c"Gallop", c"SkillData"),
    (c"umamusume.dll", c"Gallop", c"SkillManager"),
    (c"umamusume.dll", c"Gallop", c"SkillBase"),
];

/// Run the full diagnostic dump. Call from a menu button click.
pub fn run_diagnostics() {
    let sdk = Sdk::get();

    hlog_info!("=== TRAINING TRACKER DIAGNOSTICS START ===");

    // Resolve type introspection functions for enhanced dumps
    let introspect = TypeIntrospection::resolve();
    if introspect.is_some() {
        hlog_info!("Type introspection: AVAILABLE (will show return types and all fields)");
    } else {
        hlog_warn!("Type introspection: UNAVAILABLE (fallback to basic dump)");
    }

    // Phase 1: Deep-dive on key classes (full field iteration + return types)
    if let Some(ref intro) = introspect {
        hlog_info!("\n=== PHASE 1: DEEP DIVE ON KEY CLASSES ===");
        for &(asm, ns, class) in DEEP_DIVE_CLASSES {
            let class_name = class.to_str().unwrap_or("?");
            let Some(image) = asm.to_str().ok().and_then(|a| sdk.get_assembly_image(a)) else {
                continue;
            };
            let Some(klass) = ns
                .to_str()
                .ok()
                .and_then(|n| sdk.get_class(image, n, class.to_str().unwrap_or("?")))
            else {
                hlog_info!("[DEEP] {} — class NOT FOUND", class_name);
                continue;
            };

            deep_dive_class(class_name, klass.cast(), intro);
        }
    }

    hlog_info!("\n=== PHASE 2: BROAD CLASS SCAN ===");
    for &(asm, ns, class) in PROBE_CLASSES {
        let class_name = class.to_str().unwrap_or("?");

        let Some(image) = asm.to_str().ok().and_then(|a| sdk.get_assembly_image(a)) else {
            hlog_warn!("Assembly not found for {}", class_name);
            continue;
        };

        let Some(klass) = ns
            .to_str()
            .ok()
            .and_then(|n| sdk.get_class(image, n, class.to_str().unwrap_or("?")))
        else {
            hlog_info!("[{}] — class NOT FOUND", class_name);
            continue;
        };

        hlog_info!("[{}] — class FOUND at {:?}", class_name, klass);
        probe_singleton(class_name, klass.cast());
        probe_fields(class_name, klass.cast());
        dump_methods(class_name, klass.cast(), introspect.as_ref());
        hlog_info!("---");
    }

    hlog_info!("=== TRAINING TRACKER DIAGNOSTICS END ===");
}

/// Focused dump of skill-related classes for wiring up the skills panel.
const SKILL_CLASSES: &[(&CStr, &CStr, &CStr)] = &[
    (c"umamusume.dll", c"Gallop", c"AcquiredSkill"),
    (c"umamusume.dll", c"Gallop", c"SkillTips"),
    (c"umamusume.dll", c"Gallop", c"SkillData"),
    (c"umamusume.dll", c"Gallop", c"SkillBase"),
    (c"umamusume.dll", c"Gallop", c"SkillManager"),
    (c"umamusume.dll", c"Gallop", c"SingleModeAcquiredSkill"),
    (c"umamusume.dll", c"Gallop", c"WorkSingleModeSkillData"),
    (c"umamusume.dll", c"Gallop", c"MasterSkillData"),
    (c"umamusume.dll", c"Gallop", c"SkillDataManager"),
    (c"umamusume.dll", c"Gallop", c"WorkSkillData"),
    // Friendship / bond / evaluation classes
    (c"umamusume.dll", c"Gallop", c"EvaluationInfo"),
    (c"umamusume.dll", c"Gallop", c"WorkSingleModeEvaluationInfo"),
    (c"umamusume.dll", c"Gallop", c"SingleModeEvaluationInfo"),
    (c"umamusume.dll", c"Gallop", c"WorkSupportCardData"),
    (c"umamusume.dll", c"Gallop", c"SingleModeSupportCard"),
    (c"umamusume.dll", c"Gallop", c"WorkSingleModeSupportCard"),
    (c"umamusume.dll", c"Gallop", c"SingleModeEvaluation"),
    (c"umamusume.dll", c"Gallop", c"MasterSingleModeEvaluation"),
    (c"umamusume.dll", c"Gallop", c"TrainingPartnerInfo"),
    // Skill cost / master data utilities
    (c"umamusume.dll", c"Gallop", c"MasterDataUtil"),
    (c"umamusume.dll", c"Gallop", c"MasterSingleModeSkillNeedPoint"),
    // Master data instance holders
    (c"umamusume.dll", c"Gallop", c"MasterDataManager"),
    (c"umamusume.dll", c"Gallop", c"MasterHolder"),
    (c"umamusume.dll", c"Gallop", c"MasterBanker"),
    (c"umamusume.dll", c"Gallop", c"MasterSingleModeDatabase"),
    (c"umamusume.dll", c"Gallop", c"MasterCardDatabase"),
];

pub fn dump_skill_classes() {
    let sdk = Sdk::get();
    hlog_info!("=== SKILL CLASS DIAGNOSTICS START ===");

    let introspect = TypeIntrospection::resolve();
    if introspect.is_none() {
        hlog_warn!("Type introspection unavailable — field types won't be shown");
    }

    for &(asm, ns, class) in SKILL_CLASSES {
        let class_name = class.to_str().unwrap_or("?");

        let Some(image) = asm.to_str().ok().and_then(|a| sdk.get_assembly_image(a)) else {
            continue;
        };
        let Some(klass) = ns
            .to_str()
            .ok()
            .and_then(|n| sdk.get_class(image, n, class.to_str().unwrap_or("?")))
        else {
            hlog_info!("[SKILL] {} — NOT FOUND", class_name);
            continue;
        };

        if let Some(ref intro) = introspect {
            deep_dive_class(class_name, klass.cast(), intro);
        } else {
            hlog_info!("[SKILL] {} — FOUND at {:?}", class_name, klass);
            dump_methods(class_name, klass.cast(), None);
        }
    }

    // Phase 2: Introspect the actual AcquiredSkill class from a live list element
    hlog_info!("\n=== LIVE ACQUIRED SKILL INTROSPECTION ===");
    if let Some((list_ptr, count)) = crate::memory_reader::read_acquired_skill_list() {
        hlog_info!("_acquiredSkillList: {} items (list={:?})", count, list_ptr);

        if count > 0 {
            // SAFETY: IL2CPP list object layout — klass at head; get_Item from resolved MethodInfo.
            unsafe {
                let list_klass = *(list_ptr as *const *mut c_void);
                let Some(m_get_item) = sdk.get_method(list_klass.cast(), "get_Item", 1) else {
                    hlog_warn!("get_Item not found on list class");
                    return;
                };
                let mi = m_get_item;
                let fp: extern "C" fn(*mut c_void, i32, *const c_void) -> *mut c_void =
                    std::mem::transmute(*(mi as *const usize));
                let first_item = fp(list_ptr, 0, mi);

                if !first_item.is_null() {
                    let item_klass = *(first_item as *const *mut c_void);
                    hlog_info!(
                        "First AcquiredSkill element: obj={:?} klass={:?}",
                        first_item,
                        item_klass
                    );

                    if let Some(ref intro) = introspect {
                        let class_name_ptr = (intro.class_get_name)(item_klass);
                        let class_name = if !class_name_ptr.is_null() {
                            CStr::from_ptr(class_name_ptr).to_str().unwrap_or("?")
                        } else {
                            "?"
                        };
                        hlog_info!("AcquiredSkill runtime class name: {}", class_name);
                        deep_dive_class(class_name, item_klass, intro);

                        if let Some(il2cpp_class_get_parent) = sdk.resolve_symbol("il2cpp_class_get_parent") {
                            let get_parent: extern "C" fn(*mut c_void) -> *mut c_void =
                                std::mem::transmute(il2cpp_class_get_parent);
                            let mut klass = item_klass;
                            for depth in 0..5 {
                                let parent = get_parent(klass);
                                if parent.is_null() || parent == klass {
                                    break;
                                }
                                let pname_ptr = (intro.class_get_name)(parent);
                                let pname = if !pname_ptr.is_null() {
                                    CStr::from_ptr(pname_ptr).to_str().unwrap_or("?")
                                } else {
                                    "?"
                                };
                                hlog_info!("  Parent[{}]: {} (klass={:?})", depth, pname, parent);
                                deep_dive_class(&format!("parent::{}", pname), parent, intro);
                                klass = parent;
                            }
                        }
                    } else {
                        dump_methods("AcquiredSkill(runtime)", item_klass, None);
                    }
                } else {
                    hlog_info!("get_Item(0) returned null");
                }
            }
        }
    } else {
        hlog_info!("No acquired skill list available (not in a career or tracking not started)");
    }

    hlog_info!("\n=== NESTED CLASS PROBE ===");
    if let Some(image) = sdk.get_assembly_image("umamusume.dll") {
        // AcquiredSkill / SkillTips might be nested inside these parent classes
        let parent_candidates: &[(&CStr, &str)] = &[
            (c"WorkSingleModeCharaData", "WorkSingleModeCharaData"),
            (c"SingleModeChara", "SingleModeChara"),
            (c"WorkSingleModeData", "WorkSingleModeData"),
            (c"SingleModeHomeInfo", "SingleModeHomeInfo"),
            (c"MasterSkillData", "MasterSkillData"),
            (c"WorkSkillData", "WorkSkillData"),
            (c"WorkSingleModeHomeInfo", "WorkSingleModeHomeInfo"),
            (c"WorkSupportCardData", "WorkSupportCardData"),
            (c"MasterSingleModeSkillNeedPoint", "MasterSingleModeSkillNeedPoint"),
        ];
        let nested_names: &[(&CStr, &str)] = &[
            (c"AcquiredSkill", "AcquiredSkill"),
            (c"SkillTips", "SkillTips"),
            (c"SkillData", "SkillData"),
            (c"Skill", "Skill"),
            // Friendship / evaluation nested classes
            (c"EvaluationInfo", "EvaluationInfo"),
            (c"Evaluation", "Evaluation"),
            (c"SupportCard", "SupportCard"),
            (c"TrainingPartner", "TrainingPartner"),
            (c"SingleModeSkillNeedPoint", "SingleModeSkillNeedPoint"),
        ];

        for &(parent_bytes, parent_label) in parent_candidates {
            let Some(parent_klass) = parent_bytes
                .to_str()
                .ok()
                .and_then(|p| sdk.get_class(image, "Gallop", p))
            else {
                continue;
            };

            for &(nested_bytes, nested_label) in nested_names {
                let Some(nested) = nested_bytes
                    .to_str()
                    .ok()
                    .and_then(|n| sdk.find_nested_class(parent_klass, n))
                else {
                    continue;
                };
                if !nested.is_null() {
                    hlog_info!(
                        "  \u{2705} FOUND nested: {}.{} at {:?}",
                        parent_label,
                        nested_label,
                        nested
                    );
                    if let Some(ref intro) = introspect {
                        let label = format!("{}.{}", parent_label, nested_label);
                        deep_dive_class(&label, nested.cast(), intro);
                    }
                }
            }
        }
    }

    dump_skill_tag_enum();
    dump_live_skill_tags();

    hlog_info!("=== SKILL CLASS DIAGNOSTICS END ===");
}

/// Log `Gallop.SingleModeDefine.SkillTag` enum constants (for mapping distance/style filters).
pub fn dump_skill_tag_enum() {
    let sdk = Sdk::get();
    let Some(img) = sdk.get_assembly_image("umamusume.dll") else {
        return;
    };
    // SkillTag is nested under Gallop.SingleModeDefine (MasterSkillData.SkillData uses it).
    let tag_klass = sdk
        .get_class(img, "Gallop", "SingleModeDefine")
        .and_then(|parent| sdk.find_nested_class(parent, "SkillTag"));
    let Some(tag_klass) = tag_klass else {
        hlog_info!("[SKILL TAG] Gallop.SingleModeDefine.SkillTag class not found");
        return;
    };

    let Some(introspect) = TypeIntrospection::resolve() else {
        hlog_info!("[SKILL TAG] Type introspection unavailable");
        return;
    };

    hlog_info!("[SKILL TAG] Dumping Gallop.SkillTag enum fields:");
    dump_all_fields("SkillTag", tag_klass.cast(), &introspect);
}

/// Log tag IDs from the first few cached shop/tips skills when in a career.
pub fn dump_live_skill_tags() {
    let entries = crate::skill_shop::read_skill_shop();
    if entries.is_empty() {
        hlog_info!("[SKILL TAG] No shop entries to sample (not in career or no tips)");
        return;
    }
    for entry in entries.iter().take(8) {
        hlog_info!(
            "[SKILL TAG] skill_id={} name={} tags={:?} filter_switch={}",
            entry.skill_id,
            entry.name,
            entry.tags,
            entry.filter_switch
        );
    }
}
