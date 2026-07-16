//! Card behavior compiler: oracle text to Effect IR, keyword short-circuit,
//! override registry, coverage grading, compiled cache.

pub mod compiler;
pub mod templates;
pub mod text;

pub use compiler::{compile, compile_pool, CoverageStats, COMPILER_VERSION};
