//! Legal action enumeration and application: land drops, casting, and
//! ability activation. Priority timing rules live here.

use mtg_ir::{
    AddlCost, AltCost, CardTypes, Effect, KeywordSet, ManaCost, TargetCount, TargetSpec,
    TargetWhat,
};
use smallvec::SmallVec;

use crate::agent::Agents;
use crate::events::GameEvent;
use crate::filters::{obj_matches, spell_matches};
use crate::mana_pay::{self, TapPlan};
use crate::state::{
    GameState, ObjFlags, ObjectId, Seat, StackItem, StackKind, Step, Target, Zone,
};
use crate::triggers::process_event;
use crate::view::View;
use crate::zones;

#[derive(Debug, Clone)]
pub enum LegalAction {
    Pass,
    PlayLand { card: ObjectId, face: u8 },
    Cast { card: ObjectId, face: u8, from: Zone, alt: Option<u8>, plan: TapPlan },
    Activate { source: ObjectId, index: u8, plan: TapPlan },
    Cycle { card: ObjectId, plan: TapPlan },
}

fn sorcery_speed(gs: &GameState, seat: Seat) -> bool {
    gs.stack.is_empty()
        && seat == gs.active
        && matches!(gs.step, Step::Main1 | Step::Main2)
}

/// The generic-cost delta from battlefield cost-changing statics.
pub fn cost_delta(gs: &GameState, seat: Seat, card: ObjectId, face: u8) -> i32 {
    let mut delta = 0i32;
    for p in &gs.players {
        for &src in &p.battlefield {
            let o = gs.obj(src);
            if o.token.is_some() {
                continue;
            }
            let cf = gs.db.compiled_face(o.card, o.face);
            for st in &cf.statics {
                if let mtg_ir::StaticAbility::SpellCostDelta { whose, filter, delta: d } = st {
                    if crate::filters::whose_matches(*whose, o.controller, seat) {
                        // Match the card being cast against the spell filter
                        // using its face types.
                        let fc = gs.db.face(gs.obj(card).card, face);
                        if !filter.types.is_empty() && !fc.types.intersects(filter.types) {
                            continue;
                        }
                        if fc.types.intersects(filter.not_types) {
                            continue;
                        }
                        delta += *d as i32;
                    }
                }
            }
        }
    }
    delta
}

/// Commander tax for casting this object from the command zone.
fn commander_tax(gs: &GameState, seat: Seat, card: ObjectId) -> i32 {
    gs.player(seat)
        .cmdr_casts
        .iter()
        .find(|(id, _)| *id == card)
        .map(|(_, n)| 2 * *n as i32)
        .unwrap_or(0)
}

fn addl_costs_payable(gs: &GameState, seat: Seat, self_id: ObjectId, costs: &[AddlCost]) -> bool {
    for c in costs {
        match c {
            AddlCost::Sacrifice(f) => {
                let any = gs
                    .player(seat)
                    .battlefield
                    .iter()
                    .any(|&id| obj_matches(gs, f, id, seat, Some(self_id)));
                if !any {
                    return false;
                }
            }
            AddlCost::Discard(n) => {
                let others = gs.player(seat).hand.iter().filter(|&&id| id != self_id).count();
                if others < *n as usize {
                    return false;
                }
            }
            AddlCost::PayLife(n) => {
                if gs.player(seat).life <= *n as i32 {
                    return false;
                }
            }
        }
    }
    true
}

/// Are there enough legal targets to announce this ability?
fn targets_available(gs: &GameState, specs: &[TargetSpec], caster: Seat, source: Option<ObjectId>) -> bool {
    specs.iter().all(|spec| {
        let need = spec.count.min() as usize;
        need == 0 || legal_targets(gs, spec, caster, source).len() >= need
    })
}

/// Enumerate legal targets for a spec. Ward is treated as un-targetable by
/// opponents (a deliberate approximation; the tax is not modeled).
pub fn legal_targets(
    gs: &GameState,
    spec: &TargetSpec,
    caster: Seat,
    source: Option<ObjectId>,
) -> Vec<Target> {
    let mut out = Vec::new();
    let targetable = |gs: &GameState, id: ObjectId| -> bool {
        let o = gs.obj(id);
        let ch = &o.chars;
        if o.controller != caster {
            if ch.keywords.contains(KeywordSet::HEXPROOF) || ch.ward.is_some() {
                return false;
            }
        }
        if ch.keywords.contains(KeywordSet::SHROUD) {
            return false;
        }
        // Protection from the source's colors.
        if let Some(src) = source {
            if !ch.protection_from.is_empty() {
                let src_obj = gs.obj(src);
                let colors = if src_obj.token.is_some() {
                    src_obj.chars.colors
                } else {
                    mtg_ir::ManaCost::parse(&gs.db.face(src_obj.card, src_obj.face).mana_cost)
                        .map(|c| c.colors())
                        .unwrap_or_default()
                };
                if ch.protection_from.intersects(colors) {
                    return false;
                }
            }
        }
        true
    };

    match &spec.what {
        TargetWhat::Permanent(f) => {
            for p in &gs.players {
                for &id in &p.battlefield {
                    if obj_matches(gs, f, id, caster, source) && targetable(gs, id) {
                        out.push(Target::Obj(id, gs.obj(id).incarnation));
                    }
                }
            }
        }
        TargetWhat::CardInGraveyard(f, whose) => {
            for p in &gs.players {
                if !crate::filters::whose_matches(*whose, caster, p.seat) {
                    continue;
                }
                for &id in &p.graveyard {
                    if obj_matches(gs, f, id, caster, source) {
                        out.push(Target::Obj(id, gs.obj(id).incarnation));
                    }
                }
            }
        }
        TargetWhat::Player(pf) => {
            for s in gs.seats() {
                if gs.player(s).lost.is_some() {
                    continue;
                }
                let ok = match pf {
                    mtg_ir::PlayerFilter::Any => true,
                    mtg_ir::PlayerFilter::Opponent => s != caster,
                    mtg_ir::PlayerFilter::You => s == caster,
                };
                if ok {
                    out.push(Target::Player(s));
                }
            }
        }
        TargetWhat::AnyDamageable => {
            for p in &gs.players {
                for &id in &p.battlefield {
                    let t = gs.obj(id).chars.types;
                    if (t.contains(CardTypes::CREATURE)
                        || t.contains(CardTypes::PLANESWALKER)
                        || t.contains(CardTypes::BATTLE))
                        && targetable(gs, id)
                    {
                        out.push(Target::Obj(id, gs.obj(id).incarnation));
                    }
                }
            }
            for s in gs.seats() {
                if gs.player(s).lost.is_none() {
                    out.push(Target::Player(s));
                }
            }
        }
        TargetWhat::PlayerOrPlaneswalker => {
            for p in &gs.players {
                for &id in &p.battlefield {
                    if gs.obj(id).chars.types.contains(CardTypes::PLANESWALKER)
                        && targetable(gs, id)
                    {
                        out.push(Target::Obj(id, gs.obj(id).incarnation));
                    }
                }
            }
            for s in gs.seats() {
                if gs.player(s).lost.is_none() {
                    out.push(Target::Player(s));
                }
            }
        }
        TargetWhat::SpellOnStack(sf) => {
            for item in &gs.stack {
                if matches!(item.kind, StackKind::Spell { .. })
                    && spell_matches(gs, sf, item.source)
                {
                    out.push(Target::Obj(item.source, item.source_incarnation));
                }
            }
        }
    }
    out
}

pub fn legal_actions(gs: &GameState, seat: Seat) -> Vec<LegalAction> {
    let mut out = vec![LegalAction::Pass];
    if gs.player(seat).lost.is_some() {
        return out;
    }
    let sorcery_ok = sorcery_speed(gs, seat);
    let split_second_on_stack = gs.stack.iter().any(|i| {
        gs.db
            .compiled_face(i.card, match i.kind {
                StackKind::Spell { face } => face,
                _ => 0,
            })
            .keywords
            .contains(KeywordSet::SPLIT_SECOND)
            && matches!(i.kind, StackKind::Spell { .. })
    });
    if split_second_on_stack {
        return out;
    }

    // Land drops.
    if sorcery_ok && gs.player(seat).lands_played < gs.player(seat).land_limit {
        for &id in &gs.player(seat).hand {
            let card = gs.obj(id).card;
            let n_faces = gs.db.get(card).oracle.faces.len().min(2) as u8;
            for face in 0..n_faces {
                if gs.db.face(card, face).types.contains(CardTypes::LAND) {
                    out.push(LegalAction::PlayLand { card: id, face });
                }
            }
        }
    }

    // Casting from hand.
    let hand: Vec<ObjectId> = gs.player(seat).hand.clone();
    for id in hand {
        push_casts_for(gs, seat, id, Zone::Hand, sorcery_ok, &mut out);
    }
    // Alternative casts from the graveyard (flashback, escape).
    let gy: Vec<ObjectId> = gs.player(seat).graveyard.clone();
    for id in gy {
        push_casts_for(gs, seat, id, Zone::Graveyard, sorcery_ok, &mut out);
    }
    // Commanders from the command zone.
    if gs.cfg.commander {
        let cmd: Vec<ObjectId> = gs.player(seat).command.clone();
        for id in cmd {
            push_casts_for(gs, seat, id, Zone::Command, sorcery_ok, &mut out);
        }
    }

    // Activated abilities on the battlefield.
    let mine: Vec<ObjectId> = gs.player(seat).battlefield.clone();
    for id in mine {
        let o = gs.obj(id);
        if o.token.is_some() {
            continue;
        }
        let cf = gs.db.compiled_face(o.card, o.face);
        for (i, ab) in cf.activated.iter().enumerate() {
            if ab.zone != mtg_ir::AbilityZone::Battlefield {
                continue;
            }
            if (ab.sorcery_speed || ab.loyalty.is_some()) && !sorcery_ok {
                continue;
            }
            if ab.once_per_turn || ab.loyalty.is_some() {
                if o.flags.contains(ObjFlags::ACTIVATED_TURN) {
                    continue;
                }
            }
            if ab.cost.tap_self && (o.tapped || (o.sick && o.is_creature())) {
                continue;
            }
            if ab.cost.sac_self && o.token.is_some() {
                continue;
            }
            if let Some(delta) = ab.loyalty {
                if delta < 0 && (o.counter_count(mtg_ir::CounterKind::Loyalty) as i32) < -delta as i32 {
                    continue;
                }
            }
            if ab.cost.pay_life > 0 && gs.player(seat).life <= ab.cost.pay_life as i32 {
                continue;
            }
            if !targets_available(gs, &ab.ability.targets, seat, Some(id)) {
                continue;
            }
            let plan = match &ab.cost.mana {
                Some(cost) => match mana_pay::solve(gs, seat, cost, 0) {
                    Some(p) => p,
                    None => continue,
                },
                None => TapPlan::default(),
            };
            out.push(LegalAction::Activate { source: id, index: i as u8, plan });
        }
    }

    // Cycling from hand.
    for &id in &gs.player(seat).hand {
        let o = gs.obj(id);
        let cf = gs.db.compiled_face(o.card, 0);
        if let Some(cost) = &cf.cycling {
            if let Some(plan) = mana_pay::solve(gs, seat, cost, 0) {
                out.push(LegalAction::Cycle { card: id, plan });
            }
        }
    }

    out
}

fn push_casts_for(
    gs: &GameState,
    seat: Seat,
    id: ObjectId,
    from: Zone,
    sorcery_ok: bool,
    out: &mut Vec<LegalAction>,
) {
    let o = gs.obj(id);
    let card = o.card;
    let n_faces = gs.db.get(card).compiled.faces.len() as u8;
    for face in 0..n_faces {
        let fc = gs.db.face(card, face);
        let cf = gs.db.compiled_face(card, face);
        if fc.types.contains(CardTypes::LAND) {
            continue;
        }
        let instant_ok = fc.types.contains(CardTypes::INSTANT) || cf.keywords.contains(KeywordSet::FLASH);
        if !instant_ok && !sorcery_ok {
            continue;
        }
        let delta = cost_delta(gs, seat, id, face)
            + if from == Zone::Command { commander_tax(gs, seat, id) } else { 0 };
        let target_specs: &[TargetSpec] = match &cf.spell {
            Some(sa) => match &sa.effect {
                // Modal target availability is validated per chosen mode at
                // apply time; announcing requires at least one viable mode.
                Effect::Modal { .. } => &[],
                _ => &sa.targets,
            },
            None => &[],
        };
        if !targets_available(gs, target_specs, seat, Some(id)) {
            continue;
        }
        if !addl_costs_payable(gs, seat, id, &cf.addl_costs) {
            continue;
        }
        match from {
            Zone::Hand => {
                if let Some(cost) = &cf.cost {
                    if let Some(plan) = mana_pay::solve_with_delta(gs, seat, cost, 0, delta) {
                        out.push(LegalAction::Cast { card: id, face, from, alt: None, plan });
                    }
                }
            }
            Zone::Graveyard => {
                for (ai, alt) in cf.alt_costs.iter().enumerate() {
                    let cost = match alt {
                        AltCost::Flashback(c) => c,
                        AltCost::Escape { cost, exile_count } => {
                            let others = gs.player(seat).graveyard.iter().filter(|&&g| g != id).count();
                            if others < *exile_count as usize {
                                continue;
                            }
                            cost
                        }
                        _ => continue,
                    };
                    if let Some(plan) = mana_pay::solve_with_delta(gs, seat, cost, 0, delta) {
                        out.push(LegalAction::Cast {
                            card: id,
                            face,
                            from,
                            alt: Some(ai as u8),
                            plan,
                        });
                    }
                }
            }
            Zone::Command => {
                if let Some(cost) = &cf.cost {
                    if let Some(plan) = mana_pay::solve_with_delta(gs, seat, cost, 0, delta) {
                        out.push(LegalAction::Cast { card: id, face, from, alt: None, plan });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Apply a chosen action. Returns false when the action turned out to be
/// illegal at execution time (the caller treats it as a forced pass).
pub fn apply_action(gs: &mut GameState, agents: &mut Agents, seat: Seat, action: &LegalAction) -> bool {
    match action {
        LegalAction::Pass => false,
        LegalAction::PlayLand { card, face } => {
            gs.player_mut(seat).lands_played += 1;
            gs.obj_mut(*card).face = *face;
            let land = *card;
            // Playing a land does not use the stack.
            zones::move_to(gs, land, Zone::Battlefield, Some(seat));
            gs.obj_mut(land).face = *face;
            crate::layers::recompute_chars(gs);
            process_event(gs, GameEvent::LandPlayed { seat, land });
            gs.tracef(|| format!("seat {seat} plays a land"));
            true
        }
        LegalAction::Cast { card, face, from, alt, plan } => {
            cast_spell(gs, agents, seat, *card, *face, *from, *alt, plan.clone())
        }
        LegalAction::Activate { source, index, plan } => {
            activate_ability(gs, agents, seat, *source, *index, plan.clone())
        }
        LegalAction::Cycle { card, plan } => {
            let cf = gs.db.compiled_face(gs.obj(*card).card, 0);
            let cost = match &cf.cycling {
                Some(c) => c.clone(),
                None => return false,
            };
            if !mana_pay::execute(gs, seat, plan, &cost, 0) {
                return false;
            }
            zones::move_to(gs, *card, Zone::Graveyard, None);
            zones::draw_cards(gs, seat, 1);
            true
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cast_spell(
    gs: &mut GameState,
    agents: &mut Agents,
    seat: Seat,
    card: ObjectId,
    face: u8,
    from: Zone,
    alt: Option<u8>,
    plan: TapPlan,
) -> bool {
    let cf = gs.db.compiled_face(gs.obj(card).card, face).clone();
    let delta = cost_delta(gs, seat, card, face)
        + if from == Zone::Command { commander_tax(gs, seat, card) } else { 0 };

    // X value: probe upward for the largest payable X, then ask the agent.
    let mut x = 0u32;
    let base_cost: ManaCost = match alt {
        Some(ai) => match cf.alt_costs.get(ai as usize) {
            Some(AltCost::Flashback(c)) => c.clone(),
            Some(AltCost::Escape { cost, .. }) => cost.clone(),
            Some(AltCost::Foretell(c)) | Some(AltCost::Evoke(c)) => c.clone(),
            None => return false,
        },
        None => match &cf.cost {
            Some(c) => c.clone(),
            None => return false,
        },
    };
    if cf.x_spell && base_cost.x_count > 0 {
        let mut max_x = 0;
        for probe in 1..=20u32 {
            if mana_pay::solve_with_delta(gs, seat, &base_cost, probe, delta).is_some() {
                max_x = probe;
            } else {
                break;
            }
        }
        let view = View { gs, seat };
        x = agents.get(seat).choose_x(&view, max_x).min(max_x);
    }

    // Modes and targets.
    let mut modes: SmallVec<[u8; 2]> = SmallVec::new();
    let mut specs: Vec<TargetSpec> = Vec::new();
    if let Some(sa) = &cf.spell {
        if let Effect::Modal { choose, modes: mode_list } = &sa.effect {
            let view = View { gs, seat };
            let picked = agents.get(seat).choose_mode(&view, mode_list, *choose);
            for &m in picked.iter().take(*choose as usize) {
                if let Some(mode) = mode_list.get(m as usize) {
                    if targets_available(gs, &mode.targets, seat, Some(card)) {
                        modes.push(m);
                        specs.extend(mode.targets.iter().cloned());
                    }
                }
            }
            if modes.is_empty() {
                // Fall back to any castable mode.
                for (mi, mode) in mode_list.iter().enumerate() {
                    if targets_available(gs, &mode.targets, seat, Some(card)) {
                        modes.push(mi as u8);
                        specs.extend(mode.targets.iter().cloned());
                        break;
                    }
                }
                if modes.is_empty() {
                    return false;
                }
            }
        } else {
            specs = sa.targets.clone();
        }
    }
    let mut targets: SmallVec<[Target; 2]> = SmallVec::new();
    for spec in &specs {
        let cands = legal_targets(gs, spec, seat, Some(card));
        let need = spec.count.min() as usize;
        if cands.len() < need {
            return false;
        }
        if cands.is_empty() {
            continue;
        }
        let view = View { gs, seat };
        let picked = agents.get(seat).choose_targets(&view, spec, &cands);
        let take = match spec.count {
            TargetCount::Exactly(n) => n as usize,
            TargetCount::UpTo(n) => picked.len().min(n as usize),
        };
        for t in picked.into_iter().take(take.max(need)) {
            targets.push(t);
        }
    }

    // Pay: mana first (solver already validated), then additional costs.
    if !mana_pay::execute_with_delta(gs, seat, &plan, &base_cost, x, delta) {
        return false;
    }
    let addl = cf.addl_costs.clone();
    for c in &addl {
        match c {
            AddlCost::Sacrifice(f) => {
                let cands: Vec<ObjectId> = gs
                    .player(seat)
                    .battlefield
                    .iter()
                    .copied()
                    .filter(|&i| obj_matches(gs, f, i, seat, Some(card)))
                    .collect();
                let view = View { gs, seat };
                let picked = agents.get(seat).choose_sacrifice(&view, &cands, 1);
                for p in picked.into_iter().take(1) {
                    crate::sba::die(gs, p);
                }
            }
            AddlCost::Discard(n) => {
                let hand: Vec<ObjectId> =
                    gs.player(seat).hand.iter().copied().filter(|&i| i != card).collect();
                let view = View { gs, seat };
                let picked = agents.get(seat).choose_discard(&view, &hand, *n as usize);
                for p in picked.into_iter().take(*n as usize) {
                    zones::move_to(gs, p, Zone::Graveyard, None);
                }
            }
            AddlCost::PayLife(n) => {
                gs.player_mut(seat).life -= *n as i32;
            }
        }
    }
    // Escape exiles fuel from the graveyard.
    if let Some(AltCost::Escape { exile_count, .. }) = alt.and_then(|ai| cf.alt_costs.get(ai as usize))
    {
        let fuel: Vec<ObjectId> = gs
            .player(seat)
            .graveyard
            .iter()
            .copied()
            .filter(|&g| g != card)
            .take(*exile_count as usize)
            .collect();
        for f in fuel {
            zones::move_to(gs, f, Zone::Exile, None);
        }
    }

    // Commander tax bookkeeping.
    if from == Zone::Command {
        let p = gs.player_mut(seat);
        if let Some(slot) = p.cmdr_casts.iter_mut().find(|(id, _)| *id == card) {
            slot.1 += 1;
        } else {
            p.cmdr_casts.push((card, 1));
        }
    }

    // Move the card onto the stack.
    {
        let owner = gs.obj(card).owner;
        match from {
            Zone::Hand => gs.player_mut(owner).hand.retain(|&i| i != card),
            Zone::Graveyard => gs.player_mut(owner).graveyard.retain(|&i| i != card),
            Zone::Command => gs.player_mut(owner).command.retain(|&i| i != card),
            _ => return false,
        }
        let ts = gs.next_ts();
        let o = gs.obj_mut(card);
        o.zone = Zone::Stack;
        o.incarnation += 1;
        o.face = face;
        o.ts = ts;
    }
    let exile_on_resolve = matches!(
        alt.and_then(|ai| cf.alt_costs.get(ai as usize)),
        Some(AltCost::Flashback(_)) | Some(AltCost::Escape { .. })
    );
    gs.stack.push(StackItem {
        source: card,
        source_incarnation: gs.obj(card).incarnation,
        card: gs.obj(card).card,
        controller: seat,
        kind: StackKind::Spell { face },
        targets,
        x,
        modes,
        trigger_subject: None,
        trigger_player: None,
        exile_on_resolve,
    });
    process_event(gs, GameEvent::SpellCast { spell: card, caster: seat });
    let name = gs.name_of(card);
    gs.tracef(|| format!("seat {seat} casts {name}"));
    true
}

fn activate_ability(
    gs: &mut GameState,
    agents: &mut Agents,
    seat: Seat,
    source: ObjectId,
    index: u8,
    plan: TapPlan,
) -> bool {
    let o = gs.obj(source);
    let cf = gs.db.compiled_face(o.card, o.face);
    let ab = match cf.activated.get(index as usize) {
        Some(a) => a.clone(),
        None => return false,
    };
    let face = gs.obj(source).face;

    // Targets first: aborting later would leave costs half-paid.
    let mut targets: SmallVec<[Target; 2]> = SmallVec::new();
    for spec in &ab.ability.targets {
        let cands = legal_targets(gs, spec, seat, Some(source));
        let need = spec.count.min() as usize;
        if cands.len() < need {
            return false;
        }
        if cands.is_empty() {
            continue;
        }
        let view = View { gs, seat };
        let picked = agents.get(seat).choose_targets(&view, spec, &cands);
        for t in picked.into_iter().take(spec.count.max() as usize) {
            targets.push(t);
        }
    }

    // Pay costs.
    if let Some(cost) = &ab.cost.mana {
        if !mana_pay::execute(gs, seat, &plan, cost, 0) {
            return false;
        }
    }
    if ab.cost.tap_self {
        gs.obj_mut(source).tapped = true;
    }
    if ab.cost.pay_life > 0 {
        gs.player_mut(seat).life -= ab.cost.pay_life as i32;
    }
    if let Some((kind, n)) = &ab.cost.remove_counters {
        gs.obj_mut(source).add_counters(*kind, -(*n as i16));
    }
    if let Some(delta) = ab.loyalty {
        gs.obj_mut(source).add_counters(mtg_ir::CounterKind::Loyalty, delta as i16);
        gs.obj_mut(source).flags |= ObjFlags::ACTIVATED_TURN;
    }
    if ab.once_per_turn {
        gs.obj_mut(source).flags |= ObjFlags::ACTIVATED_TURN;
    }
    if ab.cost.discard_cards > 0 {
        let hand: Vec<ObjectId> = gs.player(seat).hand.clone();
        let view = View { gs, seat };
        let picked = agents.get(seat).choose_discard(&view, &hand, ab.cost.discard_cards as usize);
        for p in picked.into_iter().take(ab.cost.discard_cards as usize) {
            zones::move_to(gs, p, Zone::Graveyard, None);
        }
    }
    if let Some(f) = &ab.cost.sac {
        let cands: Vec<ObjectId> = gs
            .player(seat)
            .battlefield
            .iter()
            .copied()
            .filter(|&i| obj_matches(gs, f, i, seat, Some(source)))
            .collect();
        if cands.is_empty() {
            return false;
        }
        let view = View { gs, seat };
        let picked = agents.get(seat).choose_sacrifice(&view, &cands, 1);
        for p in picked.into_iter().take(1) {
            crate::sba::die(gs, p);
        }
    }
    if ab.cost.sac_self {
        crate::sba::die(gs, source);
    }

    gs.stack.push(StackItem {
        source,
        source_incarnation: gs.obj(source).incarnation,
        card: gs.obj(source).card,
        controller: seat,
        kind: StackKind::Activated { face, index },
        targets,
        x: 0,
        modes: SmallVec::new(),
        trigger_subject: None,
        trigger_player: None,
        exile_on_resolve: false,
    });
    true
}
