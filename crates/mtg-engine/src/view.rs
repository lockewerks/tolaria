//! The agent's window into the game. Agents receive a View rather than the
//! GameState directly; the accessors below define the information a real
//! player at this seat would have. The raw state is reachable for
//! engine-internal plumbing, but agents are written against the accessors.

use crate::carddb::{CardDb, GameCard};
use crate::state::{GameState, ObjectId, Phase, Seat, StackItem, Step, Zone};

pub struct View<'a> {
    pub gs: &'a GameState,
    pub seat: Seat,
}

impl<'a> View<'a> {
    pub fn db(&self) -> &CardDb {
        &self.gs.db
    }

    pub fn card_of(&self, id: ObjectId) -> &GameCard {
        self.gs.db.get(self.gs.obj(id).card)
    }

    pub fn obj(&self, id: ObjectId) -> &crate::state::Object {
        self.gs.obj(id)
    }

    pub fn my_hand(&self) -> &[ObjectId] {
        &self.gs.player(self.seat).hand
    }

    pub fn hand_size(&self, seat: Seat) -> usize {
        self.gs.player(seat).hand.len()
    }

    pub fn library_size(&self, seat: Seat) -> usize {
        self.gs.player(seat).library.len()
    }

    pub fn battlefield(&self, seat: Seat) -> &[ObjectId] {
        &self.gs.player(seat).battlefield
    }

    pub fn graveyard(&self, seat: Seat) -> &[ObjectId] {
        &self.gs.player(seat).graveyard
    }

    pub fn life(&self, seat: Seat) -> i32 {
        self.gs.player(seat).life
    }

    pub fn poison(&self, seat: Seat) -> u8 {
        self.gs.player(seat).poison
    }

    pub fn stack(&self) -> &[StackItem] {
        &self.gs.stack
    }

    pub fn phase(&self) -> Phase {
        self.gs.phase
    }

    pub fn step(&self) -> Step {
        self.gs.step
    }

    pub fn turn(&self) -> u32 {
        self.gs.turn
    }

    pub fn active_seat(&self) -> Seat {
        self.gs.active
    }

    pub fn seats(&self) -> u8 {
        self.gs.players.len() as u8
    }

    pub fn opponents(&self) -> Vec<Seat> {
        self.gs.opponents_of(self.seat).collect()
    }

    pub fn lands_played(&self) -> u8 {
        self.gs.player(self.seat).lands_played
    }

    pub fn in_zone(&self, id: ObjectId, zone: Zone) -> bool {
        self.gs.obj(id).zone == zone
    }

    /// Untapped mana sources this seat controls right now.
    pub fn open_mana(&self) -> u32 {
        let p = self.gs.player(self.seat);
        let mut n = p.mana.total();
        for &id in &p.battlefield {
            let o = self.gs.obj(id);
            if o.tapped || o.token.is_some() {
                continue;
            }
            if o.sick && o.is_creature() {
                continue;
            }
            let cf = self.gs.db.compiled_face(o.card, o.face);
            if cf.mana_abilities.iter().any(|m| m.cost.tap_self && m.cost.mana.is_none()) {
                n += 1;
            }
        }
        n
    }
}
