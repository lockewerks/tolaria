//! Game orchestration: setup, London mulligans, the turn loop, outcome.

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;
use smallvec::SmallVec;

use crate::agent::Agents;
use crate::carddb::{CardDb, CardRef};
use crate::state::{
    GameEnd, GameState, LossReason, ObjFlags, Player, RulesConfig, Seat, Step, Zone,
};
use crate::view::View;
use crate::zones;

#[derive(Debug, Clone)]
pub struct DeckList {
    pub cards: Vec<CardRef>,
    pub commander: Option<CardRef>,
}

#[derive(Debug, Clone)]
pub struct GameSetup {
    pub cfg: RulesConfig,
    /// None: the seed decides who goes first.
    pub first: Option<Seat>,
    pub trace: bool,
    /// Force seat 0's opening seven: these cards are placed on top of the
    /// library after the shuffle (hand-sweep enumeration). Mulligans still
    /// apply; this conditions on the dealt hand, not the kept one.
    pub forced_top: Option<Vec<CardRef>>,
}

#[derive(Debug, Clone)]
pub struct GameOutcome {
    pub end: GameEnd,
    pub turns: u32,
    pub decisions: u32,
    pub first: Seat,
    pub mulligans: SmallVec<[u8; 4]>,
    pub losses: SmallVec<[Option<LossReason>; 4]>,
    /// Final life per seat. Negative life on a damage kill is the overkill:
    /// how far past dead the loser was driven.
    pub life: SmallVec<[i32; 4]>,
    pub trace: Option<Vec<String>>,
}

pub fn new_game(
    db: std::sync::Arc<CardDb>,
    decks: &[DeckList],
    setup: &GameSetup,
    seed: u64,
) -> GameState {
    let mut rng = Pcg64Mcg::seed_from_u64(seed);
    let seats = decks.len() as Seat;
    let first = setup.first.unwrap_or_else(|| rng.gen_range(0..seats));

    let mut gs = GameState {
        db,
        players: (0..seats).map(|s| Player::new(s, setup.cfg.starting_life)).collect(),
        objects: Vec::with_capacity(decks.iter().map(|d| d.cards.len() + 1).sum()),
        stack: Vec::new(),
        active: first,
        phase: crate::state::Phase::Beginning,
        step: Step::Untap,
        turn: 0,
        ts: 0,
        floating: Vec::new(),
        pending_triggers: Vec::new(),
        combat: None,
        rng,
        cfg: setup.cfg,
        decisions: 0,
        over: None,
        trace: if setup.trace { Some(Vec::new()) } else { None },
    };

    for (seat, deck) in decks.iter().enumerate() {
        let seat = seat as Seat;
        for &card in &deck.cards {
            gs.new_object(card, seat, Zone::Library, None);
        }
        if let Some(cmdr) = deck.commander {
            let id = gs.new_object(cmdr, seat, Zone::Command, None);
            gs.obj_mut(id).flags |= ObjFlags::IS_COMMANDER;
        }
        zones::shuffle_library(&mut gs, seat);
    }

    // Stack seat 0's opening hand on top (library top is the vec's end).
    // The last `placed` slots hold already-positioned cards; each wanted
    // card is pulled from the unplaced region and pushed on top.
    if let Some(forced) = &setup.forced_top {
        let mut placed = 0usize;
        for &want in forced {
            let lib = &gs.players[0].library;
            let searchable = lib.len() - placed;
            if let Some(pos) = lib[..searchable]
                .iter()
                .position(|&id| gs.objects[id.0 as usize].card == want)
            {
                let id = gs.players[0].library.remove(pos);
                gs.players[0].library.push(id);
                placed += 1;
            }
        }
    }
    gs
}

fn mulligan_phase(gs: &mut GameState, agents: &mut Agents) {
    let seats: Vec<Seat> = gs.apnap().collect();
    for seat in seats {
        let mut taken = 0u8;
        loop {
            zones::draw_cards(gs, seat, 7);
            let hand = gs.player(seat).hand.clone();
            let wants_mull = taken < 6 && {
                let view = View { gs, seat };
                agents.get(seat).mulligan(&view, &hand, taken)
            };
            if !wants_mull {
                break;
            }
            // Return the hand and reshuffle.
            for id in hand {
                let ts = gs.next_ts();
                let o = gs.obj_mut(id);
                o.zone = Zone::Library;
                o.incarnation += 1;
                o.ts = ts;
                gs.player_mut(seat).hand.retain(|&x| x != id);
                gs.player_mut(seat).library.push(id);
            }
            zones::shuffle_library(gs, seat);
            taken += 1;
        }
        // London: bottom `taken` cards from the kept 7.
        if taken > 0 {
            let hand = gs.player(seat).hand.clone();
            let picked = {
                let view = View { gs, seat };
                agents.get(seat).choose_bottom(&view, &hand, taken as usize)
            };
            let mut bottomed = 0usize;
            for id in picked {
                if bottomed >= taken as usize {
                    break;
                }
                if gs.obj(id).zone == Zone::Hand {
                    let ts = gs.next_ts();
                    let o = gs.obj_mut(id);
                    o.zone = Zone::Library;
                    o.incarnation += 1;
                    o.ts = ts;
                    gs.player_mut(seat).hand.retain(|&x| x != id);
                    gs.player_mut(seat).library.insert(0, id);
                    bottomed += 1;
                }
            }
            while bottomed < taken as usize && !gs.player(seat).hand.is_empty() {
                let id = gs.player(seat).hand[0];
                let ts = gs.next_ts();
                let o = gs.obj_mut(id);
                o.zone = Zone::Library;
                o.incarnation += 1;
                o.ts = ts;
                gs.player_mut(seat).hand.remove(0);
                gs.player_mut(seat).library.insert(0, id);
                bottomed += 1;
            }
        }
        gs.player_mut(seat).mulligans = taken;
        if taken > 0 {
            gs.tracef(move || format!("seat {seat} mulligans to {}", 7 - taken));
        }
    }
}

/// Play one game to completion.
pub fn run_game(
    db: std::sync::Arc<CardDb>,
    decks: &[DeckList],
    setup: &GameSetup,
    agents: &mut Agents,
    seed: u64,
) -> GameOutcome {
    let mut gs = new_game(db, decks, setup, seed);
    let first = gs.active;
    mulligan_phase(&mut gs, agents);

    while gs.over.is_none() {
        gs.turn += 1;
        if gs.turn > gs.cfg.turn_cap {
            gs.tracef(|| "turn cap reached: game is a draw".to_string());
            gs.over = Some(GameEnd::Draw);
            break;
        }
        if gs.trace.is_some() {
            let life: Vec<String> = gs.players.iter().map(|p| p.life.to_string()).collect();
            let header =
                format!("--- turn {}, seat {} active (life {}) ---", gs.turn, gs.active, life.join("/"));
            gs.tracef(move || header);
        }
        crate::turn::take_turn(&mut gs, agents);
        if gs.over.is_some() {
            break;
        }
        // Advance to the next living seat.
        let n = gs.players.len() as Seat;
        let mut next = (gs.active + 1) % n;
        let mut guard = 0;
        while gs.player(next).lost.is_some() && guard < n {
            next = (next + 1) % n;
            guard += 1;
        }
        gs.active = next;
    }

    GameOutcome {
        end: gs.over.unwrap_or(GameEnd::Draw),
        turns: gs.turn,
        decisions: gs.decisions,
        first,
        mulligans: gs.players.iter().map(|p| p.mulligans).collect(),
        losses: gs.players.iter().map(|p| p.lost).collect(),
        life: gs.players.iter().map(|p| p.life).collect(),
        trace: gs.trace.take(),
    }
}
