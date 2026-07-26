//! IL2CPP type aliases matching the fork ABI (`type X = c_void`).
//!
//! Edge-sdk's FFI structs are opaque ZSTs; the tracker casts through `c_void`.
//! Layout-sensitive helpers use explicit byte offsets from the IL2CPP 64-bit ABI.

use std::ffi::c_void;

pub type Il2CppClass = c_void;
pub type Il2CppImage = c_void;
pub type Il2CppObject = c_void;
pub type Il2CppArray = c_void;
pub type Il2CppThread = c_void;
pub type MethodInfo = c_void;
pub type FieldInfo = c_void;
pub type Il2CppMethodPointer = usize;

/// Marker for a System.Delegate / Action instance (raw object pointer).
pub type Il2CppDelegate = Il2CppObject;

/// IL2CPP array header size on 64-bit (fork `kIl2CppSizeOfArray`).
pub const K_IL2CPP_SIZE_OF_ARRAY: usize = 32;

/// Byte offset of `Il2CppDelegate::method_ptr` after the 16-byte object header.
pub const DELEGATE_METHOD_PTR_OFFSET: usize = 16;
/// Byte offset of `Il2CppDelegate::invoke_impl`.
pub const DELEGATE_INVOKE_IMPL_OFFSET: usize = 24;
