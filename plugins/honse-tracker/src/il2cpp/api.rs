//! Raw IL2CPP API entry points used by skill purchase.

use edge_sdk::Api;

use super::types::{Il2CppClass, Il2CppObject};

/// Allocate a new IL2CPP object of `klass`.
#[must_use]
pub fn il2cpp_object_new(klass: *const Il2CppClass) -> *mut Il2CppObject {
    let api = Api::get();
    let Some(f) = api.il2cpp_object_new else {
        return std::ptr::null_mut();
    };
    // SAFETY: `klass` must be a valid Il2CppClass from get_class; cast is address-preserving.
    unsafe { f(klass.cast()).cast() }
}
