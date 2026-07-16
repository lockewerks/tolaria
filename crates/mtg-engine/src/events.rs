//! Game events and the pending trigger queue.

use crate::state::{ObjectId, Seat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEvent {
    Enters(ObjectId),
    /// A creature went from battlefield to graveyard.
    Dies(ObjectId),
    /// Left the battlefield for anywhere.
    Leaves(ObjectId),
    Attacks(ObjectId),
    Blocks { blocker: ObjectId, attacker: ObjectId },
    UpkeepBegins(Seat),
    EndStepBegins(Seat),
    CombatBegins(Seat),
    LandPlayed { seat: Seat, land: ObjectId },
    SpellCast { spell: ObjectId, caster: Seat },
    CombatDamageToPlayer { source: ObjectId, player: Seat },
    LifeGained { seat: Seat, amount: i32 },
    DrewCard(Seat),
}

/// A trigger that has matched an event and awaits being put on the stack.
#[derive(Debug, Clone, Copy)]
pub struct PendingTrigger {
    /// The object whose ability triggered.
    pub source: ObjectId,
    pub source_incarnation: u32,
    pub controller: Seat,
    pub face: u8,
    /// Index into the compiled face's triggered list.
    pub index: u8,
    /// The event subject, if the trigger cares (the thing that died, ...).
    pub subject: Option<ObjectId>,
    /// The event player, if the trigger cares (whose upkeep, ...).
    pub player: Option<Seat>,
}
