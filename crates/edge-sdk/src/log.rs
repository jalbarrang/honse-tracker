//! Host `log` forwarding macros and a `log::Log` adapter.

use std::ffi::CString;

use crate::api::Api;

/// Edge `log` level constants (`plugin_api.rs` match arms 1..=5).
pub mod log_level {
    pub const ERROR: i32 = 1;
    pub const WARN: i32 = 2;
    pub const INFO: i32 = 3;
    pub const DEBUG: i32 = 4;
    pub const TRACE: i32 = 5;
}

/// Forward a log line to edge's `log` get_api fn. No-ops before `Api::init`.
pub fn host_log(level: i32, target: &str, message: &str) {
    let Some(api) = Api::try_get() else {
        return;
    };
    let Some(f) = api.log else {
        return;
    };
    let (Ok(target_c), Ok(msg_c)) = (CString::new(target), CString::new(message)) else {
        return;
    };
    // SAFETY: edge copies the C strings into its logger; pointers valid for the call.
    unsafe { f(level, target_c.as_ptr(), msg_c.as_ptr()) };
}

/// Log through the host. Optional `target: "name"` overrides the default.
#[macro_export]
macro_rules! hlog {
    (target: $target:literal, $level:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        $crate::log::host_log($level, $target, &msg);
    }};
    ($level:expr, $($arg:tt)*) => {
        $crate::hlog!(target: "edge-sdk", $level, $($arg)*)
    };
}

#[macro_export]
macro_rules! hlog_info {
    (target: $target:literal, $($arg:tt)*) => {
        $crate::hlog!(target: $target, $crate::log::log_level::INFO, $($arg)*)
    };
    ($($arg:tt)*) => {
        $crate::hlog!($crate::log::log_level::INFO, $($arg)*)
    };
}

#[macro_export]
macro_rules! hlog_warn {
    (target: $target:literal, $($arg:tt)*) => {
        $crate::hlog!(target: $target, $crate::log::log_level::WARN, $($arg)*)
    };
    ($($arg:tt)*) => {
        $crate::hlog!($crate::log::log_level::WARN, $($arg)*)
    };
}

#[macro_export]
macro_rules! hlog_error {
    (target: $target:literal, $($arg:tt)*) => {
        $crate::hlog!(target: $target, $crate::log::log_level::ERROR, $($arg)*)
    };
    ($($arg:tt)*) => {
        $crate::hlog!($crate::log::log_level::ERROR, $($arg)*)
    };
}

#[macro_export]
macro_rules! hlog_debug {
    (target: $target:literal, $($arg:tt)*) => {
        $crate::hlog!(target: $target, $crate::log::log_level::DEBUG, $($arg)*)
    };
    ($($arg:tt)*) => {
        $crate::hlog!($crate::log::log_level::DEBUG, $($arg)*)
    };
}

#[macro_export]
macro_rules! hlog_trace {
    (target: $target:literal, $($arg:tt)*) => {
        $crate::hlog!(target: $target, $crate::log::log_level::TRACE, $($arg)*)
    };
    ($($arg:tt)*) => {
        $crate::hlog!($crate::log::log_level::TRACE, $($arg)*)
    };
}

/// `log` crate adapter that forwards to edge's `log` API.
pub struct HostLogger;

impl log::Log for HostLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        Api::try_get().is_some()
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            log::Level::Error => log_level::ERROR,
            log::Level::Warn => log_level::WARN,
            log::Level::Info => log_level::INFO,
            log::Level::Debug => log_level::DEBUG,
            log::Level::Trace => log_level::TRACE,
        };
        host_log(level, record.target(), &format!("{}", record.args()));
    }

    fn flush(&self) {}
}

/// Install [`HostLogger`] as the global `log` logger (idempotent-ish; ignores AlreadyInitialized).
pub fn install_logger() {
    let _ = log::set_logger(&HostLogger);
    log::set_max_level(log::LevelFilter::Trace);
}
