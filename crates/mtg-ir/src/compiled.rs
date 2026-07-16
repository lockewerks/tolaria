//! The compiled card: what the behavior compiler emits and the engine
//! executes. One CompiledCard per oracle identity, one CompiledFace per
//! castable or playable face.

use serde::{Deserialize, Serialize};

use crate::ability::{
    ActivatedAbility, ManaAbility, Replacement, SpellAbility, StaticAbility, TriggeredAbility,
};
use crate::effect::ObjFilter;
use crate::mana::{ColorSet, ManaCost};
use crate::types::{CastMods, KeywordSet};

/// Key into the hand-written override registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OverrideId(pub u32);

/// Fidelity grade for a compiled card. Ordering is worst to best so that
/// a deck's grade is the minimum over its cards.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum CoverageTier {
    /// Never castable; occupies a deck slot only.
    Unplayable,
    /// Correct body, cost, and keywords; rules text treated as inert.
    Proxy,
    /// Main effect modeled; listed riders dropped.
    Partial,
    /// Faithfully modeled.
    Full,
}

/// Alternative ways to cast a face.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AltCost {
    Flashback(ManaCost),
    Escape { cost: ManaCost, exile_count: u8 },
    Foretell(ManaCost),
    /// Cast for the evoke cost, sacrifice on entry.
    Evoke(ManaCost),
}

/// Additional costs demanded on cast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddlCost {
    Sacrifice(ObjFilter),
    Discard(u8),
    PayLife(u16),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CompiledFace {
    /// None for lands and uncastable back faces.
    pub cost: Option<ManaCost>,
    pub x_spell: bool,
    pub cast_mods: CastMods,
    pub alt_costs: Vec<AltCost>,
    pub addl_costs: Vec<AddlCost>,
    pub cycling: Option<ManaCost>,
    pub crew: Option<u8>,
    pub kicker: Option<ManaCost>,
    /// Resolution effect for instants and sorceries.
    pub spell: Option<SpellAbility>,
    pub keywords: KeywordSet,
    pub ward: Option<ManaCost>,
    pub protection_from: ColorSet,
    pub toxic: u8,
    pub activated: Vec<ActivatedAbility>,
    pub mana_abilities: Vec<ManaAbility>,
    pub triggered: Vec<TriggeredAbility>,
    pub statics: Vec<StaticAbility>,
    pub replacements: Vec<Replacement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledCard {
    pub tier: CoverageTier,
    /// Rider text the compiler dropped, verbatim, for the coverage report.
    pub dropped: Vec<Box<str>>,
    pub faces: Vec<CompiledFace>,
    pub compiler_version: u16,
}

impl CompiledCard {
    pub fn proxy(faces: usize) -> CompiledCard {
        CompiledCard {
            tier: CoverageTier::Proxy,
            dropped: Vec::new(),
            faces: (0..faces.max(1)).map(|_| CompiledFace::default()).collect(),
            compiler_version: 0,
        }
    }
}
