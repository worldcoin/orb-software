// https://github.com/OP-TEE/optee_os/blob/764994e40843a9d734bf7df504d0f038fbff7be9/lib/libutils/ext/include/trace_levels.h#L26-L31

#[repr(u8)]
pub enum TraceLevel {
    Error = 1,
    Info = 2,
    Debug = 3,
    /// optee refers to it as "flow"
    Trace = 4,
}

/// Extension trait to allow setting levels via [`TraceLevel`].
pub trait TraceExt {
    fn set_level(level: TraceLevel);
}

impl TraceExt for optee_utee::trace::Trace {
    fn set_level(level: TraceLevel) {
        Self::set_level(level as i32);
    }
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
        <optee_utee::trace::Trace as $crate::trace::TraceExt>::set_level($crate::trace::TraceLevel::Error);
        optee_utee::trace_println!($($arg)*)
    }};
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {{
        <optee_utee::trace::Trace as $crate::trace::TraceExt>::set_level($crate::trace::TraceLevel::Info);
        optee_utee::trace_println!($($arg)*)
    }};
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {{
        <optee_utee::trace::Trace as $crate::trace::TraceExt>::set_level($crate::trace::TraceLevel::Debug);
        optee_utee::trace_println!($($arg)*)
    }};
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {{
        <optee_utee::trace::Trace as $crate::trace::TraceExt>::set_level($crate::trace::TraceLevel::Trace);
        optee_utee::trace_println!($($arg)*)
    }};
}
