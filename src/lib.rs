#![doc = include_str!("../README.md")]
#[warn(missing_docs)]
/// Time functions
pub mod clock;
/// Memory functions
pub mod mman;
/// Scheduling functions
pub mod sched;
#[cfg(target_family = "unix")]
mod unix;
#[cfg(target_os = "windows")]
mod windows;
