//! Game state: objects, zones, players, and the top-level GameState.

use rand_pcg::Pcg64Mcg;
use smallvec::SmallVec;

use mtg_ir::{
    CardTypes, ColorSet, CounterKind, Duration, KeywordSet, ManaCost, Supertypes, TokenProto,
};

use crate::carddb::{CardDb, CardRef};

pub const MAX_SEATS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectId(pub u32);

pub type Seat = u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Exile,
    Stack,
    Command,
    /// Ceased to exist (tokens, spent copies). Objects here are inert.
    Limbo,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ObjFlags: u16 {
        const ATTACKING        = 1 << 0;
        const BLOCKING         = 1 << 1;
        const BLOCKED          = 1 << 2;
        /// Took damage from a deathtouch source this turn.
        const DEATHTOUCHED     = 1 << 3;
        /// Regeneration shield for this turn.
        const REGEN_SHIELD     = 1 << 4;
        /// Attacked or activated a once-per-turn ability this turn.
        const ACTIVATED_TURN   = 1 << 5;
        /// A commander (cares about tax and command-zone returns).
        const IS_COMMANDER     = 1 << 6;
        /// Cast this turn (for evoke-style sacrifice bookkeeping).
        const ENTERED_THIS_TURN = 1 << 7;
    }
}

/// Computed characteristics after continuous effects. Recomputed eagerly
/// whenever a mutation could change them.
#[derive(Debug, Clone, Default)]
pub struct Characteristics {
    pub types: CardTypes,
    pub supertypes: Supertypes,
    pub power: i32,
    pub toughness: i32,
    pub keywords: KeywordSet,
    pub colors: ColorSet,
    pub ward: Option<ManaCost>,
    pub protection_from: ColorSet,
    pub toxic: u8,
}

#[derive(Debug, Clone)]
pub struct Object {
    pub id: ObjectId,
    /// Bumped on every zone change: the rules' "new object" identity.
    pub incarnation: u32,
    pub card: CardRef,
    /// Active face for permanents (transform state, chosen MDFC face).
    pub face: u8,
    /// Tokens carry their definition inline instead of a CardRef.
    pub token: Option<Box<TokenProto>>,
    pub owner: Seat,
    pub controller: Seat,
    pub zone: Zone,
    pub tapped: bool,
    /// Summoning sickness: entered under this controller since their last
    /// turn began.
    pub sick: bool,
    pub damage: i32,
    pub counters: SmallVec<[(CounterKind, i16); 2]>,
    pub attached_to: Option<ObjectId>,
    pub attachments: SmallVec<[ObjectId; 2]>,
    pub flags: ObjFlags,
    pub entered_turn: u32,
    /// Timestamp of the last battlefield entry, for layer ordering.
    pub ts: u64,
    /// Objects this one holds exiled (Oblivion Ring pattern).
    pub exiling: SmallVec<[ObjectId; 1]>,
    pub chars: Characteristics,
}

impl Object {
    pub fn counter_count(&self, kind: CounterKind) -> i16 {
        self.counters
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }

    pub fn add_counters(&mut self, kind: CounterKind, delta: i16) {
        if let Some(slot) = self.counters.iter_mut().find(|(k, _)| *k == kind) {
            slot.1 += delta;
            if slot.1 <= 0 {
                self.counters.retain(|(_, n)| *n > 0);
            }
        } else if delta > 0 {
            self.counters.push((kind, delta));
        }
    }

    pub fn is_creature(&self) -> bool {
        self.chars.types.contains(CardTypes::CREATURE)
    }

    pub fn is_land(&self) -> bool {
        self.chars.types.contains(CardTypes::LAND)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManaPool {
    /// W U B R G C
    pub pips: [u16; 6],
}

impl ManaPool {
    pub fn total(&self) -> u32 {
        self.pips.iter().map(|&p| p as u32).sum()
    }

    pub fn clear(&mut self) {
        self.pips = [0; 6];
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossReason {
    Life,
    Poison,
    DeckOut,
    CommanderDamage,
    Conceded,
}

#[derive(Debug)]
pub struct Player {
    pub seat: Seat,
    pub life: i32,
    pub poison: u8,
    pub library: Vec<ObjectId>,
    pub hand: Vec<ObjectId>,
    pub battlefield: Vec<ObjectId>,
    pub graveyard: Vec<ObjectId>,
    pub exile: Vec<ObjectId>,
    pub command: Vec<ObjectId>,
    pub mana: ManaPool,
    pub lands_played: u8,
    pub land_limit: u8,
    pub drew_from_empty: bool,
    pub lost: Option<LossReason>,
    pub mulligans: u8,
    /// Commander damage taken, per commander object.
    pub cmdr_damage: SmallVec<[(ObjectId, i32); 2]>,
    /// Casts from the command zone, per commander object (for tax).
    pub cmdr_casts: SmallVec<[(ObjectId, u8); 2]>,
}

impl Player {
    pub fn new(seat: Seat, life: i32) -> Player {
        Player {
            seat,
            life,
            poison: 0,
            library: Vec::new(),
            hand: Vec::new(),
            battlefield: Vec::new(),
            graveyard: Vec::new(),
            exile: Vec::new(),
            command: Vec::new(),
            mana: ManaPool::default(),
            lands_played: 0,
            land_limit: 1,
            drew_from_empty: false,
            lost: None,
            mulligans: 0,
            cmdr_damage: SmallVec::new(),
            cmdr_casts: SmallVec::new(),
        }
    }

    pub fn zone_mut(&mut self, zone: Zone) -> &mut Vec<ObjectId> {
        match zone {
            Zone::Library => &mut self.library,
            Zone::Hand => &mut self.hand,
            Zone::Battlefield => &mut self.battlefield,
            Zone::Graveyard => &mut self.graveyard,
            Zone::Exile => &mut self.exile,
            Zone::Command => &mut self.command,
            Zone::Stack | Zone::Limbo => unreachable!("stack and limbo are not player zones"),
        }
    }

    pub fn zone(&self, zone: Zone) -> &Vec<ObjectId> {
        match zone {
            Zone::Library => &self.library,
            Zone::Hand => &self.hand,
            Zone::Battlefield => &self.battlefield,
            Zone::Graveyard => &self.graveyard,
            Zone::Exile => &self.exile,
            Zone::Command => &self.command,
            Zone::Stack | Zone::Limbo => unreachable!("stack and limbo are not player zones"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Beginning,
    Main1,
    Combat,
    Main2,
    Ending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Untap,
    Upkeep,
    Draw,
    Main1,
    BeginCombat,
    DeclareAttackers,
    DeclareBlockers,
    FirstStrikeDamage,
    CombatDamage,
    EndCombat,
    Main2,
    End,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Obj(ObjectId, u32),
    Player(Seat),
}

/// What a stack item resolves into: a descriptor into the compiled card, so
/// the item itself stays small and Copy-ish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackKind {
    /// A spell cast from a face.
    Spell { face: u8 },
    /// An activated ability by index into the compiled face's list.
    Activated { face: u8, index: u8 },
    /// A triggered ability by index into the compiled face's list.
    Triggered { face: u8, index: u8 },
}

#[derive(Debug, Clone)]
pub struct StackItem {
    pub source: ObjectId,
    pub source_incarnation: u32,
    pub card: CardRef,
    pub controller: Seat,
    pub kind: StackKind,
    pub targets: SmallVec<[Target; 2]>,
    pub x: u32,
    /// Chosen mode indices for modal spells.
    pub modes: SmallVec<[u8; 2]>,
    /// Subject of the trigger (the creature that died, entered, ...).
    pub trigger_subject: Option<ObjectId>,
    /// Player relevant to the trigger (whose upkeep, who gained life, ...).
    pub trigger_player: Option<Seat>,
    /// Flashback and escape exile the card instead of yarding it.
    pub exile_on_resolve: bool,
}

/// A temporary continuous effect created by a resolved spell or ability
/// ("until end of turn" pumps and grants).
#[derive(Debug, Clone)]
pub struct FloatingEffect {
    pub target: ObjectId,
    pub target_incarnation: u32,
    pub until: Duration,
    pub p: i32,
    pub t: i32,
    pub set_pt: Option<(i32, i32)>,
    pub add_kw: KeywordSet,
    pub remove_kw: KeywordSet,
    pub control_to: Option<Seat>,
    pub ts: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct RulesConfig {
    pub seats: u8,
    pub starting_life: i32,
    pub turn_cap: u32,
    pub decision_cap: u32,
    /// Player going first skips their first draw step (1v1 rule).
    pub skip_first_draw: bool,
    pub commander: bool,
}

impl RulesConfig {
    pub fn duel() -> RulesConfig {
        RulesConfig {
            seats: 2,
            starting_life: 20,
            turn_cap: 60,
            decision_cap: 4_000,
            skip_first_draw: true,
            commander: false,
        }
    }

    pub fn commander_pod(seats: u8) -> RulesConfig {
        RulesConfig {
            seats,
            starting_life: 40,
            turn_cap: 100,
            decision_cap: 12_000,
            skip_first_draw: seats <= 2,
            commander: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEnd {
    Winner(Seat),
    Draw,
}

pub struct GameState {
    pub db: std::sync::Arc<CardDb>,
    pub players: SmallVec<[Player; 2]>,
    pub objects: Vec<Object>,
    pub stack: Vec<StackItem>,
    pub active: Seat,
    pub phase: Phase,
    pub step: Step,
    pub turn: u32,
    /// Monotonic timestamp for layers and trigger ordering.
    pub ts: u64,
    pub floating: Vec<FloatingEffect>,
    pub pending_triggers: Vec<crate::events::PendingTrigger>,
    pub combat: Option<crate::combat::CombatState>,
    pub rng: Pcg64Mcg,
    pub cfg: RulesConfig,
    pub decisions: u32,
    pub over: Option<GameEnd>,
    /// Optional human-readable event trace for debugging.
    pub trace: Option<Vec<String>>,
}

impl GameState {
    pub fn obj(&self, id: ObjectId) -> &Object {
        &self.objects[id.0 as usize]
    }

    pub fn obj_mut(&mut self, id: ObjectId) -> &mut Object {
        &mut self.objects[id.0 as usize]
    }

    pub fn player(&self, seat: Seat) -> &Player {
        &self.players[seat as usize]
    }

    pub fn player_mut(&mut self, seat: Seat) -> &mut Player {
        &mut self.players[seat as usize]
    }

    pub fn seats(&self) -> impl Iterator<Item = Seat> {
        0..self.players.len() as Seat
    }

    /// Turn order starting at the active player, skipping eliminated seats.
    pub fn apnap(&self) -> impl Iterator<Item = Seat> + '_ {
        let n = self.players.len() as Seat;
        (0..n)
            .map(move |i| (self.active + i) % n)
            .filter(move |s| self.players[*s as usize].lost.is_none())
    }

    pub fn opponents_of(&self, seat: Seat) -> impl Iterator<Item = Seat> + '_ {
        self.seats()
            .filter(move |s| *s != seat && self.players[*s as usize].lost.is_none())
    }

    pub fn next_ts(&mut self) -> u64 {
        self.ts += 1;
        self.ts
    }

    pub fn alive_count(&self) -> usize {
        self.players.iter().filter(|p| p.lost.is_none()).count()
    }

    pub fn tracef(&mut self, f: impl FnOnce() -> String) {
        if let Some(t) = &mut self.trace {
            t.push(f());
        }
    }

    /// Display name of an object for traces.
    pub fn name_of(&self, id: ObjectId) -> String {
        let o = self.obj(id);
        match &o.token {
            Some(t) => format!("{} (token)", t.name),
            None => self.db.face(o.card, o.face).name.to_string(),
        }
    }

    pub fn new_object(
        &mut self,
        card: CardRef,
        owner: Seat,
        zone: Zone,
        token: Option<Box<TokenProto>>,
    ) -> ObjectId {
        let id = ObjectId(self.objects.len() as u32);
        let ts = self.next_ts();
        self.objects.push(Object {
            id,
            incarnation: 0,
            card,
            face: 0,
            token,
            owner,
            controller: owner,
            zone,
            tapped: false,
            sick: true,
            damage: 0,
            counters: SmallVec::new(),
            attached_to: None,
            attachments: SmallVec::new(),
            flags: ObjFlags::empty(),
            entered_turn: 0,
            ts,
            exiling: SmallVec::new(),
            chars: Characteristics::default(),
        });
        if zone != Zone::Stack && zone != Zone::Limbo {
            self.players[owner as usize].zone_mut(zone).push(id);
        }
        id
    }
}
