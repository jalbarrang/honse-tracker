//! Plugin entry: `declare_plugin!` exports unmangled `hachimi_init_v3`.

/// Declare the edge plugin entry point.
///
/// Expands to `#[no_mangle] pub extern "C" fn hachimi_init_v3(...)` which:
/// 1. Rejects `version < 3`
/// 2. Calls [`crate::Api::init`]
/// 3. Installs the host log adapter
/// 4. Calls the user's `fn init() -> bool`, mapping `true` → [`crate::ffi::InitResult::Ok`]
///
/// # Example
/// ```ignore
/// edge_sdk::declare_plugin! {
///     fn init() -> bool {
///         true
///     }
/// }
/// ```
#[macro_export]
macro_rules! declare_plugin {
    (
        fn init() -> bool $body:block
    ) => {
        #[no_mangle]
        pub extern "C" fn hachimi_init_v3(
            get_api: $crate::ffi::HachimiGetApiFn,
            version: i32,
        ) -> $crate::ffi::InitResult {
            if version < 3 {
                return $crate::ffi::InitResult::Error;
            }
            $crate::Api::init(get_api);
            $crate::log::install_logger();
            fn __honse_plugin_init() -> bool $body
            if __honse_plugin_init() {
                $crate::ffi::InitResult::Ok
            } else {
                $crate::ffi::InitResult::Error
            }
        }
    };
}
