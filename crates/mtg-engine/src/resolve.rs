//! Stack resolution: the Effect IR interpreter. Unknown or unimplemented
//! constructs resolve as no-ops; the compiler's coverage tiers own honesty
//! about what was dropped.

use mtg_ir::{
    Condition, CounterKind, Duration, Effect, ObjSel, PlayerSel, Recipient, ReplKind, ReplScope,
    SearchDest, ValueExpr,
};
use smallvec::SmallVec;

use crate::agent::{Agents, YesNo};
use crate::combat::{apply_damage, gain_life, DamageTarget};
use crate::filters::obj_matches;
use crate::state::{
    FloatingEffect, GameState, ObjectId, Seat, StackItem, StackKind, Target, Zone,
};
use crate::view::View;
use crate::zones;

pub struct Ctx {
    pub source: ObjectId,
    pub controller: Seat,
    pub targets: SmallVec<[Target; 2]>,
    pub x: u32,
    pub subject: Option<ObjectId>,
    pub tplayer: Option<Seat>,
}

fn target_alive(gs: &GameState, t: Target) -> bool {
    match t {
        Target::Obj(id, inc) => {
            let o = gs.obj(id);
            o.incarnation == inc && !matches!(o.zone, Zone::Limbo)
        }
        Target::Player(s) => gs.player(s).lost.is_none(),
    }
}

pub fn resolve(gs: &mut GameState, agents: &mut Agents, item: StackItem) {
    match item.kind {
        StackKind::Spell { face } => resolve_spell(gs, agents, item, face),
        StackKind::Activated { face, index } => {
            let ability = {
                let cf = gs.db.compiled_face(item.card, face);
                match cf.activated.get(index as usize) {
                    Some(a) => a.ability.clone(),
                    None => return,
                }
            };
            run_ability(gs, agents, &item, &ability.effect);
        }
        StackKind::Triggered { face, index } => {
            let ability = {
                let cf = gs.db.compiled_face(item.card, face);
                match cf.triggered.get(index as usize) {
                    Some(t) => t.ability.clone(),
                    None => return,
                }
            };
            run_ability(gs, agents, &item, &ability.effect);
        }
    }
}

fn run_ability(gs: &mut GameState, agents: &mut Agents, item: &StackItem, effect: &Effect) {
    if fizzles(gs, item) {
        return;
    }
    let mut ctx = Ctx {
        source: item.source,
        controller: item.controller,
        targets: item.targets.clone(),
        x: item.x,
        subject: item.trigger_subject,
        tplayer: item.trigger_player,
    };
    exec(gs, agents, &mut ctx, effect);
}

fn fizzles(gs: &GameState, item: &StackItem) -> bool {
    !item.targets.is_empty() && !item.targets.iter().any(|&t| target_alive(gs, t))
}

fn resolve_spell(gs: &mut GameState, agents: &mut Agents, item: StackItem, face: u8) {
    let fc_types = gs.db.face(item.card, face).types;
    if zones::is_permanent_types(fc_types) {
        // Permanent spell: enter the battlefield under caster's control.
        gs.obj_mut(item.source).face = face;
        zones::move_to(gs, item.source, Zone::Battlefield, Some(item.controller));
        gs.obj_mut(item.source).face = face;
        crate::layers::recompute_chars(gs);

        // Auras attach to their cast target or die trying.
        let is_aura = gs
            .db
            .face(item.card, face)
            .subtypes
            .iter()
            .any(|s| s.as_ref() == "aura");
        if is_aura {
            let host = item.targets.iter().copied().find(|&t| target_alive(gs, t));
            match host {
                Some(Target::Obj(h, _)) => {
                    gs.obj_mut(item.source).attached_to = Some(h);
                    gs.obj_mut(h).attachments.push(item.source);
                }
                _ => {
                    zones::move_to(gs, item.source, Zone::Graveyard, None);
                }
            }
        }
        let name = gs.name_of(item.source);
        gs.tracef(|| format!("{name} enters the battlefield"));
        return;
    }

    // Instant or sorcery.
    if !fizzles(gs, &item) {
        let effect = {
            let cf = gs.db.compiled_face(item.card, face);
            cf.spell.as_ref().map(|sa| sa.effect.clone())
        };
        if let Some(effect) = effect {
            let mut ctx = Ctx {
                source: item.source,
                controller: item.controller,
                targets: item.targets.clone(),
                x: item.x,
                subject: item.trigger_subject,
                tplayer: item.trigger_player,
            };
            match &effect {
                Effect::Modal { modes, .. } => {
                    // Slice flattened targets per chosen mode, in order.
                    let mut offset = 0usize;
                    let all: SmallVec<[Target; 2]> = ctx.targets.clone();
                    for &mi in &item.modes {
                        if let Some(mode) = modes.get(mi as usize) {
                            let want: usize =
                                mode.targets.iter().map(|s| s.count.max() as usize).sum();
                            let end = (offset + want).min(all.len());
                            ctx.targets = all[offset..end].iter().copied().collect();
                            offset = end;
                            let eff = mode.effect.clone();
                            exec(gs, agents, &mut ctx, &eff);
                        }
                    }
                }
                _ => exec(gs, agents, &mut ctx, &effect),
            }
        }
    }
    let dest = if item.exile_on_resolve { Zone::Exile } else { Zone::Graveyard };
    zones::move_to(gs, item.source, dest, None);
}

// Selection helpers.

fn players_of(gs: &GameState, ctx: &Ctx, sel: &PlayerSel) -> SmallVec<[Seat; 4]> {
    let mut out = SmallVec::new();
    match sel {
        PlayerSel::You => out.push(ctx.controller),
        PlayerSel::EachOpponent => out.extend(gs.opponents_of(ctx.controller)),
        PlayerSel::EachPlayer => {
            out.extend(gs.seats().filter(|&s| gs.player(s).lost.is_none()))
        }
        PlayerSel::Target(i) => {
            if let Some(Target::Player(s)) = ctx.targets.get(*i as usize) {
                if gs.player(*s).lost.is_none() {
                    out.push(*s);
                }
            }
        }
        PlayerSel::TriggerPlayer => {
            if let Some(p) = ctx.tplayer {
                if gs.player(p).lost.is_none() {
                    out.push(p);
                }
            }
        }
        PlayerSel::ControllerOf(inner) => {
            for id in objs_of(gs, ctx, inner) {
                let c = gs.obj(id).controller;
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        }
    }
    out
}

fn objs_of(gs: &GameState, ctx: &Ctx, sel: &ObjSel) -> SmallVec<[ObjectId; 4]> {
    let mut out = SmallVec::new();
    match sel {
        ObjSel::Target(i) => {
            if let Some(&t) = ctx.targets.get(*i as usize) {
                if let Target::Obj(id, inc) = t {
                    if gs.obj(id).incarnation == inc {
                        out.push(id);
                    }
                }
            }
        }
        ObjSel::This => out.push(ctx.source),
        ObjSel::All(f) => {
            for p in &gs.players {
                for &id in &p.battlefield {
                    if obj_matches(gs, f, id, ctx.controller, Some(ctx.source)) {
                        out.push(id);
                    }
                }
            }
        }
        ObjSel::TriggerSubject => {
            if let Some(s) = ctx.subject {
                out.push(s);
            }
        }
        ObjSel::AttachedHost => {
            if let Some(h) = gs.obj(ctx.source).attached_to {
                out.push(h);
            }
        }
    }
    out
}

pub fn eval_value(gs: &GameState, ctx: &Ctx, v: &ValueExpr) -> i32 {
    match v {
        ValueExpr::Fixed(n) => *n,
        ValueExpr::X => ctx.x as i32,
        ValueExpr::Count(f) => {
            let mut n = 0;
            for p in &gs.players {
                for &id in &p.battlefield {
                    if obj_matches(gs, f, id, ctx.controller, Some(ctx.source)) {
                        n += 1;
                    }
                }
            }
            n
        }
        ValueExpr::CardsInHand(sel) => players_of(gs, ctx, sel)
            .first()
            .map(|&s| gs.player(s).hand.len() as i32)
            .unwrap_or(0),
        ValueExpr::LifeTotal(sel) => players_of(gs, ctx, sel)
            .first()
            .map(|&s| gs.player(s).life)
            .unwrap_or(0),
        ValueExpr::CountersOnThis(kind) => gs.obj(ctx.source).counter_count(*kind) as i32,
        ValueExpr::Custom(_) => 0,
    }
}

fn eval_condition(gs: &GameState, ctx: &Ctx, c: &Condition) -> bool {
    match c {
        Condition::Compare(a, cmp, b) => {
            cmp.eval(eval_value(gs, ctx, a) as i64, eval_value(gs, ctx, b) as i64)
        }
        Condition::YourTurn => gs.active == ctx.controller,
    }
}

/// How many doubling replacements a player controls for a given kind.
fn doubling_factor(gs: &GameState, seat: Seat, kind: &ReplKind) -> u32 {
    let mut n = 0;
    for &id in &gs.player(seat).battlefield {
        let o = gs.obj(id);
        if o.token.is_some() {
            continue;
        }
        let cf = gs.db.compiled_face(o.card, o.face);
        for r in &cf.replacements {
            let scope_ok = matches!(r.scope, ReplScope::Yours(_) | ReplScope::All(_));
            if scope_ok && std::mem::discriminant(&r.kind) == std::mem::discriminant(kind) {
                n += 1;
            }
        }
    }
    1u32 << n.min(4)
}

pub fn exec(gs: &mut GameState, agents: &mut Agents, ctx: &mut Ctx, effect: &Effect) {
    match effect {
        Effect::Seq(list) => {
            for e in list {
                if gs.over.is_some() {
                    return;
                }
                exec(gs, agents, ctx, e);
            }
        }
        Effect::DealDamage { n, to } => {
            let amount = eval_value(gs, ctx, n);
            let mut hits: SmallVec<[DamageTarget; 4]> = SmallVec::new();
            match to {
                Recipient::Target(i) => {
                    if let Some(&t) = ctx.targets.get(*i as usize) {
                        if target_alive(gs, t) {
                            hits.push(match t {
                                Target::Obj(id, _) => DamageTarget::Obj(id),
                                Target::Player(s) => DamageTarget::Player(s),
                            });
                        }
                    }
                }
                Recipient::Object(sel) => {
                    hits.extend(objs_of(gs, ctx, sel).into_iter().map(DamageTarget::Obj))
                }
                Recipient::Player(sel) => {
                    hits.extend(players_of(gs, ctx, sel).into_iter().map(DamageTarget::Player))
                }
            }
            for h in hits {
                apply_damage(gs, ctx.source, h, amount, false);
            }
        }
        Effect::Draw { who, n } => {
            let n = eval_value(gs, ctx, n).max(0) as u32;
            for s in players_of(gs, ctx, who) {
                zones::draw_cards(gs, s, n);
            }
        }
        Effect::Discard { who, n, random } => {
            let n = eval_value(gs, ctx, n).max(0) as usize;
            for s in players_of(gs, ctx, who) {
                for _ in 0..n {
                    let hand = gs.player(s).hand.clone();
                    if hand.is_empty() {
                        break;
                    }
                    let pick = if *random {
                        use rand::Rng;
                        hand[gs.rng.gen_range(0..hand.len())]
                    } else {
                        let view = View { gs, seat: s };
                        agents
                            .get(s)
                            .choose_discard(&view, &hand, 1)
                            .first()
                            .copied()
                            .unwrap_or(hand[0])
                    };
                    zones::move_to(gs, pick, Zone::Graveyard, None);
                }
            }
        }
        Effect::Destroy { what } => {
            for id in objs_of(gs, ctx, what) {
                crate::sba::destroy(gs, id);
            }
        }
        Effect::Exile { what } => {
            for id in objs_of(gs, ctx, what) {
                zones::move_to(gs, id, Zone::Exile, None);
            }
        }
        Effect::ExileUntilSourceLeaves { what } => {
            let src = ctx.source;
            for id in objs_of(gs, ctx, what) {
                zones::move_to(gs, id, Zone::Exile, None);
                if gs.obj(src).zone == Zone::Battlefield && gs.obj(id).zone == Zone::Exile {
                    gs.obj_mut(src).exiling.push(id);
                }
            }
        }
        Effect::Bounce { what } => {
            for id in objs_of(gs, ctx, what) {
                zones::move_to(gs, id, Zone::Hand, None);
            }
        }
        Effect::PutOnTopOfLibrary { what } => {
            for id in objs_of(gs, ctx, what) {
                let owner = gs.obj(id).owner;
                zones::move_to(gs, id, Zone::Library, None);
                // move_to pushed it at the end, which is the top. Nothing
                // else to do, but keep the intent explicit.
                let _ = owner;
            }
        }
        Effect::Reanimate { what, controller, tapped } => {
            let ctrl = players_of(gs, ctx, controller).first().copied().unwrap_or(ctx.controller);
            for id in objs_of(gs, ctx, what) {
                if gs.obj(id).zone == Zone::Graveyard {
                    zones::move_to(gs, id, Zone::Battlefield, Some(ctrl));
                    if *tapped {
                        gs.obj_mut(id).tapped = true;
                    }
                }
            }
        }
        Effect::CreateToken { proto, n, tapped, attacking } => {
            let base = eval_value(gs, ctx, n).max(0) as u32;
            let mult = doubling_factor(gs, ctx.controller, &ReplKind::TokensDoubled);
            for _ in 0..base * mult {
                zones::create_token(gs, proto.clone(), ctx.controller, *tapped, *attacking);
            }
        }
        Effect::ModifyPt { what, p, t, dur } => {
            let p = eval_value(gs, ctx, p);
            let t = eval_value(gs, ctx, t);
            for id in objs_of(gs, ctx, what) {
                let ts = gs.next_ts();
                let inc = gs.obj(id).incarnation;
                gs.floating.push(FloatingEffect {
                    target: id,
                    target_incarnation: inc,
                    until: *dur,
                    p,
                    t,
                    set_pt: None,
                    add_kw: Default::default(),
                    remove_kw: Default::default(),
                    control_to: None,
                    ts,
                });
            }
            crate::layers::recompute_chars(gs);
        }
        Effect::SetPt { what, p, t, dur } => {
            for id in objs_of(gs, ctx, what) {
                let ts = gs.next_ts();
                let inc = gs.obj(id).incarnation;
                gs.floating.push(FloatingEffect {
                    target: id,
                    target_incarnation: inc,
                    until: *dur,
                    p: 0,
                    t: 0,
                    set_pt: Some((*p, *t)),
                    add_kw: Default::default(),
                    remove_kw: Default::default(),
                    control_to: None,
                    ts,
                });
            }
            crate::layers::recompute_chars(gs);
        }
        Effect::GrantKeywords { what, kw, dur } => {
            for id in objs_of(gs, ctx, what) {
                let ts = gs.next_ts();
                let inc = gs.obj(id).incarnation;
                gs.floating.push(FloatingEffect {
                    target: id,
                    target_incarnation: inc,
                    until: *dur,
                    p: 0,
                    t: 0,
                    set_pt: None,
                    add_kw: *kw,
                    remove_kw: Default::default(),
                    control_to: None,
                    ts,
                });
            }
            crate::layers::recompute_chars(gs);
        }
        Effect::RemoveKeywords { what, kw, dur } => {
            for id in objs_of(gs, ctx, what) {
                let ts = gs.next_ts();
                let inc = gs.obj(id).incarnation;
                gs.floating.push(FloatingEffect {
                    target: id,
                    target_incarnation: inc,
                    until: *dur,
                    p: 0,
                    t: 0,
                    set_pt: None,
                    add_kw: Default::default(),
                    remove_kw: *kw,
                    control_to: None,
                    ts,
                });
            }
            crate::layers::recompute_chars(gs);
        }
        Effect::PutCounters { what, kind, n } => {
            let base = eval_value(gs, ctx, n).max(0) as i16;
            let mult = if *kind == CounterKind::PlusOne {
                doubling_factor(gs, ctx.controller, &ReplKind::CountersDoubled) as i16
            } else {
                1
            };
            for id in objs_of(gs, ctx, what) {
                gs.obj_mut(id).add_counters(*kind, base * mult);
            }
            crate::layers::recompute_chars(gs);
        }
        Effect::RemoveCounters { what, kind, n } => {
            let n = eval_value(gs, ctx, n).max(0) as i16;
            for id in objs_of(gs, ctx, what) {
                gs.obj_mut(id).add_counters(*kind, -n);
            }
            crate::layers::recompute_chars(gs);
        }
        Effect::CounterSpell { target, unless_pay } => {
            if let Some(Target::Obj(id, inc)) = ctx.targets.get(*target as usize).copied() {
                let idx = gs
                    .stack
                    .iter()
                    .position(|i| i.source == id && i.source_incarnation == inc);
                if let Some(idx) = idx {
                    let owner_seat = gs.stack[idx].controller;
                    if let Some(cost) = unless_pay {
                        if let Some(plan) = crate::mana_pay::solve(gs, owner_seat, cost, 0) {
                            let view = View { gs, seat: owner_seat };
                            if agents.get(owner_seat).yes_no(&view, YesNo::CounterUnlessPay) {
                                crate::mana_pay::execute(gs, owner_seat, &plan, cost, 0);
                                return;
                            }
                        }
                    }
                    gs.stack.remove(idx);
                    zones::move_to(gs, id, Zone::Graveyard, None);
                    let name = gs.name_of(id);
                    gs.tracef(|| format!("{name} is countered"));
                }
            }
        }
        Effect::AddMana { produce } => {
            crate::mana_pay::add_production(gs, ctx.controller, produce, None);
        }
        Effect::Mill { who, n } => {
            let n = eval_value(gs, ctx, n).max(0) as usize;
            for s in players_of(gs, ctx, who) {
                for _ in 0..n {
                    match gs.player_mut(s).library.pop() {
                        Some(id) => zones::move_to(gs, id, Zone::Graveyard, None),
                        None => break,
                    }
                }
            }
        }
        Effect::SearchLibrary { who, filter, dest, count, enters_tapped } => {
            for s in players_of(gs, ctx, who) {
                let cands: Vec<ObjectId> = gs
                    .player(s)
                    .library
                    .iter()
                    .copied()
                    .filter(|&id| obj_matches(gs, filter, id, s, Some(ctx.source)))
                    .collect();
                let view = View { gs, seat: s };
                let picked = agents.get(s).search_pick(&view, &cands, *count as usize);
                let picked: Vec<ObjectId> =
                    picked.into_iter().filter(|p| cands.contains(p)).take(*count as usize).collect();
                for id in &picked {
                    gs.player_mut(s).library.retain(|x| x != id);
                }
                zones::shuffle_library(gs, s);
                for id in picked {
                    match dest {
                        SearchDest::Hand => {
                            let ts = gs.next_ts();
                            let o = gs.obj_mut(id);
                            o.zone = Zone::Hand;
                            o.incarnation += 1;
                            o.ts = ts;
                            gs.player_mut(s).hand.push(id);
                        }
                        SearchDest::Battlefield => {
                            let o = gs.obj_mut(id);
                            o.zone = Zone::Limbo;
                            zones::move_to(gs, id, Zone::Battlefield, Some(s));
                            if *enters_tapped {
                                gs.obj_mut(id).tapped = true;
                            }
                        }
                        SearchDest::Graveyard => {
                            let o = gs.obj_mut(id);
                            o.zone = Zone::Limbo;
                            zones::move_to(gs, id, Zone::Graveyard, None);
                        }
                        SearchDest::TopOfLibrary => {
                            gs.player_mut(s).library.push(id);
                        }
                    }
                }
            }
        }
        Effect::Shuffle { who } => {
            for s in players_of(gs, ctx, who) {
                zones::shuffle_library(gs, s);
            }
        }
        Effect::GainLife { who, n } => {
            let n = eval_value(gs, ctx, n);
            for s in players_of(gs, ctx, who) {
                gain_life(gs, s, n);
            }
        }
        Effect::LoseLife { who, n } => {
            let n = eval_value(gs, ctx, n);
            for s in players_of(gs, ctx, who) {
                gs.player_mut(s).life -= n;
            }
        }
        Effect::Fight { a, b } => {
            let oa = objs_of(gs, ctx, a).first().copied();
            let ob = objs_of(gs, ctx, b).first().copied();
            if let (Some(oa), Some(ob)) = (oa, ob) {
                let pa = gs.obj(oa).chars.power;
                let pb = gs.obj(ob).chars.power;
                apply_damage(gs, oa, DamageTarget::Obj(ob), pa, false);
                apply_damage(gs, ob, DamageTarget::Obj(oa), pb, false);
            }
        }
        Effect::Sacrifice { who, filter, n } => {
            let n = eval_value(gs, ctx, n).max(0) as usize;
            for s in players_of(gs, ctx, who) {
                let cands: Vec<ObjectId> = gs
                    .player(s)
                    .battlefield
                    .iter()
                    .copied()
                    .filter(|&id| obj_matches(gs, filter, id, s, Some(ctx.source)))
                    .collect();
                if cands.is_empty() {
                    continue;
                }
                let view = View { gs, seat: s };
                let picked = agents.get(s).choose_sacrifice(&view, &cands, n);
                for p in picked.into_iter().filter(|p| cands.contains(p)).take(n) {
                    crate::sba::die(gs, p);
                }
            }
        }
        Effect::TapObjects { what, tap } => {
            for id in objs_of(gs, ctx, what) {
                gs.obj_mut(id).tapped = *tap;
            }
        }
        Effect::Scry { who, n } => {
            let n = eval_value(gs, ctx, n).max(0) as usize;
            for s in players_of(gs, ctx, who) {
                let mut looked = Vec::new();
                for _ in 0..n {
                    match gs.player_mut(s).library.pop() {
                        Some(id) => looked.push(id),
                        None => break,
                    }
                }
                let view = View { gs, seat: s };
                let bottoms = agents.get(s).scry_bottom(&view, &looked);
                let (bot, top): (Vec<ObjectId>, Vec<ObjectId>) =
                    looked.into_iter().partition(|id| bottoms.contains(id));
                for id in top.into_iter().rev() {
                    gs.player_mut(s).library.push(id);
                }
                for id in bot {
                    gs.player_mut(s).library.insert(0, id);
                }
            }
        }
        Effect::Surveil { who, n } => {
            let n = eval_value(gs, ctx, n).max(0) as usize;
            for s in players_of(gs, ctx, who) {
                let mut looked = Vec::new();
                for _ in 0..n {
                    match gs.player_mut(s).library.pop() {
                        Some(id) => looked.push(id),
                        None => break,
                    }
                }
                let view = View { gs, seat: s };
                let to_yard = agents.get(s).scry_bottom(&view, &looked);
                let (yard, top): (Vec<ObjectId>, Vec<ObjectId>) =
                    looked.into_iter().partition(|id| to_yard.contains(id));
                for id in top.into_iter().rev() {
                    gs.player_mut(s).library.push(id);
                }
                for id in yard {
                    zones::move_to(gs, id, Zone::Graveyard, None);
                }
            }
        }
        Effect::Modal { .. } => {
            // Reached only when a modal effect is nested; top-level modal
            // spells are unpacked in resolve_spell.
        }
        Effect::If { cond, then, otherwise } => {
            if eval_condition(gs, ctx, cond) {
                exec(gs, agents, ctx, then);
            } else if let Some(e) = otherwise {
                exec(gs, agents, ctx, e);
            }
        }
        Effect::GainControl { what, dur } => {
            for id in objs_of(gs, ctx, what) {
                let prev = gs.obj(id).controller;
                if prev == ctx.controller {
                    continue;
                }
                zones::change_control(gs, id, ctx.controller);
                if *dur == Duration::EndOfTurn {
                    let ts = gs.next_ts();
                    let inc = gs.obj(id).incarnation;
                    gs.floating.push(FloatingEffect {
                        target: id,
                        target_incarnation: inc,
                        until: Duration::EndOfTurn,
                        p: 0,
                        t: 0,
                        set_pt: None,
                        add_kw: Default::default(),
                        remove_kw: Default::default(),
                        control_to: Some(prev),
                        ts,
                    });
                }
            }
        }
        Effect::Transform => {
            let id = ctx.source;
            let faces = gs.db.get(gs.obj(id).card).oracle.faces.len();
            if faces >= 2 && gs.obj(id).zone == Zone::Battlefield {
                let cur = gs.obj(id).face;
                gs.obj_mut(id).face = if cur == 0 { 1 } else { 0 };
                crate::layers::recompute_chars(gs);
            }
        }
        Effect::Attach { target } => {
            let src = ctx.source;
            if gs.obj(src).zone != Zone::Battlefield {
                return;
            }
            if let Some(Target::Obj(host, inc)) = ctx.targets.get(*target as usize).copied() {
                if gs.obj(host).incarnation != inc || gs.obj(host).zone != Zone::Battlefield {
                    return;
                }
                if let Some(old) = gs.obj(src).attached_to {
                    gs.obj_mut(old).attachments.retain(|x| *x != src);
                }
                gs.obj_mut(src).attached_to = Some(host);
                gs.obj_mut(host).attachments.push(src);
                crate::layers::recompute_chars(gs);
            }
        }
        Effect::Custom(_) | Effect::Noop => {}
    }
}
