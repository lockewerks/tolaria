//! State-based actions (rule 704 subset), death handling with undying and
//! persist, and player elimination.

use mtg_ir::{CardTypes, CounterKind, KeywordSet, Supertypes};

use crate::state::{GameEnd, GameState, LossReason, ObjFlags, ObjectId, Seat, Zone};
use crate::zones;

/// A creature dies (sacrifice, lethal damage, toughness 0). Undying and
/// persist return it with a counter instead.
pub fn die(gs: &mut GameState, id: ObjectId) {
    let o = gs.obj(id);
    if o.zone != Zone::Battlefield {
        return;
    }
    let kw = o.chars.keywords;
    let controller = o.controller;
    if kw.contains(KeywordSet::UNDYING) && o.counter_count(CounterKind::PlusOne) == 0 {
        zones::move_to(gs, id, Zone::Graveyard, None);
        if gs.obj(id).zone == Zone::Graveyard {
            zones::move_to(gs, id, Zone::Battlefield, Some(controller));
            gs.obj_mut(id).add_counters(CounterKind::PlusOne, 1);
            crate::layers::recompute_chars(gs);
        }
        return;
    }
    if kw.contains(KeywordSet::PERSIST) && o.counter_count(CounterKind::MinusOne) == 0 {
        zones::move_to(gs, id, Zone::Graveyard, None);
        if gs.obj(id).zone == Zone::Graveyard {
            zones::move_to(gs, id, Zone::Battlefield, Some(controller));
            gs.obj_mut(id).add_counters(CounterKind::MinusOne, 1);
            crate::layers::recompute_chars(gs);
        }
        return;
    }
    // Dies-to-exile replacement on the object itself.
    let to_exile = {
        let o = gs.obj(id);
        o.token.is_none()
            && gs
                .db
                .compiled_face(o.card, o.face)
                .replacements
                .iter()
                .any(|r| matches!(r.kind, mtg_ir::ReplKind::DiesToExile))
    };
    zones::move_to(gs, id, if to_exile { Zone::Exile } else { Zone::Graveyard }, None);
}

/// Destroy respects indestructible; regeneration shields are consumed.
pub fn destroy(gs: &mut GameState, id: ObjectId) -> bool {
    let o = gs.obj(id);
    if o.zone != Zone::Battlefield {
        return false;
    }
    if o.chars.keywords.contains(KeywordSet::INDESTRUCTIBLE) {
        return false;
    }
    if o.flags.contains(ObjFlags::REGEN_SHIELD) {
        let o = gs.obj_mut(id);
        o.flags.remove(ObjFlags::REGEN_SHIELD);
        o.tapped = true;
        o.damage = 0;
        return false;
    }
    die(gs, id);
    true
}

fn eliminate(gs: &mut GameState, seat: Seat, reason: LossReason) {
    if gs.player(seat).lost.is_some() {
        return;
    }
    gs.player_mut(seat).lost = Some(reason);
    gs.tracef(|| format!("seat {seat} loses: {reason:?}"));

    // Rule 800.4: objects owned by the leaving player leave the game, and
    // their spells and abilities on the stack cease to exist.
    gs.stack.retain(|item| item.controller != seat);
    let owned: Vec<ObjectId> = gs
        .objects
        .iter()
        .filter(|o| o.owner == seat && !matches!(o.zone, Zone::Limbo))
        .map(|o| o.id)
        .collect();
    for id in owned {
        // Direct removal: these objects are simply gone.
        crate::zones::move_to(gs, id, Zone::Limbo, None);
        let o = gs.obj_mut(id);
        o.zone = Zone::Limbo;
    }
    // Objects the eliminated player controlled but did not own go back to
    // their owners' control.
    let stolen: Vec<ObjectId> = gs
        .players
        .iter()
        .flat_map(|p| p.battlefield.iter().copied())
        .filter(|&id| gs.obj(id).controller == seat)
        .collect();
    for id in stolen {
        let owner = gs.obj(id).owner;
        crate::zones::change_control(gs, id, owner);
    }
    crate::layers::recompute_chars(gs);
}

/// Run state-based actions to quiescence. Returns true if anything fired.
pub fn run_sba(gs: &mut GameState) -> bool {
    let mut any = false;
    for _round in 0..32 {
        let mut fired = false;

        // Player checks.
        for seat in 0..gs.players.len() as Seat {
            let p = gs.player(seat);
            if p.lost.is_some() {
                continue;
            }
            let reason = if p.life <= 0 {
                Some(LossReason::Life)
            } else if p.poison >= 10 {
                Some(LossReason::Poison)
            } else if p.drew_from_empty {
                Some(LossReason::DeckOut)
            } else if p.cmdr_damage.iter().any(|(_, n)| *n >= 21) {
                Some(LossReason::CommanderDamage)
            } else {
                None
            };
            if let Some(r) = reason {
                eliminate(gs, seat, r);
                fired = true;
            }
        }

        // Game end?
        let alive: Vec<Seat> =
            gs.seats().filter(|&s| gs.player(s).lost.is_none()).collect();
        if alive.len() <= 1 && gs.over.is_none() {
            gs.over = Some(match alive.first() {
                Some(&s) => GameEnd::Winner(s),
                None => GameEnd::Draw,
            });
            return true;
        }
        if gs.over.is_some() {
            return any;
        }

        let battlefield: Vec<ObjectId> = gs
            .players
            .iter()
            .flat_map(|p| p.battlefield.iter().copied())
            .collect();

        for &id in &battlefield {
            let (zone, types, toughness, damage, deathtouched, loyalty, plus, minus) = {
                let o = gs.obj(id);
                (
                    o.zone,
                    o.chars.types,
                    o.chars.toughness,
                    o.damage,
                    o.flags.contains(ObjFlags::DEATHTOUCHED),
                    o.counter_count(CounterKind::Loyalty),
                    o.counter_count(CounterKind::PlusOne),
                    o.counter_count(CounterKind::MinusOne),
                )
            };
            if zone != Zone::Battlefield {
                continue;
            }
            if types.contains(CardTypes::CREATURE) {
                if toughness <= 0 {
                    die(gs, id);
                    fired = true;
                    continue;
                }
                let lethal = damage >= toughness || (damage > 0 && deathtouched);
                if lethal && destroy(gs, id) {
                    fired = true;
                    continue;
                }
            }
            if types.contains(CardTypes::PLANESWALKER) && loyalty <= 0 {
                die(gs, id);
                fired = true;
                continue;
            }
            // Auras must be attached to something legal.
            let (is_aura, attach_ok) = {
                let o = gs.obj(id);
                let is_aura = o.token.is_none()
                    && gs.db.face(o.card, o.face).subtypes.iter().any(|s| s.as_ref() == "aura");
                let ok = o
                    .attached_to
                    .map(|h| gs.obj(h).zone == Zone::Battlefield)
                    .unwrap_or(false);
                (is_aura, ok)
            };
            if is_aura && !attach_ok {
                die(gs, id);
                fired = true;
                continue;
            }
            // +1/+1 and -1/-1 counters annihilate.
            if plus > 0 && minus > 0 {
                let n = plus.min(minus);
                let o = gs.obj_mut(id);
                o.add_counters(CounterKind::PlusOne, -n);
                o.add_counters(CounterKind::MinusOne, -n);
                crate::layers::recompute_chars(gs);
                fired = true;
            }
        }

        // Legend rule, per controller and name: newest survives.
        for seat in 0..gs.players.len() as Seat {
            let mut seen: Vec<(String, ObjectId, u64)> = Vec::new();
            let ids: Vec<ObjectId> = gs.player(seat).battlefield.clone();
            for id in ids {
                let o = gs.obj(id);
                if !o.chars.supertypes.contains(Supertypes::LEGENDARY) {
                    continue;
                }
                let name = gs.name_of(id);
                if let Some(slot) = seen.iter_mut().find(|(n, _, _)| *n == name) {
                    let (_, old_id, old_ts) = *slot;
                    if o.ts > old_ts {
                        slot.1 = id;
                        slot.2 = o.ts;
                        die(gs, old_id);
                    } else {
                        die(gs, id);
                    }
                    fired = true;
                } else {
                    seen.push((name, id, o.ts));
                }
            }
        }

        if !fired {
            break;
        }
        any = true;
        crate::layers::recompute_chars(gs);
    }
    any
}
