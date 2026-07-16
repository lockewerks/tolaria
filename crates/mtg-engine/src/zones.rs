//! Zone movement: the single choke point where objects change zones,
//! pick up new incarnations, trigger events, and hit enter-replacements.

use mtg_ir::{CardTypes, ReplKind, ReplScope, ValueExpr};

use crate::events::GameEvent;
use crate::state::{GameState, ObjFlags, ObjectId, Seat, Zone};
use crate::triggers::process_event;

/// Remove an object from whatever container currently holds it.
fn detach_from_container(gs: &mut GameState, id: ObjectId) {
    let (zone, owner, controller) = {
        let o = gs.obj(id);
        (o.zone, o.owner, o.controller)
    };
    match zone {
        Zone::Battlefield => {
            gs.player_mut(controller).battlefield.retain(|&x| x != id);
        }
        Zone::Stack => {
            // Stack items referencing this object are handled by callers.
        }
        Zone::Limbo => {}
        z => {
            gs.player_mut(owner).zone_mut(z).retain(|&x| x != id);
        }
    }
}

/// How many counters an enter-replacement grants, evaluated statically.
fn fixed_value(n: &ValueExpr) -> i16 {
    match n {
        ValueExpr::Fixed(v) => *v as i16,
        _ => 1,
    }
}

/// Move an object to a new zone. `controller` only matters for battlefield
/// entries. Fires Leaves/Dies/Enters events and applies enter-replacements.
pub fn move_to(gs: &mut GameState, id: ObjectId, to: Zone, controller: Option<Seat>) {
    let (from, owner, was_creature, was_token, is_commander) = {
        let o = gs.obj(id);
        (
            o.zone,
            o.owner,
            o.is_creature(),
            o.token.is_some(),
            o.flags.contains(ObjFlags::IS_COMMANDER),
        )
    };
    if from == to && to != Zone::Battlefield {
        return;
    }

    // Tokens cease to exist anywhere but the battlefield.
    let mut to = if was_token && to != Zone::Battlefield { Zone::Limbo } else { to };
    // Commanders headed to the graveyard or exile go home instead. The
    // rules make this a choice; returning is almost always right.
    if is_commander && matches!(to, Zone::Graveyard | Zone::Exile) {
        to = Zone::Command;
    }

    detach_from_container(gs, id);

    // Attachments this object held: auras will die to SBA, equipment stays.
    let attachments: Vec<ObjectId> = gs.obj(id).attachments.iter().copied().collect();
    for a in attachments {
        gs.obj_mut(a).attached_to = None;
    }
    if let Some(host) = gs.obj(id).attached_to {
        gs.obj_mut(host).attachments.retain(|x| *x != id);
        gs.obj_mut(id).attached_to = None;
    }

    // Oblivion Ring pattern: release anything this object was exiling.
    if from == Zone::Battlefield {
        let jailed: Vec<ObjectId> = gs.obj_mut(id).exiling.drain(..).collect();
        for j in jailed {
            if gs.obj(j).zone == Zone::Exile {
                move_to(gs, j, Zone::Battlefield, Some(gs.obj(j).owner));
            }
        }
    }

    let ts = gs.next_ts();
    let turn = gs.turn;
    {
        let o = gs.obj_mut(id);
        o.incarnation += 1;
        o.zone = to;
        o.damage = 0;
        o.counters.clear();
        o.tapped = false;
        o.sick = true;
        o.flags &= ObjFlags::IS_COMMANDER;
        o.face = 0;
        o.entered_turn = turn;
        o.ts = ts;
    }

    match to {
        Zone::Battlefield => {
            let ctrl = controller.unwrap_or(owner);
            gs.obj_mut(id).controller = ctrl;
            gs.player_mut(ctrl).battlefield.push(id);
            apply_enter_replacements(gs, id);
        }
        Zone::Stack | Zone::Limbo => {}
        z => {
            gs.obj_mut(id).controller = owner;
            gs.player_mut(owner).zone_mut(z).push(id);
        }
    }

    if from == Zone::Battlefield {
        process_event(gs, GameEvent::Leaves(id));
        if to == Zone::Graveyard && was_creature {
            process_event(gs, GameEvent::Dies(id));
        }
    }
    if to == Zone::Battlefield {
        process_event(gs, GameEvent::Enters(id));
    }
    crate::layers::recompute_chars(gs);
}

/// Self and field-wide enter-the-battlefield replacements.
fn apply_enter_replacements(gs: &mut GameState, id: ObjectId) {
    let entering_controller = gs.obj(id).controller;

    // Own replacements.
    let own: Vec<ReplKind> = {
        let o = gs.obj(id);
        if o.token.is_some() {
            Vec::new()
        } else {
            gs.db
                .compiled_face(o.card, o.face)
                .replacements
                .iter()
                .filter(|r| matches!(r.scope, ReplScope::This))
                .map(|r| r.kind.clone())
                .collect()
        }
    };
    for kind in own {
        apply_one_enter_replacement(gs, id, &kind);
    }

    // Field-wide replacements from other battlefield permanents.
    let battlefield: Vec<ObjectId> = gs
        .players
        .iter()
        .flat_map(|p| p.battlefield.iter().copied())
        .filter(|&x| x != id)
        .collect();
    for src in battlefield {
        let repls: Vec<(ReplScope, ReplKind)> = {
            let o = gs.obj(src);
            if o.token.is_some() {
                continue;
            }
            gs.db
                .compiled_face(o.card, o.face)
                .replacements
                .iter()
                .filter(|r| !matches!(r.scope, ReplScope::This))
                .map(|r| (r.scope.clone(), r.kind.clone()))
                .collect()
        };
        let src_controller = gs.obj(src).controller;
        for (scope, kind) in repls {
            let applies = match &scope {
                ReplScope::This => false,
                ReplScope::Yours(f) => {
                    entering_controller == src_controller
                        && crate::filters::obj_matches(gs, f, id, src_controller, Some(src))
                }
                ReplScope::All(f) => {
                    crate::filters::obj_matches(gs, f, id, src_controller, Some(src))
                }
            };
            if applies {
                apply_one_enter_replacement(gs, id, &kind);
            }
        }
    }
}

fn apply_one_enter_replacement(gs: &mut GameState, id: ObjectId, kind: &ReplKind) {
    match kind {
        ReplKind::EntersTapped => gs.obj_mut(id).tapped = true,
        ReplKind::EntersWithCounters { kind, n } => {
            let n = fixed_value(n);
            gs.obj_mut(id).add_counters(*kind, n);
        }
        _ => {}
    }
}

/// Draw n cards; drawing from an empty library flags the loss SBA.
pub fn draw_cards(gs: &mut GameState, seat: Seat, n: u32) {
    for _ in 0..n {
        match gs.player_mut(seat).library.pop() {
            Some(id) => {
                let ts = gs.next_ts();
                {
                    let o = gs.obj_mut(id);
                    o.zone = Zone::Hand;
                    o.incarnation += 1;
                    o.ts = ts;
                }
                gs.player_mut(seat).hand.push(id);
                process_event(gs, GameEvent::DrewCard(seat));
            }
            None => {
                gs.player_mut(seat).drew_from_empty = true;
            }
        }
    }
}

/// True Fisher-Yates (Durstenfeld) shuffle via rand's SliceRandom, driven by
/// the game's seeded PCG stream: unbiased, O(n), deterministic per seed.
pub fn shuffle_library(gs: &mut GameState, seat: Seat) {
    use rand::seq::SliceRandom;
    let mut lib = std::mem::take(&mut gs.player_mut(seat).library);
    lib.shuffle(&mut gs.rng);
    gs.player_mut(seat).library = lib;
}

/// Change control of a battlefield permanent, moving it between
/// battlefield containers.
pub fn change_control(gs: &mut GameState, id: ObjectId, to: Seat) {
    let (zone, cur) = {
        let o = gs.obj(id);
        (o.zone, o.controller)
    };
    if zone != Zone::Battlefield || cur == to {
        return;
    }
    gs.player_mut(cur).battlefield.retain(|&x| x != id);
    gs.player_mut(to).battlefield.push(id);
    let ts = gs.next_ts();
    let turn = gs.turn;
    {
        let o = gs.obj_mut(id);
        o.controller = to;
        o.sick = true;
        o.entered_turn = turn;
        o.ts = ts;
    }
    crate::layers::recompute_chars(gs);
}

/// Create a token on the battlefield under a controller. Tokens use a
/// sentinel CardRef pointing at index 0; their identity lives in the proto.
pub fn create_token(
    gs: &mut GameState,
    proto: mtg_ir::TokenProto,
    controller: Seat,
    tapped: bool,
    attacking: bool,
) -> ObjectId {
    let card = crate::carddb::CardRef(0);
    let id = gs.new_object(card, controller, Zone::Limbo, Some(Box::new(proto)));
    detach_from_container(gs, id);
    let ts = gs.next_ts();
    let turn = gs.turn;
    {
        let o = gs.obj_mut(id);
        o.zone = Zone::Battlefield;
        o.tapped = tapped;
        o.entered_turn = turn;
        o.ts = ts;
        if attacking {
            o.flags |= ObjFlags::ATTACKING;
        }
    }
    gs.player_mut(controller).battlefield.push(id);
    process_event(gs, GameEvent::Enters(id));
    crate::layers::recompute_chars(gs);
    id
}

/// True when the face's types make it a permanent card.
pub fn is_permanent_types(types: CardTypes) -> bool {
    types.intersects(
        CardTypes::ARTIFACT
            | CardTypes::BATTLE
            | CardTypes::CREATURE
            | CardTypes::ENCHANTMENT
            | CardTypes::LAND
            | CardTypes::PLANESWALKER,
    )
}
