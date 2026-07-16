//! Characteristics recompute: the pragmatic layer pipeline.
//!
//! Order per object: base (printed or token) -> floating set-P/T (7b) ->
//! static buffs (7c) -> counters (7c) -> floating P/T mods (7d-ish).
//! Keywords: base -> static grants (6) -> floating grants/removes by
//! timestamp. Filters for statics are matched against base characteristics,
//! which sidesteps dependency loops at the cost of rare inaccuracy.

use mtg_ir::{CounterKind, Duration, StaticAbility};

use crate::filters::obj_matches;
use crate::state::{Characteristics, GameState, ObjectId, Zone};

/// True when the object currently has the given subtype. Tokens carry their
/// own list; card objects read the active oracle face.
pub fn has_subtype(gs: &GameState, id: ObjectId, want: &str) -> bool {
    let o = gs.obj(id);
    match &o.token {
        Some(t) => t.subtypes.iter().any(|s| s.as_ref() == want),
        None => {
            let face = gs.db.face(o.card, o.face);
            face.subtypes.iter().any(|s| s.as_ref() == want)
        }
    }
}

fn base_chars(gs: &GameState, id: ObjectId) -> Characteristics {
    let o = gs.obj(id);
    match &o.token {
        Some(t) => Characteristics {
            types: t.types,
            supertypes: Default::default(),
            power: t.power,
            toughness: t.toughness,
            keywords: t.keywords,
            colors: t.colors,
            ward: None,
            protection_from: Default::default(),
            toxic: 0,
        },
        None => {
            let face = gs.db.face(o.card, o.face);
            let cf = gs.db.compiled_face(o.card, o.face);
            Characteristics {
                types: face.types,
                supertypes: face.supertypes,
                power: face.power.unwrap_or(0),
                toughness: face.toughness.unwrap_or(0),
                keywords: cf.keywords,
                colors: face.colors,
                ward: cf.ward.clone(),
                protection_from: cf.protection_from,
                toxic: cf.toxic,
            }
        }
    }
}

/// Recompute characteristics for every battlefield object. Cheap (tens of
/// objects) and called eagerly after any mutation that could matter.
pub fn recompute_chars(gs: &mut GameState) {
    let battlefield: Vec<ObjectId> = gs
        .players
        .iter()
        .flat_map(|p| p.battlefield.iter().copied())
        .collect();

    // Base pass first so static filters see printed characteristics.
    for &id in &battlefield {
        let base = base_chars(gs, id);
        gs.obj_mut(id).chars = base;
    }

    // Collect statics from battlefield sources.
    struct Applied {
        source: ObjectId,
        controller: u8,
        ts: u64,
        ability: StaticAbility,
    }
    let mut statics: Vec<Applied> = Vec::new();
    for &id in &battlefield {
        let o = gs.obj(id);
        let cf = gs.db.compiled_face(o.card, o.face);
        for st in &cf.statics {
            statics.push(Applied {
                source: id,
                controller: o.controller,
                ts: o.ts,
                ability: st.clone(),
            });
        }
    }
    statics.sort_by_key(|a| a.ts);

    // Layer 6: keyword grants from statics.
    for a in &statics {
        match &a.ability {
            StaticAbility::GrantKeywords { affects, kw } => {
                for &id in &battlefield {
                    if (affects.include_self || id != a.source)
                        && obj_matches(gs, &affects.filter, id, a.controller, Some(a.source))
                    {
                        gs.obj_mut(id).chars.keywords |= *kw;
                    }
                }
            }
            StaticAbility::AttachedBuff { kw, .. } if !kw.is_empty() => {
                if let Some(host) = gs.obj(a.source).attached_to {
                    if gs.obj(host).zone == Zone::Battlefield {
                        gs.obj_mut(host).chars.keywords |= *kw;
                    }
                }
            }
            _ => {}
        }
    }

    // Floating keyword grants and removes, in timestamp order.
    let mut floats: Vec<usize> = (0..gs.floating.len()).collect();
    floats.sort_by_key(|&i| gs.floating[i].ts);
    for &i in &floats {
        let f = gs.floating[i].clone();
        let alive = gs
            .objects
            .get(f.target.0 as usize)
            .map(|o| o.incarnation == f.target_incarnation && o.zone == Zone::Battlefield)
            .unwrap_or(false);
        if !alive {
            continue;
        }
        let ch = &mut gs.obj_mut(f.target).chars;
        ch.keywords |= f.add_kw;
        ch.keywords &= !f.remove_kw;
        if let Some((p, t)) = f.set_pt {
            ch.power = p;
            ch.toughness = t;
        }
    }

    // Layer 7c: static P/T buffs.
    for a in &statics {
        match &a.ability {
            StaticAbility::PtBuff { affects, p, t } => {
                for &id in &battlefield {
                    if (affects.include_self || id != a.source)
                        && obj_matches(gs, &affects.filter, id, a.controller, Some(a.source))
                    {
                        let ch = &mut gs.obj_mut(id).chars;
                        ch.power += p;
                        ch.toughness += t;
                    }
                }
            }
            StaticAbility::AttachedBuff { p, t, .. } if *p != 0 || *t != 0 => {
                if let Some(host) = gs.obj(a.source).attached_to {
                    if gs.obj(host).zone == Zone::Battlefield {
                        let ch = &mut gs.obj_mut(host).chars;
                        ch.power += p;
                        ch.toughness += t;
                    }
                }
            }
            _ => {}
        }
    }

    // Counters, then floating P/T deltas.
    for &id in &battlefield {
        let plus = gs.obj(id).counter_count(CounterKind::PlusOne) as i32;
        let minus = gs.obj(id).counter_count(CounterKind::MinusOne) as i32;
        let ch = &mut gs.obj_mut(id).chars;
        ch.power += plus - minus;
        ch.toughness += plus - minus;
    }
    for &i in &floats {
        let f = gs.floating[i].clone();
        let alive = gs
            .objects
            .get(f.target.0 as usize)
            .map(|o| o.incarnation == f.target_incarnation && o.zone == Zone::Battlefield)
            .unwrap_or(false);
        if !alive || (f.p == 0 && f.t == 0) {
            continue;
        }
        let ch = &mut gs.obj_mut(f.target).chars;
        ch.power += f.p;
        ch.toughness += f.t;
    }
}

/// Drop floating effects that expire at end of turn.
pub fn expire_end_of_turn(gs: &mut GameState) {
    gs.floating.retain(|f| f.until != Duration::EndOfTurn);
}
