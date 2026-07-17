//! The decision interface. Every choice the rules ask of a player routes
//! through this trait; defaults are deliberately dumb but legal so partial
//! implementations always produce a finished game.

use mtg_ir::{SpellAbility, TargetSpec};
use smallvec::SmallVec;

use crate::combat::Defender;
use crate::state::{ObjectId, Seat, Target};
use crate::view::View;

pub trait Agent: Send {
    /// True = take a mulligan (London: redraw 7, bottom `taken + 1` later).
    fn mulligan(&mut self, _v: &View, _hand: &[ObjectId], _taken: u8) -> bool {
        false
    }

    /// Which cards to put on the bottom after keeping a mulliganed hand.
    fn choose_bottom(&mut self, _v: &View, hand: &[ObjectId], n: usize) -> Vec<ObjectId> {
        hand.iter().copied().take(n).collect()
    }

    /// Pick from the legal actions; index 0 is always Pass.
    fn choose_action(&mut self, _v: &View, _legal: &[crate::actions::LegalAction]) -> usize {
        0
    }

    fn choose_targets(
        &mut self,
        _v: &View,
        spec: &TargetSpec,
        candidates: &[Target],
    ) -> SmallVec<[Target; 2]> {
        let want = spec.count.max() as usize;
        let need = spec.count.min() as usize;
        let take = candidates.len().min(want).max(need.min(candidates.len()));
        candidates.iter().copied().take(take).collect()
    }

    fn declare_attackers(
        &mut self,
        _v: &View,
        _candidates: &[ObjectId],
        _defenders: &[Defender],
    ) -> Vec<(ObjectId, Defender)> {
        Vec::new()
    }

    /// Pairs of (blocker, attacker).
    fn declare_blockers(
        &mut self,
        _v: &View,
        _attackers: &[ObjectId],
        _candidates: &[ObjectId],
    ) -> Vec<(ObjectId, ObjectId)> {
        Vec::new()
    }

    fn order_blockers(
        &mut self,
        _v: &View,
        _attacker: ObjectId,
        blockers: &[ObjectId],
    ) -> Vec<ObjectId> {
        blockers.to_vec()
    }

    fn choose_discard(&mut self, _v: &View, hand: &[ObjectId], n: usize) -> Vec<ObjectId> {
        hand.iter().copied().take(n).collect()
    }

    fn choose_mode(&mut self, _v: &View, _modes: &[SpellAbility], choose: u8) -> SmallVec<[u8; 2]> {
        (0..choose).collect()
    }

    /// Choose X for the spell being cast. `source` and `face` identify it
    /// so the agent can read its effect and cap X at something useful.
    fn choose_x(&mut self, _v: &View, _source: ObjectId, _face: u8, max: u32) -> u32 {
        max
    }

    /// Generic yes/no: pay optional costs, use optional abilities.
    fn yes_no(&mut self, _v: &View, _prompt: YesNo) -> bool {
        false
    }

    /// Scry: which of the looked-at cards go to the bottom.
    fn scry_bottom(&mut self, _v: &View, _looked: &[ObjectId]) -> Vec<ObjectId> {
        Vec::new()
    }

    fn choose_sacrifice(&mut self, _v: &View, candidates: &[ObjectId], n: usize) -> Vec<ObjectId> {
        candidates.iter().copied().take(n).collect()
    }

    /// Library search: pick up to `count` from candidates.
    fn search_pick(&mut self, _v: &View, candidates: &[ObjectId], count: usize) -> Vec<ObjectId> {
        candidates.iter().copied().take(count).collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum YesNo {
    PayWard,
    /// The controller of `spell` may pay `cost_mv` mana to save it from a
    /// counter. The values let the agent weigh the spell against the tax.
    CounterUnlessPay { spell: ObjectId, cost_mv: u32 },
    OptionalTrigger,
    ReturnCommanderToCommandZone,
}

/// The per-seat agent roster for one game.
pub struct Agents {
    pub seats: Vec<Box<dyn Agent>>,
}

impl Agents {
    pub fn get(&mut self, seat: Seat) -> &mut dyn Agent {
        self.seats[seat as usize].as_mut()
    }
}

/// Does nothing but pass and keep every hand. The rules-floor baseline.
pub struct PassAgent;

impl Agent for PassAgent {}

/// Plays the first legal action, attacks with everything, never blocks.
/// Exists so engine tests have a pulse to test against before mtg-ai.
pub struct NaiveAgent;

impl Agent for NaiveAgent {
    fn choose_action(&mut self, _v: &View, legal: &[crate::actions::LegalAction]) -> usize {
        if legal.len() > 1 {
            1
        } else {
            0
        }
    }

    fn declare_attackers(
        &mut self,
        _v: &View,
        candidates: &[ObjectId],
        defenders: &[Defender],
    ) -> Vec<(ObjectId, Defender)> {
        match defenders.first() {
            Some(d) => candidates.iter().map(|&c| (c, *d)).collect(),
            None => Vec::new(),
        }
    }
}
