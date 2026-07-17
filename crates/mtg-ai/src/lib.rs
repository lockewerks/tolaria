//! Decision agents: the GreedyAgent heuristic baseline. No search, no
//! rollouts; every decision is a cheap scoring pass so whole games stay in
//! the low-millisecond range.

use mtg_engine::actions::LegalAction;
use mtg_engine::combat::Defender;
use mtg_engine::state::{ObjectId, Target, Zone};
use mtg_engine::{Agent, View};
use mtg_ir::{CardTypes, Effect, KeywordSet, Limit, LimitCategory::Pilot, TargetSpec, Whose};
use smallvec::SmallVec;

pub struct GreedyAgent;

/// Where the greedy pilot is known to play worse than a person would.
pub const LIMITS: &[Limit] = &[
    Limit {
        id: "pilot.no-search",
        category: Pilot,
        rule_ref: "-",
        summary: "every decision is a one-ply scoring pass with no lookahead, sequencing plan, or combo execution",
        impact: "decks that reward planned lines (combo, control) win less than a skilled pilot would take",
    },
    Limit {
        id: "pilot.sorcery-speed",
        category: Pilot,
        rule_ref: "-",
        summary: "the pilot casts on its own main phases and does not hold instants for the opponent's turn",
        impact: "reactive decks that want to hold up removal or counters play too proactively",
    },
    Limit {
        id: "pilot.fidelity-heuristic",
        category: Pilot,
        rule_ref: "-",
        summary: "the low-fidelity flag is a creature count (under 10), not real archetype analysis",
        impact: "creature-based combo is not flagged; a fair creature-light deck is flagged the same as degenerate combo",
    },
];

fn is_land(v: &View, id: ObjectId) -> bool {
    let o = v.obj(id);
    let card = v.card_of(id);
    card.oracle.faces[(o.face as usize).min(card.oracle.faces.len() - 1)]
        .types
        .contains(CardTypes::LAND)
}

fn mana_value(v: &View, id: ObjectId) -> i32 {
    v.card_of(id).oracle.cmc as i32
}

/// How scary a battlefield creature is.
fn threat(v: &View, id: ObjectId) -> i32 {
    let o = v.obj(id);
    let ch = &o.chars;
    let mut score = ch.power * 3 + ch.toughness;
    let kw = ch.keywords;
    for (flag, bonus) in [
        (KeywordSet::FLYING, 3),
        (KeywordSet::TRAMPLE, 2),
        (KeywordSet::LIFELINK, 2),
        (KeywordSet::DEATHTOUCH, 2),
        (KeywordSet::DOUBLE_STRIKE, ch.power * 3),
        (KeywordSet::FIRST_STRIKE, 1),
        (KeywordSet::MENACE, 1),
        (KeywordSet::INDESTRUCTIBLE, 2),
        (KeywordSet::INFECT, ch.power * 2),
        (KeywordSet::UNBLOCKABLE, 3),
    ] {
        if kw.contains(flag) {
            score += bonus;
        }
    }
    if o.chars.types.contains(CardTypes::PLANESWALKER) {
        score += 8;
    }
    score
}

/// Rough card quality for cast ordering.
fn spell_value(v: &View, id: ObjectId, face: u8) -> i32 {
    let card = v.card_of(id);
    let cf = &card.compiled.faces[(face as usize).min(card.compiled.faces.len() - 1)];
    let of = &card.oracle.faces[(face as usize).min(card.oracle.faces.len() - 1)];
    let mut score = 0;
    if of.types.contains(CardTypes::CREATURE) {
        score += of.power.unwrap_or(0) * 3 + of.toughness.unwrap_or(0) * 2;
        score += (cf.keywords.bits().count_ones() as i32) * 2;
    }
    if of.types.contains(CardTypes::PLANESWALKER) {
        score += 14;
    }
    if let Some(sa) = &cf.spell {
        score += effect_value(&sa.effect);
    }
    score += (cf.statics.len() as i32) * 6;
    score += (cf.triggered.len() as i32) * 4;
    score += (cf.activated.len() as i32) * 3;
    score
}

fn effect_value(e: &Effect) -> i32 {
    match e {
        Effect::Seq(list) => list.iter().map(effect_value).sum(),
        Effect::DealDamage { .. } => 9,
        Effect::Destroy { .. } | Effect::Exile { .. } | Effect::ExileUntilSourceLeaves { .. } => 11,
        Effect::CounterSpell { .. } => 6,
        Effect::Draw { .. } => 8,
        Effect::CreateToken { .. } => 8,
        Effect::ModifyPt { .. } | Effect::GrantKeywords { .. } => 4,
        Effect::PutCounters { .. } => 5,
        Effect::SearchLibrary { .. } => 7,
        Effect::Reanimate { .. } => 10,
        Effect::GainControl { .. } => 12,
        Effect::Discard { .. } => 6,
        Effect::Mill { .. } => 3,
        Effect::GainLife { .. } => 2,
        Effect::LoseLife { .. } => 4,
        Effect::Fight { .. } => 7,
        Effect::Bounce { .. } => 5,
        Effect::TapObjects { .. } => 3,
        Effect::Scry { .. } | Effect::Surveil { .. } => 3,
        Effect::Modal { modes, .. } => modes.iter().map(|m| effect_value(&m.effect)).max().unwrap_or(0),
        _ => 1,
    }
}

/// Does this spell want an enemy target that exists right now?
fn has_worthwhile_enemy_target(v: &View, spec: &TargetSpec) -> bool {
    if let mtg_ir::TargetWhat::Permanent(f) = &spec.what {
        if f.controller == Whose::You {
            return true;
        }
        for opp in v.opponents() {
            for &id in v.battlefield(opp) {
                if v.obj(id).is_creature() && threat(v, id) >= 6 {
                    return true;
                }
            }
        }
        return false;
    }
    true
}

impl Agent for GreedyAgent {
    fn mulligan(&mut self, v: &View, hand: &[ObjectId], taken: u8) -> bool {
        if taken >= 3 {
            return false;
        }
        let lands = hand.iter().filter(|&&id| is_land(v, id)).count();
        let hand_size = hand.len();
        let playable = hand
            .iter()
            .any(|&id| !is_land(v, id) && mana_value(v, id) <= 3);
        match hand_size {
            7 => lands < 2 || lands > 5 || !playable,
            _ => lands < 1 || lands as i32 > hand_size as i32 - 1,
        }
    }

    fn choose_bottom(&mut self, v: &View, hand: &[ObjectId], n: usize) -> Vec<ObjectId> {
        let lands = hand.iter().filter(|&&id| is_land(v, id)).count();
        let mut ranked: Vec<ObjectId> = hand.to_vec();
        // Bottom the most expensive spells; excess lands first when flooded.
        ranked.sort_by_key(|&id| {
            let land = is_land(v, id);
            let flood_penalty = if land && lands > 4 { 100 } else { 0 };
            let mv = mana_value(v, id);
            -(flood_penalty + if land { -50 } else { mv })
        });
        ranked.into_iter().take(n).collect()
    }

    fn choose_action(&mut self, v: &View, legal: &[LegalAction]) -> usize {
        let mut best = 0usize;
        let mut best_score = 0i32;
        for (i, action) in legal.iter().enumerate() {
            let score = match action {
                LegalAction::Pass => 0,
                LegalAction::PlayLand { .. } => 1000,
                LegalAction::Cast { card, face, .. } => {
                    let value = spell_value(v, *card, *face);
                    if value <= 0 {
                        continue;
                    }
                    // Removal wants a target worth killing.
                    let cf = &v.card_of(*card).compiled.faces
                        [(*face as usize).min(v.card_of(*card).compiled.faces.len() - 1)];
                    let mut ok = true;
                    if let Some(sa) = &cf.spell {
                        // Hard removal waits for something worth killing;
                        // damage can always go to the face.
                        if matches!(sa.effect, Effect::Destroy { .. } | Effect::Exile { .. }) {
                            ok = sa.targets.iter().all(|s| has_worthwhile_enemy_target(v, s));
                        }
                        // Counterspells held for the stack.
                        if matches!(sa.effect, Effect::CounterSpell { .. }) && v.stack().is_empty() {
                            ok = false;
                        }
                    }
                    if !ok {
                        continue;
                    }
                    100 + value + mana_value(v, *card) * 2
                }
                LegalAction::Activate { source, index, .. } => {
                    let card = v.card_of(*source);
                    let o = v.obj(*source);
                    let cf = &card.compiled.faces[(o.face as usize).min(card.compiled.faces.len() - 1)];
                    match cf.activated.get(*index as usize) {
                        Some(ab) => {
                            let val = effect_value(&ab.ability.effect);
                            if val <= 2 && ab.loyalty.is_none() {
                                continue;
                            }
                            40 + val
                        }
                        None => continue,
                    }
                }
                LegalAction::Cycle { .. } => {
                    // Cycle spare land-heavy hands late.
                    if v.lands_played() > 0 && v.my_hand().len() <= 2 {
                        30
                    } else {
                        continue;
                    }
                }
            };
            if score > best_score {
                best_score = score;
                best = i;
            }
        }
        best
    }

    fn choose_targets(
        &mut self,
        v: &View,
        spec: &TargetSpec,
        candidates: &[Target],
    ) -> SmallVec<[Target; 2]> {
        let want = spec.count.max() as usize;
        let need = spec.count.min() as usize;
        let mut scored: Vec<(i32, Target)> = candidates
            .iter()
            .map(|&t| {
                let s = match t {
                    Target::Obj(id, _) => {
                        let mine = v.obj(id).controller == v.seat;
                        let base = if v.obj(id).zone == Zone::Battlefield {
                            threat(v, id)
                        } else {
                            mana_value(v, id) * 2
                        };
                        // Aim harmful things at the scariest enemy object,
                        // helpful things at our own best object.
                        if mine {
                            if spec_is_friendly(spec) {
                                base + 50
                            } else {
                                -base
                            }
                        } else if spec_is_friendly(spec) {
                            -base
                        } else {
                            base + 20
                        }
                    }
                    Target::Player(s) => {
                        if s == v.seat {
                            if spec_is_friendly(spec) {
                                60
                            } else {
                                -100
                            }
                        } else if spec_is_friendly(spec) {
                            -50
                        } else {
                            // Face is fine, creatures usually better.
                            10 + (20 - v.life(s)).max(0)
                        }
                    }
                };
                (s, t)
            })
            .collect();
        scored.sort_by_key(|(s, _)| -*s);
        let take = scored.iter().filter(|(s, _)| *s > 0).count().clamp(need, want.max(need));
        scored.into_iter().take(take.min(candidates.len())).map(|(_, t)| t).collect()
    }

    fn declare_attackers(
        &mut self,
        v: &View,
        candidates: &[ObjectId],
        defenders: &[Defender],
    ) -> Vec<(ObjectId, Defender)> {
        // Pick the weakest defending seat: fewest untapped creatures.
        let defender = defenders
            .iter()
            .min_by_key(|d| match d {
                Defender::Player(s) => {
                    v.battlefield(*s)
                        .iter()
                        .filter(|&&b| v.obj(b).is_creature() && !v.obj(b).tapped)
                        .count() as i32
                        * 10
                        + v.life(*s) / 4
                }
                Defender::Planeswalker(_) => 5,
            })
            .copied();
        let Some(defender) = defender else { return Vec::new() };
        let def_seat = match defender {
            Defender::Player(s) => s,
            Defender::Planeswalker(id) => v.obj(id).controller,
        };
        let blockers: Vec<ObjectId> = v
            .battlefield(def_seat)
            .iter()
            .copied()
            .filter(|&b| v.obj(b).is_creature() && !v.obj(b).tapped)
            .collect();
        let total_power: i32 = candidates.iter().map(|&c| v.obj(c).chars.power).sum();
        let racing = v.life(def_seat) <= total_power;

        let mut out = Vec::new();
        for &a in candidates {
            let ach = &v.obj(a).chars;
            if ach.power <= 0 {
                continue;
            }
            // A block is bad for us when the blocker kills us and survives.
            let punished = blockers.iter().any(|&b| {
                let bch = &v.obj(b).chars;
                let can_block = mtg_engine::combat::can_block(v.gs, b, a);
                can_block
                    && bch.power >= ach.toughness
                    && bch.toughness > ach.power
                    && !ach.keywords.contains(KeywordSet::FIRST_STRIKE)
            });
            if racing || !punished {
                out.push((a, defender));
            }
        }
        out
    }

    fn declare_blockers(
        &mut self,
        v: &View,
        attackers: &[ObjectId],
        candidates: &[ObjectId],
    ) -> Vec<(ObjectId, ObjectId)> {
        let mut out: Vec<(ObjectId, ObjectId)> = Vec::new();
        let mut used: Vec<ObjectId> = Vec::new();
        let incoming: i32 = attackers.iter().map(|&a| v.obj(a).chars.power).sum();
        let lethal_threat = incoming >= v.life(v.seat);

        // Favorable blocks first: kill it and live.
        let mut sorted_attackers: Vec<ObjectId> = attackers.to_vec();
        sorted_attackers.sort_by_key(|&a| -threat(v, a));
        for &a in &sorted_attackers {
            let ach = v.obj(a).chars.clone();
            let best = candidates
                .iter()
                .filter(|&&b| !used.contains(&b) && mtg_engine::combat::can_block(v.gs, b, a))
                .map(|&b| {
                    let bch = &v.obj(b).chars;
                    let kills = bch.power >= ach.toughness
                        || bch.keywords.contains(KeywordSet::DEATHTOUCH);
                    let dies = ach.power >= bch.toughness
                        && !bch.keywords.contains(KeywordSet::INDESTRUCTIBLE);
                    let score = match (kills, dies) {
                        (true, false) => 100 + threat(v, a),
                        (true, true) => 40 + threat(v, a) - threat(v, b),
                        (false, false) => 20,
                        (false, true) => -20 + if lethal_threat { 60 } else { 0 },
                    };
                    (score, b)
                })
                .max_by_key(|(s, _)| *s);
            if let Some((score, b)) = best {
                if score > 0 {
                    out.push((b, a));
                    used.push(b);
                }
            }
        }
        out
    }

    fn order_blockers(
        &mut self,
        v: &View,
        _attacker: ObjectId,
        blockers: &[ObjectId],
    ) -> Vec<ObjectId> {
        // Kill the biggest threat we can, cheapest bodies last.
        let mut ordered = blockers.to_vec();
        ordered.sort_by_key(|&b| -(threat(v, b)));
        ordered
    }

    fn choose_discard(&mut self, v: &View, hand: &[ObjectId], n: usize) -> Vec<ObjectId> {
        let lands_in_play = v
            .battlefield(v.seat)
            .iter()
            .filter(|&&id| v.obj(id).is_land())
            .count();
        let mut ranked = hand.to_vec();
        ranked.sort_by_key(|&id| {
            let land = is_land(v, id);
            if land && lands_in_play >= 5 {
                -100
            } else if land {
                -mana_value(v, id) - 20
            } else {
                -mana_value(v, id)
            }
        });
        ranked.into_iter().take(n).collect()
    }

    fn choose_x(&mut self, _v: &View, max: u32) -> u32 {
        max
    }

    fn yes_no(&mut self, _v: &View, _prompt: mtg_engine::agent::YesNo) -> bool {
        true
    }

    fn scry_bottom(&mut self, v: &View, looked: &[ObjectId]) -> Vec<ObjectId> {
        let lands_in_play = v
            .battlefield(v.seat)
            .iter()
            .filter(|&&id| v.obj(id).is_land())
            .count();
        looked
            .iter()
            .copied()
            .filter(|&id| {
                let land = is_land(v, id);
                (land && lands_in_play >= 5) || (!land && mana_value(v, id) > 7)
            })
            .collect()
    }

    fn choose_sacrifice(&mut self, v: &View, candidates: &[ObjectId], n: usize) -> Vec<ObjectId> {
        let mut ranked = candidates.to_vec();
        ranked.sort_by_key(|&id| threat(v, id));
        ranked.into_iter().take(n).collect()
    }

    fn search_pick(&mut self, _v: &View, candidates: &[ObjectId], count: usize) -> Vec<ObjectId> {
        candidates.iter().copied().take(count).collect()
    }
}

fn spec_is_friendly(spec: &TargetSpec) -> bool {
    match &spec.what {
        mtg_ir::TargetWhat::Permanent(f) => f.controller == Whose::You,
        mtg_ir::TargetWhat::Player(mtg_ir::PlayerFilter::You) => true,
        _ => false,
    }
}
