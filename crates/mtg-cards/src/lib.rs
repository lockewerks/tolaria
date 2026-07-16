//! Card behavior compiler: oracle text to Effect IR, keyword short-circuit,
//! override registry, coverage grading, compiled cache.

pub mod compiler;

pub use compiler::{compile, COMPILER_VERSION};
