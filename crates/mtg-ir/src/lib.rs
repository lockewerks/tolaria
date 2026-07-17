//! Effect IR: the shared vocabulary between the card compiler and the rules
//! engine. Pure data types plus small parsing helpers. Leaf crate.

pub mod ability;
pub mod compiled;
pub mod effect;
pub mod limits;
pub mod mana;
pub mod types;

pub use ability::*;
pub use compiled::*;
pub use effect::*;
pub use limits::{Limit, LimitCategory};
pub use mana::*;
pub use types::*;
