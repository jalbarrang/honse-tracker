//! Skill-point memory reader shared by the career overlay and telemetry.

use std::ffi::c_void;
use std::sync::OnceLock;

use crate::compat::Sdk;

use super::chain::get_chara_ptr;
use super::il2cpp::read_obscured_int_field;

struct Resolved {
    field_skill_point: *mut c_void,
}

// SAFETY: IL2CPP field pointers are stable for the process lifetime.
unsafe impl Send for Resolved {}
// SAFETY: IL2CPP field pointers are stable for the process lifetime.
unsafe impl Sync for Resolved {}

static RESOLVED: OnceLock<Resolved> = OnceLock::new();

fn ensure_resolved() -> Option<&'static Resolved> {
    if let Some(resolved) = RESOLVED.get() {
        return Some(resolved);
    }

    let resolved = try_resolve().ok()?;
    let _ = RESOLVED.set(resolved);
    RESOLVED.get()
}

fn try_resolve() -> Result<Resolved, &'static str> {
    let sdk = Sdk::get();
    let Some(image) = sdk.get_assembly_image("umamusume.dll") else {
        return Err("umamusume.dll not found");
    };
    let Some(chara) = sdk.get_class(image, "Gallop", "WorkSingleModeCharaData") else {
        return Err("WorkSingleModeCharaData not found");
    };
    let Some(field) = sdk.get_field_from_name(chara, "<SkillPoint>k__BackingField") else {
        return Err("WorkSingleModeCharaData.SkillPoint not found");
    };

    hlog_info!("Skill-point memory reader resolved");
    Ok(Resolved {
        field_skill_point: field.cast(),
    })
}

/// Read the current career skill-point balance from the active character.
pub(crate) fn read_skill_points() -> Option<i32> {
    let resolved = ensure_resolved()?;
    let chara = get_chara_ptr()?;
    // SAFETY: The field pointer was resolved from WorkSingleModeCharaData and
    // chara is a live active-career object.
    Some(unsafe { read_obscured_int_field(chara, resolved.field_skill_point) })
}
