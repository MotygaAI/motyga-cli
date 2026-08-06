//! Read-path helpers for Motyga memories.
//!
//! This crate owns memory injection, memory citation parsing, and telemetry
//! classification for read access to the memory folder. It intentionally does
//! not depend on the memory write pipeline.

pub mod citations;
mod metrics;
pub mod usage;

use motyga_utils_absolute_path::AbsolutePathBuf;

pub fn memory_root(motyga_home: &AbsolutePathBuf) -> AbsolutePathBuf {
    motyga_home.join("memories")
}
