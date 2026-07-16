//! Trigger matching and the pending queue. Events are matched eagerly when
//! they happen; matched triggers wait in the queue until the next priority
//! window puts them on the stack in APNAP order.

use mtg_ir::{CardTypes, TrigSubject, TriggerCondition};

use crate::events::{GameEvent, PendingTrigger};
use crate::filters::{obj_matches, spell_matches, whose_matches};
use crate::state::{GameState, ObjectId, Seat, StackItem, StackKind, Target, Zone};

/// Scan for triggered abilities matching this event and queue them.
pub fn process_event(gs: &mut GameState, ev: GameEvent) {
    // Battlefield objects plus the event subject (a dying creature's own
    // "when this dies" trigger fires from the graveyard).
    let mut scan: Vec<ObjectId> = gs
        .players
        .iter()
        .flat_map(|p| p.battlefield.iter().copied())
        .collect();
    if let Some(subject) = event_subject(&ev) {
        if !scan.contains(&subject) {
            scan.push(subject);
        }
    }

    for oid in scan {
        let o = gs.obj(oid);
        if o.token.is_some() {
            continue;
        }
        let cf = gs.db.compiled_face(o.card, o.face);
        for (i, ta) in cf.triggered.iter().enumerate() {
            if condition_matches(gs, &ev, oid, &ta.when) {
                let o = gs.obj(oid);
                gs.pending_triggers.push(PendingTrigger {
                    source: oid,
                    source_incarnation: o.incarnation,
                    controller: o.controller,
                    face: o.face,
                    index: i as u8,
                    subject: event_subject(&ev),
                    player: event_player(&ev),
                });
            }
        }
    }
}

fn event_subject(ev: &GameEvent) -> Option<ObjectId> {
    match ev {
        GameEvent::Enters(s)
        | GameEvent::Dies(s)
        | GameEvent::Leaves(s)
        | GameEvent::Attacks(s) => Some(*s),
        GameEvent::Blocks { blocker, .. } => Some(*blocker),
        GameEvent::LandPlayed { land, .. } => Some(*land),
        GameEvent::SpellCast { spell, .. } => Some(*spell),
        GameEvent::CombatDamageToPlayer { source, .. } => Some(*source),
        _ => None,
    }
}

fn event_player(ev: &GameEvent) -> Option<Seat> {
    match ev {
        GameEvent::UpkeepBegins(p)
        | GameEvent::EndStepBegins(p)
        | GameEvent::CombatBegins(p)
        | GameEvent::DrewCard(p) => Some(*p),
        GameEvent::LandPlayed { seat, .. } => Some(*seat),
        GameEvent::SpellCast { caster, .. } => Some(*caster),
        GameEvent::CombatDamageToPlayer { player, .. } => Some(*player),
        GameEvent::LifeGained { seat, .. } => Some(*seat),
        _ => None,
    }
}

fn subject_matches(
    gs: &GameState,
    spec: &TrigSubject,
    listener: ObjectId,
    subject: ObjectId,
) -> bool {
    match spec {
        TrigSubject::This => listener == subject,
        TrigSubject::Matching(f) => {
            let controller = gs.obj(listener).controller;
            obj_matches(gs, f, subject, controller, Some(listener))
        }
    }
}

fn condition_matches(
    gs: &GameState,
    ev: &GameEvent,
    listener: ObjectId,
    when: &TriggerCondition,
) -> bool {
    let lc = gs.obj(listener).controller;
    match (when, ev) {
        (TriggerCondition::Etb(spec), GameEvent::Enters(s)) => {
            subject_matches(gs, spec, listener, *s)
        }
        (TriggerCondition::Dies(spec), GameEvent::Dies(s)) => {
            subject_matches(gs, spec, listener, *s)
        }
        (TriggerCondition::Ltb(spec), GameEvent::Leaves(s)) => {
            subject_matches(gs, spec, listener, *s)
        }
        (TriggerCondition::Attacks(spec), GameEvent::Attacks(s)) => {
            subject_matches(gs, spec, listener, *s)
        }
        (TriggerCondition::Blocks(spec), GameEvent::Blocks { blocker, .. }) => {
            subject_matches(gs, spec, listener, *blocker)
        }
        (TriggerCondition::Upkeep(whose), GameEvent::UpkeepBegins(p)) => {
            whose_matches(*whose, lc, *p)
        }
        (TriggerCondition::EndStep(whose), GameEvent::EndStepBegins(p)) => {
            whose_matches(*whose, lc, *p)
        }
        (TriggerCondition::BeginCombat(whose), GameEvent::CombatBegins(p)) => {
            whose_matches(*whose, lc, *p)
        }
        (TriggerCondition::Landfall, GameEvent::Enters(s)) => {
            let o = gs.obj(*s);
            o.chars.types.contains(CardTypes::LAND) && o.controller == lc
        }
        (TriggerCondition::CastSpell { whose, filter }, GameEvent::SpellCast { spell, caster }) => {
            whose_matches(*whose, lc, *caster) && spell_matches(gs, filter, *spell)
        }
        (
            TriggerCondition::DealsCombatDamageToPlayer(spec),
            GameEvent::CombatDamageToPlayer { source, .. },
        ) => subject_matches(gs, spec, listener, *source),
        (TriggerCondition::GainLife(whose), GameEvent::LifeGained { seat, .. }) => {
            whose_matches(*whose, lc, *seat)
        }
        (TriggerCondition::Draws(whose), GameEvent::DrewCard(p)) => whose_matches(*whose, lc, *p),
        _ => false,
    }
}

/// Move waiting triggers onto the stack in APNAP order. Triggers that
/// require targets pick them now; a trigger with no legal target is dropped.
pub fn flush_triggers(gs: &mut GameState, agents: &mut crate::agent::Agents) {
    if gs.pending_triggers.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut gs.pending_triggers);
    let order: Vec<Seat> = gs.apnap().collect();
    // APNAP: the active player's triggers go on the stack first, so later
    // seats' triggers resolve first. Stable within a seat.
    for seat in order {
        for t in pending.iter().filter(|t| t.controller == seat) {
            // Source may have changed zones; the trigger still resolves, but
            // we validate it against the incarnation that queued it when the
            // ability text is source-bound. Pragmatic: keep it.
            let ability = {
                let cf = gs.db.compiled_face(gs.obj(t.source).card, t.face);
                match cf.triggered.get(t.index as usize) {
                    Some(ta) => ta.ability.clone(),
                    None => continue,
                }
            };
            let mut targets = smallvec::SmallVec::<[Target; 2]>::new();
            let mut fizzled = false;
            for spec in &ability.targets {
                let cands = crate::actions::legal_targets(gs, spec, t.controller, Some(t.source));
                let need = spec.count.min() as usize;
                if cands.len() < need {
                    fizzled = true;
                    break;
                }
                let view = crate::view::View { gs, seat: t.controller };
                let picked = agents.get(t.controller).choose_targets(&view, spec, &cands);
                targets.extend(picked);
            }
            if fizzled {
                continue;
            }
            gs.stack.push(StackItem {
                source: t.source,
                source_incarnation: t.source_incarnation,
                card: gs.obj(t.source).card,
                controller: t.controller,
                kind: StackKind::Triggered { face: t.face, index: t.index },
                targets,
                x: 0,
                modes: smallvec::SmallVec::new(),
                trigger_subject: t.subject,
                trigger_player: t.player,
                exile_on_resolve: false,
            });
        }
    }
    let _ = Zone::Stack;
}
