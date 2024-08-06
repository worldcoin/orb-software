#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::ignored_unit_patterns,
    clippy::items_after_statements
)]

pub mod consts;
pub mod interfaces;
pub mod proxies;
pub mod shutdown;
pub mod startup;
pub mod tasks;
pub mod telemetry;
