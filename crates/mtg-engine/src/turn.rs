//! The turn machine: phases, steps, priority windows, and cleanup.

use crate::actions::{apply_action, legal_actions, LegalAction};
use crate::agent::Agents;
use crate::events::GameEvent;
use crate::state::{GameEnd, GameState, ObjFlags, Phase, Step, Zone};
use crate::triggers::{flush_triggers, process_event};
use crate::view::View;
use crate::zones;

fn clear_pools(gs: &mut GameState) {
    for p in &mut gs.players {
        p.mana.clear();
    }
}

fn set_step(gs: &mut GameState, phase: Phase, step: Step) {
    clear_pools(gs);
    gs.phase = phase;
    gs.step = step;
}

/// One full priority round: SBAs and triggers, then players act in APNAP
/// order until everyone passes on an empty stack.
pub fn priority_round(gs: &mut GameState, agents: &mut Agents) {
    loop {
        crate::sba::run_sba(gs);
        if gs.over.is_some() {
            return;
        }
        flush_triggers(gs, agents);
        crate::sba::run_sba(gs);
        if gs.over.is_some() {
            return;
        }

        let mut acted = false;
        let seats: Vec<_> = gs.apnap().collect();
        for seat in seats {
            let legal = legal_actions(gs, seat);
            if legal.len() <= 1 {
                continue;
            }
            gs.decisions += 1;
            if gs.decisions > gs.cfg.decision_cap {
                gs.over = Some(GameEnd::Draw);
                return;
            }
            let choice = {
                let view = View { gs, seat };
                agents.get(seat).choose_action(&view, &legal)
            };
            let choice = choice.min(legal.len() - 1);
            if !matches!(legal[choice], LegalAction::Pass)
                && apply_action(gs, agents, seat, &legal[choice])
            {
                acted = true;
                break;
            }
        }
        if gs.over.is_some() {
            return;
        }
        if !acted {
            match gs.stack.pop() {
                Some(item) => {
                    crate::resolve::resolve(gs, agents, item);
                    crate::layers::recompute_chars(gs);
                }
                None => break,
            }
        }
    }
}

pub fn take_turn(gs: &mut GameState, agents: &mut Agents) {
    let active = gs.active;

    // Beginning: untap.
    set_step(gs, Phase::Beginning, Step::Untap);
    gs.player_mut(active).lands_played = 0;
    gs.player_mut(active).land_limit = 1;
    let all: Vec<_> = gs
        .players
        .iter()
        .flat_map(|p| p.battlefield.iter().copied())
        .collect();
    for id in all {
        let controller = gs.obj(id).controller;
        let o = gs.obj_mut(id);
        o.flags.remove(ObjFlags::ACTIVATED_TURN);
        if controller == active {
            o.tapped = false;
            o.sick = false;
        }
    }
    crate::layers::recompute_chars(gs);

    // Upkeep.
    set_step(gs, Phase::Beginning, Step::Upkeep);
    process_event(gs, GameEvent::UpkeepBegins(active));
    priority_round(gs, agents);
    if gs.over.is_some() {
        return;
    }

    // Draw.
    set_step(gs, Phase::Beginning, Step::Draw);
    if !(gs.turn == 1 && gs.cfg.skip_first_draw) {
        zones::draw_cards(gs, active, 1);
    }
    priority_round(gs, agents);
    if gs.over.is_some() {
        return;
    }

    // First main.
    set_step(gs, Phase::Main1, Step::Main1);
    priority_round(gs, agents);
    if gs.over.is_some() {
        return;
    }

    // Combat.
    set_step(gs, Phase::Combat, Step::BeginCombat);
    process_event(gs, GameEvent::CombatBegins(active));
    priority_round(gs, agents);
    if gs.over.is_some() {
        return;
    }
    set_step(gs, Phase::Combat, Step::DeclareAttackers);
    let any_attacks = crate::combat::declare_attackers(gs, agents);
    if any_attacks {
        priority_round(gs, agents);
        if gs.over.is_some() {
            return;
        }
        set_step(gs, Phase::Combat, Step::DeclareBlockers);
        crate::combat::declare_blockers(gs, agents);
        priority_round(gs, agents);
        if gs.over.is_some() {
            return;
        }
        if crate::combat::has_first_strike_step(gs) {
            set_step(gs, Phase::Combat, Step::FirstStrikeDamage);
            crate::combat::combat_damage(gs, true);
            priority_round(gs, agents);
            if gs.over.is_some() {
                return;
            }
        }
        set_step(gs, Phase::Combat, Step::CombatDamage);
        crate::combat::combat_damage(gs, false);
        priority_round(gs, agents);
        if gs.over.is_some() {
            return;
        }
    }
    set_step(gs, Phase::Combat, Step::EndCombat);
    crate::combat::end_combat(gs);

    // Second main.
    set_step(gs, Phase::Main2, Step::Main2);
    priority_round(gs, agents);
    if gs.over.is_some() {
        return;
    }

    // End step.
    set_step(gs, Phase::Ending, Step::End);
    process_event(gs, GameEvent::EndStepBegins(active));
    priority_round(gs, agents);
    if gs.over.is_some() {
        return;
    }

    // Cleanup.
    set_step(gs, Phase::Ending, Step::Cleanup);
    let hand_size = gs.player(active).hand.len();
    if hand_size > 7 {
        let n = hand_size - 7;
        let hand = gs.player(active).hand.clone();
        let picked = {
            let view = View { gs, seat: active };
            agents.get(active).choose_discard(&view, &hand, n)
        };
        let mut discarded = 0;
        for id in picked {
            if discarded >= n {
                break;
            }
            if gs.obj(id).zone == Zone::Hand {
                zones::move_to(gs, id, Zone::Graveyard, None);
                discarded += 1;
            }
        }
        // Agents that under-discard get the rest taken from the front.
        while gs.player(active).hand.len() > 7 {
            let id = gs.player(active).hand[0];
            zones::move_to(gs, id, Zone::Graveyard, None);
        }
    }
    // Damage wipe and end-of-turn expirations.
    let all: Vec<_> = gs
        .players
        .iter()
        .flat_map(|p| p.battlefield.iter().copied())
        .collect();
    for id in all {
        let o = gs.obj_mut(id);
        o.damage = 0;
        o.flags.remove(ObjFlags::DEATHTOUCHED | ObjFlags::REGEN_SHIELD);
    }
    // Temporary control effects revert before the floats are dropped.
    let reverts: Vec<(crate::state::ObjectId, crate::state::Seat)> = gs
        .floating
        .iter()
        .filter(|f| f.until == mtg_ir::Duration::EndOfTurn)
        .filter_map(|f| f.control_to.map(|s| (f.target, s)))
        .collect();
    for (id, seat) in reverts {
        if gs.obj(id).zone == Zone::Battlefield {
            zones::change_control(gs, id, seat);
        }
    }
    crate::layers::expire_end_of_turn(gs);
    crate::layers::recompute_chars(gs);
    crate::sba::run_sba(gs);
    clear_pools(gs);
}
