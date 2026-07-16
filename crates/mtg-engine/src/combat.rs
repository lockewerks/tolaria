//! Combat: attack and block declaration, damage assignment ordering,
//! first and double strike, trample, deathtouch, and the shared damage
//! application path (also used by spells and fights).

use mtg_ir::{CardTypes, ColorSet, CounterKind, KeywordSet};
use smallvec::SmallVec;

use crate::agent::Agents;
use crate::events::GameEvent;
use crate::state::{GameState, ObjFlags, ObjectId, Seat, Zone};
use crate::triggers::process_event;
use crate::view::View;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defender {
    Player(Seat),
    Planeswalker(ObjectId),
}

#[derive(Debug, Clone)]
pub struct Attack {
    pub attacker: ObjectId,
    pub defender: Defender,
    pub blockers: SmallVec<[ObjectId; 2]>,
}

#[derive(Debug, Clone, Default)]
pub struct CombatState {
    pub attacks: Vec<Attack>,
}

#[derive(Debug, Clone, Copy)]
pub enum DamageTarget {
    Obj(ObjectId),
    Player(Seat),
}

fn kw(gs: &GameState, id: ObjectId) -> KeywordSet {
    gs.obj(id).chars.keywords
}

fn colors_of_source(gs: &GameState, id: ObjectId) -> ColorSet {
    let o = gs.obj(id);
    if o.zone == Zone::Battlefield || o.token.is_some() {
        o.chars.colors
    } else {
        mtg_ir::ManaCost::parse(&gs.db.face(o.card, o.face).mana_cost)
            .map(|c| c.colors())
            .unwrap_or_default()
    }
}

pub fn gain_life(gs: &mut GameState, seat: Seat, n: i32) {
    if n <= 0 {
        return;
    }
    gs.player_mut(seat).life += n;
    process_event(gs, GameEvent::LifeGained { seat, amount: n });
}

/// Apply damage from a source to a creature, planeswalker, battle, or
/// player. Handles protection, prevention, lifelink, deathtouch marking,
/// infect, wither, and toxic. Returns the amount actually dealt.
pub fn apply_damage(
    gs: &mut GameState,
    source: ObjectId,
    target: DamageTarget,
    amount: i32,
    combat: bool,
) -> i32 {
    if amount <= 0 {
        return 0;
    }
    let src_kw = kw(gs, source);
    let src_colors = colors_of_source(gs, source);
    let src_controller = gs.obj(source).controller;

    let dealt = match target {
        DamageTarget::Obj(id) => {
            if gs.obj(id).zone != Zone::Battlefield {
                return 0;
            }
            let ch = &gs.obj(id).chars;
            if ch.protection_from.intersects(src_colors) {
                return 0;
            }
            if prevented(gs, id) {
                return 0;
            }
            let types = ch.types;
            if types.contains(CardTypes::PLANESWALKER) || types.contains(CardTypes::BATTLE) {
                gs.obj_mut(id).add_counters(CounterKind::Loyalty, -(amount as i16));
            } else if src_kw.contains(KeywordSet::INFECT) || src_kw.contains(KeywordSet::WITHER) {
                gs.obj_mut(id).add_counters(CounterKind::MinusOne, amount as i16);
                crate::layers::recompute_chars(gs);
            } else {
                gs.obj_mut(id).damage += amount;
                if src_kw.contains(KeywordSet::DEATHTOUCH) {
                    gs.obj_mut(id).flags |= ObjFlags::DEATHTOUCHED;
                }
            }
            amount
        }
        DamageTarget::Player(seat) => {
            if gs.player(seat).lost.is_some() {
                return 0;
            }
            if src_kw.contains(KeywordSet::INFECT) {
                gs.player_mut(seat).poison += amount.min(127) as u8;
            } else {
                gs.player_mut(seat).life -= amount;
            }
            if combat {
                let toxic = gs.obj(source).chars.toxic;
                if toxic > 0 {
                    gs.player_mut(seat).poison += toxic;
                }
                if gs.obj(source).flags.contains(ObjFlags::IS_COMMANDER) {
                    let p = gs.player_mut(seat);
                    if let Some(slot) = p.cmdr_damage.iter_mut().find(|(id, _)| *id == source) {
                        slot.1 += amount;
                    } else {
                        p.cmdr_damage.push((source, amount));
                    }
                }
                process_event(gs, GameEvent::CombatDamageToPlayer { source, player: seat });
            }
            amount
        }
    };

    if dealt > 0 && src_kw.contains(KeywordSet::LIFELINK) {
        gain_life(gs, src_controller, dealt);
    }
    dealt
}

/// Damage prevention from field-wide PreventDamage replacements.
fn prevented(gs: &GameState, id: ObjectId) -> bool {
    for p in &gs.players {
        for &src in &p.battlefield {
            let o = gs.obj(src);
            if o.token.is_some() {
                continue;
            }
            let cf = gs.db.compiled_face(o.card, o.face);
            for r in &cf.replacements {
                if !matches!(r.kind, mtg_ir::ReplKind::PreventDamage) {
                    continue;
                }
                let hit = match &r.scope {
                    mtg_ir::ReplScope::This => src == id,
                    mtg_ir::ReplScope::Yours(f) => {
                        gs.obj(id).controller == o.controller
                            && crate::filters::obj_matches(gs, f, id, o.controller, Some(src))
                    }
                    mtg_ir::ReplScope::All(f) => {
                        crate::filters::obj_matches(gs, f, id, o.controller, Some(src))
                    }
                };
                if hit {
                    return true;
                }
            }
        }
    }
    false
}

pub fn eligible_attackers(gs: &GameState) -> Vec<ObjectId> {
    let seat = gs.active;
    gs.player(seat)
        .battlefield
        .iter()
        .copied()
        .filter(|&id| {
            let o = gs.obj(id);
            o.is_creature()
                && !o.tapped
                && (!o.sick || o.chars.keywords.contains(KeywordSet::HASTE))
                && !o.chars.keywords.contains(KeywordSet::DEFENDER)
        })
        .collect()
}

pub fn possible_defenders(gs: &GameState) -> Vec<Defender> {
    let mut out = Vec::new();
    for s in gs.opponents_of(gs.active).collect::<Vec<_>>() {
        out.push(Defender::Player(s));
        for &id in &gs.player(s).battlefield {
            if gs.obj(id).chars.types.contains(CardTypes::PLANESWALKER) {
                out.push(Defender::Planeswalker(id));
            }
        }
    }
    out
}

/// Returns true when at least one attack was declared.
pub fn declare_attackers(gs: &mut GameState, agents: &mut Agents) -> bool {
    let candidates = eligible_attackers(gs);
    let defenders = possible_defenders(gs);
    if candidates.is_empty() || defenders.is_empty() {
        gs.combat = Some(CombatState::default());
        return false;
    }
    let seat = gs.active;
    let view = View { gs, seat };
    let mut picks = agents.get(seat).declare_attackers(&view, &candidates, &defenders);

    // Creatures that must attack are added if the agent left them home.
    for &id in &candidates {
        if gs.obj(id).chars.keywords.contains(KeywordSet::ATTACKS_EACH_TURN)
            && !picks.iter().any(|(a, _)| *a == id)
        {
            picks.push((id, defenders[0]));
        }
    }

    let mut combat = CombatState::default();
    for (attacker, defender) in picks {
        if !candidates.contains(&attacker) {
            continue;
        }
        let valid = match defender {
            Defender::Player(s) => {
                s != seat && gs.player(s).lost.is_none()
            }
            Defender::Planeswalker(id) => {
                gs.obj(id).zone == Zone::Battlefield && gs.obj(id).controller != seat
            }
        };
        if !valid || combat.attacks.iter().any(|a| a.attacker == attacker) {
            continue;
        }
        combat.attacks.push(Attack { attacker, defender, blockers: SmallVec::new() });
    }

    let any = !combat.attacks.is_empty();
    let attackers: Vec<ObjectId> = combat.attacks.iter().map(|a| a.attacker).collect();
    gs.combat = Some(combat);
    for id in attackers {
        gs.obj_mut(id).flags |= ObjFlags::ATTACKING;
        if !gs.obj(id).chars.keywords.contains(KeywordSet::VIGILANCE) {
            gs.obj_mut(id).tapped = true;
        }
        process_event(gs, GameEvent::Attacks(id));
        let name = gs.name_of(id);
        gs.tracef(|| format!("{name} attacks"));
    }
    any
}

pub fn can_block(gs: &GameState, blocker: ObjectId, attacker: ObjectId) -> bool {
    let b = gs.obj(blocker);
    if !b.is_creature() || b.tapped || b.zone != Zone::Battlefield {
        return false;
    }
    let bk = b.chars.keywords;
    if bk.contains(KeywordSet::CANT_BLOCK) {
        return false;
    }
    let a = gs.obj(attacker);
    let ak = a.chars.keywords;
    if ak.contains(KeywordSet::UNBLOCKABLE) {
        return false;
    }
    if ak.contains(KeywordSet::FLYING)
        && !(bk.contains(KeywordSet::FLYING) || bk.contains(KeywordSet::REACH))
    {
        return false;
    }
    if ak.contains(KeywordSet::SHADOW) != bk.contains(KeywordSet::SHADOW) {
        return false;
    }
    if ak.contains(KeywordSet::FEAR)
        && !(b.chars.types.contains(CardTypes::ARTIFACT) || b.chars.colors.contains(ColorSet::B))
    {
        return false;
    }
    if ak.contains(KeywordSet::INTIMIDATE)
        && !(b.chars.types.contains(CardTypes::ARTIFACT) || b.chars.colors.intersects(a.chars.colors))
    {
        return false;
    }
    // Protection from any of the blocker's colors forbids the block.
    if a.chars.protection_from.intersects(b.chars.colors) {
        return false;
    }
    true
}

pub fn declare_blockers(gs: &mut GameState, agents: &mut Agents) {
    let Some(combat) = gs.combat.clone() else { return };
    if combat.attacks.is_empty() {
        return;
    }

    // Each defending seat declares blocks for the attacks pointed at them.
    let mut defending: Vec<Seat> = Vec::new();
    for a in &combat.attacks {
        let s = match a.defender {
            Defender::Player(s) => s,
            Defender::Planeswalker(id) => gs.obj(id).controller,
        };
        if !defending.contains(&s) {
            defending.push(s);
        }
    }

    let mut blocks: Vec<(ObjectId, ObjectId)> = Vec::new();
    for seat in defending {
        let my_attackers: Vec<ObjectId> = combat
            .attacks
            .iter()
            .filter(|a| match a.defender {
                Defender::Player(s) => s == seat,
                Defender::Planeswalker(id) => gs.obj(id).controller == seat,
            })
            .map(|a| a.attacker)
            .collect();
        let candidates: Vec<ObjectId> = gs
            .player(seat)
            .battlefield
            .iter()
            .copied()
            .filter(|&id| gs.obj(id).is_creature() && !gs.obj(id).tapped)
            .collect();
        if my_attackers.is_empty() || candidates.is_empty() {
            continue;
        }
        let view = View { gs, seat };
        let picks = agents.get(seat).declare_blockers(&view, &my_attackers, &candidates);
        for (blocker, attacker) in picks {
            if my_attackers.contains(&attacker)
                && candidates.contains(&blocker)
                && can_block(gs, blocker, attacker)
                && !blocks.iter().any(|(b, _)| *b == blocker)
            {
                blocks.push((blocker, attacker));
            }
        }
    }

    // Menace: solo blocks on menace attackers are dropped.
    let mut combat = combat;
    for a in &mut combat.attacks {
        let mine: Vec<ObjectId> =
            blocks.iter().filter(|(_, at)| *at == a.attacker).map(|(b, _)| *b).collect();
        if gs.obj(a.attacker).chars.keywords.contains(KeywordSet::MENACE) && mine.len() == 1 {
            continue;
        }
        a.blockers = mine.into_iter().collect();
    }

    // Attacker orders multi-blocks.
    let active = gs.active;
    for a in &mut combat.attacks {
        if a.blockers.len() > 1 {
            let view = View { gs, seat: active };
            let order =
                agents.get(active).order_blockers(&view, a.attacker, a.blockers.as_slice());
            let mut ordered: SmallVec<[ObjectId; 2]> = SmallVec::new();
            for b in order {
                if a.blockers.contains(&b) && !ordered.contains(&b) {
                    ordered.push(b);
                }
            }
            for &b in a.blockers.iter() {
                if !ordered.contains(&b) {
                    ordered.push(b);
                }
            }
            a.blockers = ordered;
        }
    }

    // Flags and events.
    let mut events = Vec::new();
    for a in &combat.attacks {
        if !a.blockers.is_empty() {
            gs.obj_mut(a.attacker).flags |= ObjFlags::BLOCKED;
        }
        for &b in &a.blockers {
            gs.obj_mut(b).flags |= ObjFlags::BLOCKING;
            events.push(GameEvent::Blocks { blocker: b, attacker: a.attacker });
        }
    }
    gs.combat = Some(combat);
    for e in events {
        process_event(gs, e);
    }
}

fn participates(gs: &GameState, id: ObjectId, first_strike_step: bool) -> bool {
    let k = kw(gs, id);
    if first_strike_step {
        k.contains(KeywordSet::FIRST_STRIKE) || k.contains(KeywordSet::DOUBLE_STRIKE)
    } else {
        !k.contains(KeywordSet::FIRST_STRIKE) || k.contains(KeywordSet::DOUBLE_STRIKE)
    }
}

/// True when any combatant has first or double strike.
pub fn has_first_strike_step(gs: &GameState) -> bool {
    let Some(combat) = &gs.combat else { return false };
    combat.attacks.iter().any(|a| {
        let ak = kw(gs, a.attacker);
        ak.contains(KeywordSet::FIRST_STRIKE)
            || ak.contains(KeywordSet::DOUBLE_STRIKE)
            || a.blockers.iter().any(|&b| {
                let bk = kw(gs, b);
                bk.contains(KeywordSet::FIRST_STRIKE) || bk.contains(KeywordSet::DOUBLE_STRIKE)
            })
    })
}

pub fn combat_damage(gs: &mut GameState, first_strike_step: bool) {
    let Some(combat) = gs.combat.clone() else { return };
    let mut hits: Vec<(ObjectId, DamageTarget, i32)> = Vec::new();

    for a in &combat.attacks {
        let attacker = a.attacker;
        let ao = gs.obj(attacker);
        if ao.zone != Zone::Battlefield || !ao.flags.contains(ObjFlags::ATTACKING) {
            continue;
        }
        let alive_blockers: Vec<ObjectId> = a
            .blockers
            .iter()
            .copied()
            .filter(|&b| gs.obj(b).zone == Zone::Battlefield)
            .collect();

        // Attacker's damage.
        if participates(gs, attacker, first_strike_step) {
            let power = gs.obj(attacker).chars.power;
            if power > 0 {
                let deathtouch = kw(gs, attacker).contains(KeywordSet::DEATHTOUCH);
                let trample = kw(gs, attacker).contains(KeywordSet::TRAMPLE);
                if alive_blockers.is_empty() {
                    if !gs.obj(attacker).flags.contains(ObjFlags::BLOCKED) || trample {
                        hits.push((
                            attacker,
                            match a.defender {
                                Defender::Player(s) => DamageTarget::Player(s),
                                Defender::Planeswalker(id) => DamageTarget::Obj(id),
                            },
                            power,
                        ));
                    }
                } else {
                    let mut remaining = power;
                    let last = *alive_blockers.last().unwrap();
                    for &b in &alive_blockers {
                        if remaining <= 0 {
                            break;
                        }
                        let lethal = if deathtouch {
                            1
                        } else {
                            (gs.obj(b).chars.toughness - gs.obj(b).damage).max(0)
                        };
                        let assign = if b == last && !trample {
                            remaining
                        } else {
                            lethal.min(remaining)
                        };
                        if assign > 0 {
                            hits.push((attacker, DamageTarget::Obj(b), assign));
                            remaining -= assign;
                        }
                    }
                    if trample && remaining > 0 {
                        hits.push((
                            attacker,
                            match a.defender {
                                Defender::Player(s) => DamageTarget::Player(s),
                                Defender::Planeswalker(id) => DamageTarget::Obj(id),
                            },
                            remaining,
                        ));
                    }
                }
            }
        }

        // Blockers' damage.
        for &b in &alive_blockers {
            if participates(gs, b, first_strike_step) {
                let p = gs.obj(b).chars.power;
                if p > 0 {
                    hits.push((b, DamageTarget::Obj(attacker), p));
                }
            }
        }
    }

    // Simultaneous application.
    for (src, tgt, n) in hits {
        apply_damage(gs, src, tgt, n, true);
    }
}

pub fn end_combat(gs: &mut GameState) {
    for p in 0..gs.players.len() {
        let ids: Vec<ObjectId> = gs.players[p].battlefield.clone();
        for id in ids {
            gs.obj_mut(id)
                .flags
                .remove(ObjFlags::ATTACKING | ObjFlags::BLOCKING | ObjFlags::BLOCKED);
        }
    }
    gs.combat = None;
}
