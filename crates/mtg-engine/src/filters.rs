//! ObjFilter and SpellFilter evaluation against live objects.

use mtg_ir::{ObjFilter, SpellFilter, Whose};

use crate::state::{GameState, ObjectId, Seat, Zone};

pub fn whose_matches(whose: Whose, perspective: Seat, owner: Seat) -> bool {
    match whose {
        Whose::Any => true,
        Whose::You => owner == perspective,
        Whose::Opponents => owner != perspective,
    }
}

/// Does an object match a filter, from the perspective of a controller
/// (usually the filter source's controller)? State constraints (tapped,
/// attacking) are only meaningful on the battlefield and are treated as
/// unsatisfied elsewhere.
pub fn obj_matches(
    gs: &GameState,
    filter: &ObjFilter,
    id: ObjectId,
    perspective: Seat,
    source: Option<ObjectId>,
) -> bool {
    let o = gs.obj(id);
    if filter.other_than_self && Some(id) == source {
        return false;
    }
    let ch = &o.chars;
    if !filter.types.is_empty() && !ch.types.intersects(filter.types) {
        return false;
    }
    if ch.types.intersects(filter.not_types) {
        return false;
    }
    if !filter.supertypes.is_empty() && !ch.supertypes.intersects(filter.supertypes) {
        return false;
    }
    if ch.supertypes.intersects(filter.not_supertypes) {
        return false;
    }
    if !filter.colors_any.is_empty() && !ch.colors.intersects(filter.colors_any) {
        return false;
    }
    if ch.colors.intersects(filter.not_colors) {
        return false;
    }
    if !whose_matches(filter.controller, perspective, o.controller) {
        return false;
    }
    if !filter.with_keywords.is_empty() && !ch.keywords.contains(filter.with_keywords) {
        return false;
    }
    if ch.keywords.intersects(filter.without_keywords) {
        return false;
    }
    if let Some(t) = filter.tapped {
        if o.zone != Zone::Battlefield || o.tapped != t {
            return false;
        }
    }
    if let Some(a) = filter.attacking {
        let is = o.flags.contains(crate::state::ObjFlags::ATTACKING);
        if o.zone != Zone::Battlefield || is != a {
            return false;
        }
    }
    if let Some(b) = filter.blocking {
        let is = o.flags.contains(crate::state::ObjFlags::BLOCKING);
        if o.zone != Zone::Battlefield || is != b {
            return false;
        }
    }
    if filter.attacking_or_blocking {
        let f = o.flags;
        if o.zone != Zone::Battlefield
            || !(f.contains(crate::state::ObjFlags::ATTACKING)
                || f.contains(crate::state::ObjFlags::BLOCKING))
        {
            return false;
        }
    }
    if let Some(t) = filter.is_token {
        if o.token.is_some() != t {
            return false;
        }
    }
    if !filter.subtypes_any.is_empty() {
        let has = filter.subtypes_any.iter().any(|want| {
            if crate::layers::has_subtype(gs, id, want) {
                return true;
            }
            // Changelings hold every creature type.
            ch.keywords.contains(mtg_ir::KeywordSet::CHANGELING)
        });
        if !has {
            return false;
        }
    }
    if let Some((cmp, n)) = filter.power {
        if !cmp.eval(ch.power as i64, n as i64) {
            return false;
        }
    }
    if let Some((cmp, n)) = filter.toughness {
        if !cmp.eval(ch.toughness as i64, n as i64) {
            return false;
        }
    }
    if let Some((cmp, n)) = filter.mana_value {
        let mv = mana_value_of(gs, id);
        if !cmp.eval(mv as i64, n as i64) {
            return false;
        }
    }
    if let Some(name) = &filter.name_is {
        let obj_name = match &o.token {
            Some(t) => t.name.to_string(),
            None => gs.db.face(o.card, o.face).name.to_string(),
        };
        if !obj_name.eq_ignore_ascii_case(name) {
            return false;
        }
    }
    true
}

pub fn mana_value_of(gs: &GameState, id: ObjectId) -> u32 {
    let o = gs.obj(id);
    if o.token.is_some() {
        return 0;
    }
    gs.db.get(o.card).oracle.cmc as u32
}

/// Does a spell on the stack match a spell filter?
pub fn spell_matches(gs: &GameState, filter: &SpellFilter, spell_obj: ObjectId) -> bool {
    let o = gs.obj(spell_obj);
    let face = gs.db.face(o.card, o.face);
    if !filter.types.is_empty() && !face.types.intersects(filter.types) {
        return false;
    }
    if face.types.intersects(filter.not_types) {
        return false;
    }
    if let Some((cmp, n)) = filter.mana_value {
        if !cmp.eval(mana_value_of(gs, spell_obj) as i64, n as i64) {
            return false;
        }
    }
    if !filter.colors_any.is_empty() {
        let colors = mtg_ir::ManaCost::parse(&face.mana_cost)
            .map(|c| c.colors())
            .unwrap_or_default();
        if !colors.intersects(filter.colors_any) {
            return false;
        }
    }
    true
}
