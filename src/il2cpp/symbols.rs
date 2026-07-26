//! IL2CPP symbol helpers used by skill purchase (fork `il2cpp::symbols` subset).

use std::marker::PhantomData;

use edge_sdk::ffi::il2cpp_array_size_t;
use edge_sdk::{Api, Sdk};

use super::api::il2cpp_object_new;
use super::types::{
    FieldInfo, Il2CppArray, Il2CppClass, Il2CppDelegate, Il2CppObject, Il2CppThread, MethodInfo,
    DELEGATE_INVOKE_IMPL_OFFSET, DELEGATE_METHOD_PTR_OFFSET, K_IL2CPP_SIZE_OF_ARRAY,
};

/// Write `value` into an instance field.
pub fn set_field_value<T>(obj: *mut Il2CppObject, field: *mut FieldInfo, value: &T) {
    let api = Api::get();
    let Some(f) = api.il2cpp_set_field_value else {
        return;
    };
    // SAFETY: caller guarantees valid obj/field; cast is address-preserving.
    unsafe { f(obj.cast(), field.cast(), (value as *const T).cast()) };
}

/// IL2CPP array wrapper. Element data starts at `this + K_IL2CPP_SIZE_OF_ARRAY`.
#[repr(transparent)]
pub struct Array<T = *mut Il2CppObject> {
    pub this: *mut Il2CppArray,
    _phantom: PhantomData<T>,
}

impl<T> Array<T> {
    #[must_use]
    pub fn new(element_type: *mut Il2CppClass, length: usize) -> Array<T> {
        let api = Api::get();
        let this = if let Some(f) = api.il2cpp_create_array {
            // SAFETY: element_type from get_class; length is the element count.
            unsafe { f(element_type.cast(), length as il2cpp_array_size_t).cast() }
        } else {
            std::ptr::null_mut()
        };
        Array {
            this,
            _phantom: PhantomData,
        }
    }

    /// Pointer to the first element (past the Il2CppArray header).
    ///
    /// # Safety
    /// `this` must be a live array; caller must not overrun `length`.
    #[must_use]
    pub unsafe fn data_ptr(&self) -> *mut T {
        // SAFETY: 64-bit IL2CPP array header is 32 bytes (fork kIl2CppSizeOfArray).
        unsafe { (self.this as *mut u8).add(K_IL2CPP_SIZE_OF_ARRAY) as *mut T }
    }
}

/// Main-thread scheduler façade. Routes through edge `schedule_on_main_thread`.
pub struct Thread(*mut Il2CppThread);

impl Thread {
    #[must_use]
    pub fn main_thread() -> Thread {
        let api = Api::get();
        let ptr = api
            .il2cpp_get_main_thread
            .map(|f| {
                // SAFETY: edge returns the live main Il2CppThread or null.
                unsafe { f().cast() }
            })
            .unwrap_or(std::ptr::null_mut());
        Thread(ptr)
    }

    /// Post `callback` onto the IL2CPP main thread.
    pub fn schedule(&self, callback: fn()) {
        // Edge takes `unsafe extern "C" fn()`; transmute capture-free `fn()` like edge plugin_api.
        let cb: unsafe extern "C" fn() = unsafe { std::mem::transmute(callback) };
        Sdk::get().schedule_on_main_thread(cb);
    }
}

/// Build a `System.Action`-style delegate pointing at `method_ptr`.
pub fn create_delegate(
    delegate_class: *mut Il2CppClass,
    args_count: i32,
    method_ptr: fn(),
) -> Option<*mut Il2CppDelegate> {
    let sdk = Sdk::get();
    let delegate_invoke: *const MethodInfo = sdk.get_method(delegate_class.cast(), "Invoke", args_count)?.cast();
    let delegate_ctor_addr = sdk.get_method_addr(delegate_class.cast(), ".ctor", 2)?;
    let delegate_ctor: extern "C" fn(*mut Il2CppObject, *mut Il2CppObject, *const MethodInfo) =
        // SAFETY: method addr is a live IL2CPP .ctor.
        unsafe { std::mem::transmute(delegate_ctor_addr) };

    let delegate_obj = il2cpp_object_new(delegate_class);
    if delegate_obj.is_null() {
        return None;
    }
    delegate_ctor(delegate_obj, delegate_obj, delegate_invoke);
    // Write method_ptr / invoke_impl at known 64-bit offsets.
    // SAFETY: freshly constructed delegate object from il2cpp_object_new.
    unsafe {
        let base = delegate_obj as *mut u8;
        *(base.add(DELEGATE_METHOD_PTR_OFFSET) as *mut usize) = method_ptr as usize;
        *(base.add(DELEGATE_INVOKE_IMPL_OFFSET) as *mut usize) = method_ptr as usize;
    }
    Some(delegate_obj)
}
