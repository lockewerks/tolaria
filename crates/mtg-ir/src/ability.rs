//! Ability shapes: activated, triggered, static, mana abilities, and
//! replacement effects.

use serde::{Deserialize, Serialize};

use crate::compiled::OverrideId;
use crate::effect::{Effect, ObjFilter, SpellFilter, TargetSpec, ValueExpr, Whose};
use crate::mana::{ManaCost, ManaProduction};
use crate::types::{CounterKind, KeywordSet};

/// A castable or resolvable unit: target requirements plus the effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellAbility {
    pub targets: Vec<TargetSpec>,
    pub effect: Effect,
}

impl SpellAbility {
    pub fn untargeted(effect: Effect) -> SpellAbility {
        SpellAbility { targets: Vec::new(), effect }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AbilityCost {
    pub mana: Option<ManaCost>,
    pub tap_self: bool,
    pub sac_self: bool,
    /// Sacrifice another permanent matching the filter.
    pub sac: Option<ObjFilter>,
    pub pay_life: u16,
    pub discard_cards: u8,
    pub remove_counters: Option<(CounterKind, u8)>,
}

impl AbilityCost {
    pub fn tap() -> AbilityCost {
        AbilityCost { tap_self: true, ..Default::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum AbilityZone {
    #[default]
    Battlefield,
    Hand,
    Graveyard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivatedAbility {
    pub cost: AbilityCost,
    pub ability: SpellAbility,
    pub sorcery_speed: bool,
    pub once_per_turn: bool,
    /// Loyalty delta for planeswalker abilities; implies sorcery speed and
    /// the one-per-turn planeswalker rule.
    pub loyalty: Option<i8>,
    pub zone: AbilityZone,
}

/// Mana abilities resolve immediately and never use the stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaAbility {
    pub cost: AbilityCost,
    pub produce: ManaProduction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrigSubject {
    This,
    Matching(ObjFilter),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerCondition {
    Etb(TrigSubject),
    Dies(TrigSubject),
    Ltb(TrigSubject),
    Attacks(TrigSubject),
    Blocks(TrigSubject),
    Upkeep(Whose),
    EndStep(Whose),
    BeginCombat(Whose),
    /// A land entering the battlefield under your control.
    Landfall,
    CastSpell { whose: Whose, filter: SpellFilter },
    DealsCombatDamageToPlayer(TrigSubject),
    GainLife(Whose),
    Draws(Whose),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggeredAbility {
    pub when: TriggerCondition,
    pub ability: SpellAbility,
    pub once_per_turn: bool,
}

/// Which objects a static ability applies to, relative to its source's
/// controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AffectSpec {
    pub filter: ObjFilter,
    pub include_self: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StaticAbility {
    /// Layer 7c: anthems and debuffs.
    PtBuff { affects: AffectSpec, p: i32, t: i32 },
    /// Layer 6: keyword grants.
    GrantKeywords { affects: AffectSpec, kw: KeywordSet },
    /// Auras and equipment buffing whatever they are attached to.
    AttachedBuff { p: i32, t: i32, kw: KeywordSet },
    /// Cost changes for spells you or others cast. Negative delta reduces.
    SpellCostDelta { whose: Whose, filter: SpellFilter, delta: i16 },
    Custom(OverrideId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplScope {
    /// The source object itself.
    This,
    /// Objects you control matching the filter.
    Yours(ObjFilter),
    /// All matching objects.
    All(ObjFilter),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplKind {
    EntersTapped,
    EntersWithCounters { kind: CounterKind, n: ValueExpr },
    DiesToExile,
    /// Prevent all damage that would be dealt to the scoped objects.
    PreventDamage,
    TokensDoubled,
    CountersDoubled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacement {
    pub scope: ReplScope,
    pub kind: ReplKind,
}
