//! Effects, filters, targets, values, and conditions: the resolution
//! vocabulary the engine interprets.

use serde::{Deserialize, Serialize};

use crate::compiled::OverrideId;
use crate::mana::{ColorSet, ManaCost, ManaProduction};
use crate::types::{CardTypes, CounterKind, KeywordSet, Supertypes};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Whose {
    #[default]
    Any,
    You,
    Opponents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cmp {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl Cmp {
    pub fn eval(self, a: i64, b: i64) -> bool {
        match self {
            Cmp::Lt => a < b,
            Cmp::Le => a <= b,
            Cmp::Eq => a == b,
            Cmp::Ge => a >= b,
            Cmp::Gt => a > b,
        }
    }
}

/// A conjunctive object filter. Empty flag sets and None fields mean
/// "no constraint". Evaluated by the engine against battlefield objects or
/// cards in other zones (state constraints are ignored off-battlefield).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ObjFilter {
    /// Object must have at least one of these types.
    pub types: CardTypes,
    pub not_types: CardTypes,
    pub supertypes: Supertypes,
    pub not_supertypes: Supertypes,
    /// Object must have at least one of these subtypes (lowercase).
    pub subtypes_any: Vec<Box<str>>,
    /// Object must be at least one of these colors.
    pub colors_any: ColorSet,
    pub not_colors: ColorSet,
    pub controller: Whose,
    pub with_keywords: KeywordSet,
    pub without_keywords: KeywordSet,
    pub tapped: Option<bool>,
    pub attacking: Option<bool>,
    pub blocking: Option<bool>,
    pub attacking_or_blocking: bool,
    pub is_token: Option<bool>,
    /// Exclude the source object itself ("another creature").
    pub other_than_self: bool,
    pub power: Option<(Cmp, i32)>,
    pub toughness: Option<(Cmp, i32)>,
    pub mana_value: Option<(Cmp, i32)>,
    /// Exact card name ("a card named X").
    pub name_is: Option<Box<str>>,
}

impl ObjFilter {
    pub fn of_types(types: CardTypes) -> ObjFilter {
        ObjFilter { types, ..Default::default() }
    }

    pub fn creature() -> ObjFilter {
        Self::of_types(CardTypes::CREATURE)
    }

    pub fn land() -> ObjFilter {
        Self::of_types(CardTypes::LAND)
    }

    pub fn controlled_by(mut self, whose: Whose) -> ObjFilter {
        self.controller = whose;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerFilter {
    Any,
    Opponent,
    You,
}

/// Filter over spells on the stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SpellFilter {
    pub types: CardTypes,
    pub not_types: CardTypes,
    pub mana_value: Option<(Cmp, i32)>,
    pub colors_any: ColorSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetCount {
    Exactly(u8),
    UpTo(u8),
}

impl TargetCount {
    pub fn max(self) -> u8 {
        match self {
            TargetCount::Exactly(n) | TargetCount::UpTo(n) => n,
        }
    }

    pub fn min(self) -> u8 {
        match self {
            TargetCount::Exactly(n) => n,
            TargetCount::UpTo(_) => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetWhat {
    /// A permanent on the battlefield.
    Permanent(ObjFilter),
    /// A card in a graveyard.
    CardInGraveyard(ObjFilter, Whose),
    Player(PlayerFilter),
    /// "Any target": creature, planeswalker, battle, or player.
    AnyDamageable,
    SpellOnStack(SpellFilter),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetSpec {
    pub count: TargetCount,
    pub what: TargetWhat,
}

impl TargetSpec {
    pub fn one(what: TargetWhat) -> TargetSpec {
        TargetSpec { count: TargetCount::Exactly(1), what }
    }
}

/// Selects objects at resolution time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjSel {
    /// Index into the chosen targets of the resolving ability.
    Target(u8),
    /// The source object of the ability.
    This,
    /// Every object matching the filter (relative to the controller).
    All(ObjFilter),
    /// The object that caused the trigger (the creature that died, entered,
    /// attacked, ...).
    TriggerSubject,
    /// The permanent this aura or equipment is attached to.
    AttachedHost,
}

/// Selects players at resolution time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerSel {
    You,
    EachOpponent,
    EachPlayer,
    Target(u8),
    /// The player relevant to the trigger (who gained life, whose upkeep,
    /// the defending player of an attack trigger, ...).
    TriggerPlayer,
    ControllerOf(Box<ObjSel>),
}

/// Where damage or an effect lands when it can hit objects or players.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recipient {
    /// A chosen target that may be an object or a player.
    Target(u8),
    Object(ObjSel),
    Player(PlayerSel),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueExpr {
    Fixed(i32),
    X,
    /// Number of objects matching the filter (controller-relative).
    Count(Box<ObjFilter>),
    CardsInHand(PlayerSel),
    LifeTotal(PlayerSel),
    CountersOnThis(CounterKind),
    Custom(OverrideId),
}

impl ValueExpr {
    pub const ONE: ValueExpr = ValueExpr::Fixed(1);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Condition {
    Compare(Box<ValueExpr>, Cmp, Box<ValueExpr>),
    YourTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Duration {
    EndOfTurn,
    WhileSourceOnBattlefield,
    Permanent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenProto {
    pub name: Box<str>,
    pub power: i32,
    pub toughness: i32,
    pub types: CardTypes,
    pub subtypes: Vec<Box<str>>,
    pub colors: ColorSet,
    pub keywords: KeywordSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SearchDest {
    Hand,
    Battlefield,
    Graveyard,
    TopOfLibrary,
}

/// The resolution effect AST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    Seq(Vec<Effect>),
    DealDamage { n: ValueExpr, to: Recipient },
    Draw { who: PlayerSel, n: ValueExpr },
    Discard { who: PlayerSel, n: ValueExpr, random: bool },
    Destroy { what: ObjSel },
    Exile { what: ObjSel },
    /// Oblivion Ring pattern: exiled until the source leaves the battlefield.
    ExileUntilSourceLeaves { what: ObjSel },
    /// Return to owner's hand.
    Bounce { what: ObjSel },
    PutOnTopOfLibrary { what: ObjSel },
    /// Graveyard to battlefield.
    Reanimate { what: ObjSel, controller: PlayerSel, tapped: bool },
    CreateToken { proto: TokenProto, n: ValueExpr, tapped: bool, attacking: bool },
    ModifyPt { what: ObjSel, p: ValueExpr, t: ValueExpr, dur: Duration },
    SetPt { what: ObjSel, p: i32, t: i32, dur: Duration },
    GrantKeywords { what: ObjSel, kw: KeywordSet, dur: Duration },
    RemoveKeywords { what: ObjSel, kw: KeywordSet, dur: Duration },
    PutCounters { what: ObjSel, kind: CounterKind, n: ValueExpr },
    RemoveCounters { what: ObjSel, kind: CounterKind, n: ValueExpr },
    CounterSpell { target: u8, unless_pay: Option<ManaCost> },
    AddMana { produce: ManaProduction },
    Mill { who: PlayerSel, n: ValueExpr },
    SearchLibrary {
        who: PlayerSel,
        filter: ObjFilter,
        dest: SearchDest,
        count: u8,
        enters_tapped: bool,
    },
    Shuffle { who: PlayerSel },
    GainLife { who: PlayerSel, n: ValueExpr },
    LoseLife { who: PlayerSel, n: ValueExpr },
    Fight { a: ObjSel, b: ObjSel },
    Sacrifice { who: PlayerSel, filter: ObjFilter, n: ValueExpr },
    TapObjects { what: ObjSel, tap: bool },
    Scry { who: PlayerSel, n: ValueExpr },
    Surveil { who: PlayerSel, n: ValueExpr },
    /// Modes carry their own target lists; target indices inside a mode's
    /// effect refer to that mode's list.
    Modal { choose: u8, modes: Vec<crate::ability::SpellAbility> },
    If { cond: Condition, then: Box<Effect>, otherwise: Option<Box<Effect>> },
    GainControl { what: ObjSel, dur: Duration },
    /// Flip a transforming double-faced source.
    Transform,
    /// Attach the source (aura or equipment) to a chosen target.
    Attach { target: u8 },
    Custom(OverrideId),
    Noop,
}
