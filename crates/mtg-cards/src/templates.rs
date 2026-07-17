//! The template bank: whole-line frames and effect sentence parsers.
//! Precision-first: a sentence either parses completely or is dropped and
//! reported; the main clause failing fails the line.

use mtg_ir::{
    AbilityCost, ActivatedAbility, AffectSpec, AltCost, CardTypes, ColorSet, CounterKind,
    Duration, Effect, KeywordSet, ManaAbility, ManaCost, ManaProduction, ObjFilter, ObjSel,
    PlayerSel, Recipient, ReplKind, Replacement, ReplScope, SpellAbility, SpellFilter,
    StaticAbility, TargetSpec, TargetWhat, TriggerCondition, TriggeredAbility, TrigSubject,
    ValueExpr, Whose,
};

use crate::text::{
    fixed_count, parse_count_word, parse_keyword_list, parse_obj_phrase, parse_target_phrase,
};

/// Outcome of parsing one face's worth of normalized text into a
/// CompiledFace that already has keywords and payload fields set.
pub struct ParseOutcome {
    pub matched_lines: usize,
    pub unmatched: Vec<String>,
}

pub fn parse_face(
    text: &str,
    cf: &mut mtg_ir::CompiledFace,
    face_types: CardTypes,
) -> ParseOutcome {
    let mut out = ParseOutcome { matched_lines: 0, unmatched: Vec::new() };
    let is_spell_face =
        face_types.contains(CardTypes::INSTANT) || face_types.contains(CardTypes::SORCERY);

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let mut spell_specs: Vec<TargetSpec> = Vec::new();
    let mut spell_effects: Vec<Effect> = Vec::new();
    let mut spell_dropped: Vec<String> = Vec::new();

    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() {
            continue;
        }

        // Modal block: "choose one -" followed by bullet lines.
        if let Some(choose) = parse_choose_header(line) {
            let mut modes = Vec::new();
            let mut ok = true;
            while i < lines.len() && lines[i].trim_start().starts_with('*') {
                let mode_line = lines[i].trim_start().trim_start_matches('*').trim();
                i += 1;
                match parse_sentences(mode_line, true) {
                    Some((specs, effect, dropped)) => {
                        out.unmatched.extend(dropped);
                        modes.push(SpellAbility { targets: specs, effect });
                    }
                    None => ok = false,
                }
            }
            if ok && !modes.is_empty() {
                cf.spell = Some(SpellAbility {
                    targets: Vec::new(),
                    effect: Effect::Modal { choose, modes },
                });
                out.matched_lines += 1;
            } else {
                out.unmatched.push(line.to_string());
            }
            continue;
        }

        if try_mechanic_line(line, cf)
            || try_enchant_line(line, cf)
            || try_loyalty_line(line, cf, &mut out.unmatched)
            || try_triggered_line(line, cf, &mut out.unmatched)
            || try_activated_line(line, cf, &mut out.unmatched)
            || try_static_line(line, cf, &mut out.unmatched)
            || try_keyword_line(line, cf)
        {
            out.matched_lines += 1;
            continue;
        }

        // Instant/sorcery bare imperative lines become the spell effect.
        if is_spell_face {
            match parse_sentences(line, false) {
                Some((mut specs, effect, mut dropped)) => {
                    spell_specs.append(&mut specs);
                    spell_effects.push(effect);
                    spell_dropped.append(&mut dropped);
                    out.matched_lines += 1;
                }
                None => out.unmatched.push(line.to_string()),
            }
            continue;
        }
        out.unmatched.push(line.to_string());
    }

    if is_spell_face && !spell_effects.is_empty() && cf.spell.is_none() {
        let effect = if spell_effects.len() == 1 {
            spell_effects.pop().unwrap()
        } else {
            Effect::Seq(spell_effects)
        };
        cf.spell = Some(SpellAbility { targets: spell_specs, effect });
    }
    out.unmatched.extend(spell_dropped);
    out
}

fn parse_choose_header(line: &str) -> Option<u8> {
    let l = line.trim_end_matches('-').trim();
    match l {
        "choose one" => Some(1),
        "choose two" => Some(2),
        "choose one or both" | "choose up to one" | "choose up to two" => Some(1),
        _ => None,
    }
}

/// Keyword-only line ("flying, vigilance").
fn try_keyword_line(line: &str, cf: &mut mtg_ir::CompiledFace) -> bool {
    let stripped = line.trim().trim_end_matches('.');
    match parse_keyword_list(stripped) {
        Some(kw) => {
            cf.keywords |= kw;
            true
        }
        None => {
            // Payload keywords already parsed by stage one still count.
            let l = stripped.to_ascii_lowercase();
            l.starts_with("ward {")
                || l.starts_with("protection from")
                || l.starts_with("toxic ")
        }
    }
}

/// Alt-cost and mechanic keyword lines.
fn try_mechanic_line(line: &str, cf: &mut mtg_ir::CompiledFace) -> bool {
    let l = line.trim().trim_end_matches('.');
    if let Some(rest) = l.strip_prefix("flashback ") {
        if let Some(c) = ManaCost::parse(rest.trim()) {
            cf.alt_costs.push(AltCost::Flashback(c));
            return true;
        }
    }
    if let Some(rest) = l.strip_prefix("foretell ") {
        if let Some(c) = ManaCost::parse(rest.trim()) {
            cf.alt_costs.push(AltCost::Foretell(c));
            return true;
        }
    }
    if let Some(rest) = l.strip_prefix("evoke ") {
        if let Some(c) = ManaCost::parse(rest.trim()) {
            cf.alt_costs.push(AltCost::Evoke(c));
            return true;
        }
    }
    if let Some(rest) = l.strip_prefix("cycling ") {
        if let Some(c) = ManaCost::parse(rest.trim()) {
            cf.cycling = Some(c);
            return true;
        }
    }
    if let Some(rest) = l.strip_prefix("kicker ") {
        if let Some(c) = ManaCost::parse(rest.trim()) {
            cf.kicker = Some(c);
            return true;
        }
    }
    if let Some(rest) = l.strip_prefix("escape-") {
        // "escape-{2}{b}, exile three other cards from your graveyard"
        let mut parts = rest.splitn(2, ',');
        let cost = ManaCost::parse(parts.next().unwrap_or("").trim());
        let exile_n = parts
            .next()
            .and_then(|p| {
                let p = p.trim();
                let w = p.strip_prefix("exile ")?.split_whitespace().next()?;
                fixed_count(w)
            })
            .unwrap_or(3);
        if let Some(c) = cost {
            cf.alt_costs.push(AltCost::Escape { cost: c, exile_count: exile_n });
            return true;
        }
    }
    if let Some(rest) = l.strip_prefix("equip ") {
        if let Some(c) = ManaCost::parse(rest.trim()) {
            let mut filter = ObjFilter::creature();
            filter.controller = Whose::You;
            cf.activated.push(ActivatedAbility {
                cost: AbilityCost { mana: Some(c), ..Default::default() },
                ability: SpellAbility {
                    targets: vec![TargetSpec::one(TargetWhat::Permanent(filter))],
                    effect: Effect::Attach { target: 0 },
                },
                sorcery_speed: true,
                once_per_turn: false,
                loyalty: None,
                zone: mtg_ir::AbilityZone::Battlefield,
            });
            return true;
        }
    }
    if let Some(rest) = l.strip_prefix("crew ") {
        if let Some(n) = rest.trim().parse::<u8>().ok() {
            cf.crew = Some(n);
            return true;
        }
    }
    if l == "~ enters the battlefield tapped" || l == "~ enters tapped" {
        cf.replacements.push(Replacement { scope: ReplScope::This, kind: ReplKind::EntersTapped });
        return true;
    }
    // "~ enters the battlefield with three +1/+1 counters on it"
    for prefix in ["~ enters the battlefield with ", "~ enters with "] {
        if let Some(rest) = l.strip_prefix(prefix) {
            let mut words = rest.split_whitespace();
            if let Some(n) = words.next().and_then(fixed_count) {
                if rest.contains("+1/+1 counter") {
                    cf.replacements.push(Replacement {
                        scope: ReplScope::This,
                        kind: ReplKind::EntersWithCounters {
                            kind: CounterKind::PlusOne,
                            n: ValueExpr::Fixed(n as i32),
                        },
                    });
                    return true;
                }
                if rest.contains("charge counter") {
                    cf.replacements.push(Replacement {
                        scope: ReplScope::This,
                        kind: ReplKind::EntersWithCounters {
                            kind: CounterKind::Charge,
                            n: ValueExpr::Fixed(n as i32),
                        },
                    });
                    return true;
                }
            }
        }
    }
    if l == "~ attacks each combat if able" || l == "~ attacks each turn if able" {
        cf.keywords |= KeywordSet::ATTACKS_EACH_TURN;
        return true;
    }
    if l == "~ can't block" {
        cf.keywords |= KeywordSet::CANT_BLOCK;
        return true;
    }
    if l == "~ can't be blocked" {
        cf.keywords |= KeywordSet::UNBLOCKABLE;
        return true;
    }
    false
}

fn try_enchant_line(line: &str, cf: &mut mtg_ir::CompiledFace) -> bool {
    let l = line.trim().trim_end_matches('.');
    if let Some(rest) = l.strip_prefix("enchant ") {
        if let Some((f, _)) = parse_obj_phrase(rest) {
            cf.spell = Some(SpellAbility {
                targets: vec![TargetSpec::one(TargetWhat::Permanent(f))],
                effect: Effect::Noop,
            });
            return true;
        }
    }
    false
}

/// Planeswalker loyalty lines: "+1: ...", "-3: ...", "0: ...".
fn try_loyalty_line(line: &str, cf: &mut mtg_ir::CompiledFace, riders: &mut Vec<String>) -> bool {
    let l = line.trim();
    let Some(colon) = l.find(':') else { return false };
    let head = &l[..colon];
    let delta: i8 = match head {
        "0" => 0,
        _ if head.starts_with('+') || head.starts_with('-') => match head.parse() {
            Ok(d) => d,
            Err(_) => return false,
        },
        _ => return false,
    };
    let body = l[colon + 1..].trim();
    match parse_sentences(body, true) {
        Some((specs, effect, dropped)) => {
            riders.extend(dropped);
            cf.activated.push(ActivatedAbility {
                cost: AbilityCost::default(),
                ability: SpellAbility { targets: specs, effect },
                sorcery_speed: true,
                once_per_turn: true,
                loyalty: Some(delta),
                zone: mtg_ir::AbilityZone::Battlefield,
            });
            true
        }
        None => {
            // Keep the walker usable: unknown loyalty ability becomes a
            // no-op plus, so loyalty still ticks. Only for plus abilities.
            if delta > 0 {
                cf.activated.push(ActivatedAbility {
                    cost: AbilityCost::default(),
                    ability: SpellAbility::untargeted(Effect::Noop),
                    sorcery_speed: true,
                    once_per_turn: true,
                    loyalty: Some(delta),
                    zone: mtg_ir::AbilityZone::Battlefield,
                });
            }
            false
        }
    }
}

fn try_triggered_line(line: &str, cf: &mut mtg_ir::CompiledFace, riders: &mut Vec<String>) -> bool {
    let l = line.trim();
    let lower_starts = l.starts_with("when ") || l.starts_with("whenever ") || l.starts_with("at ");
    if !lower_starts {
        return false;
    }
    let Some(comma) = l.find(", ") else { return false };
    let cond_text = l[..comma]
        .trim_start_matches("whenever ")
        .trim_start_matches("when ")
        .trim_start_matches("at ");
    let body = l[comma + 2..].trim();
    let Some(conds) = parse_trigger_condition(cond_text) else { return false };
    let Some((specs, effect, dropped)) = parse_sentences(body, true) else { return false };
    riders.extend(dropped);
    for when in conds {
        cf.triggered.push(TriggeredAbility {
            when,
            ability: SpellAbility { targets: specs.clone(), effect: effect.clone() },
            once_per_turn: false,
        });
    }
    true
}

fn parse_trigger_condition(c: &str) -> Option<Vec<TriggerCondition>> {
    let c = c.trim();
    // Beginnings of steps.
    if let Some(rest) = c.strip_prefix("the beginning of ") {
        return Some(vec![match rest {
            "your upkeep" => TriggerCondition::Upkeep(Whose::You),
            "each upkeep" | "each player's upkeep" => TriggerCondition::Upkeep(Whose::Any),
            "each opponent's upkeep" => TriggerCondition::Upkeep(Whose::Opponents),
            "your end step" => TriggerCondition::EndStep(Whose::You),
            "each end step" | "each player's end step" => TriggerCondition::EndStep(Whose::Any),
            "each opponent's end step" => TriggerCondition::EndStep(Whose::Opponents),
            "combat on your turn" => TriggerCondition::BeginCombat(Whose::You),
            "each combat" => TriggerCondition::BeginCombat(Whose::Any),
            _ => return None,
        }]);
    }
    match c {
        "~ enters the battlefield" | "~ enters" => return Some(vec![TriggerCondition::Etb(TrigSubject::This)]),
        "~ dies" => return Some(vec![TriggerCondition::Dies(TrigSubject::This)]),
        "~ leaves the battlefield" => return Some(vec![TriggerCondition::Ltb(TrigSubject::This)]),
        "~ attacks" => return Some(vec![TriggerCondition::Attacks(TrigSubject::This)]),
        "~ attacks or blocks" => {
            return Some(vec![
                TriggerCondition::Attacks(TrigSubject::This),
                TriggerCondition::Blocks(TrigSubject::This),
            ])
        }
        "~ enters the battlefield or attacks" | "~ enters or attacks" => {
            return Some(vec![
                TriggerCondition::Etb(TrigSubject::This),
                TriggerCondition::Attacks(TrigSubject::This),
            ])
        }
        "~ blocks" => return Some(vec![TriggerCondition::Blocks(TrigSubject::This)]),
        "~ deals combat damage to a player" => {
            return Some(vec![TriggerCondition::DealsCombatDamageToPlayer(TrigSubject::This)])
        }
        "you gain life" => return Some(vec![TriggerCondition::GainLife(Whose::You)]),
        "you draw a card" => return Some(vec![TriggerCondition::Draws(Whose::You)]),
        _ => {}
    }
    // "a/another <phrase> enters [the battlefield] [under your control]"
    for verb in [" enters the battlefield under your control", " enters under your control"] {
        if let Some(subj) = c.strip_suffix(verb) {
            let (mut f, _) = parse_obj_phrase(subj)?;
            f.controller = Whose::You;
            if f.types == CardTypes::LAND && f.not_types.is_empty() {
                return Some(vec![TriggerCondition::Landfall]);
            }
            return Some(vec![TriggerCondition::Etb(TrigSubject::Matching(f))]);
        }
    }
    for verb in [" enters the battlefield", " enters"] {
        if let Some(subj) = c.strip_suffix(verb) {
            let (f, _) = parse_obj_phrase(subj)?;
            if f.types == CardTypes::LAND && f.controller == Whose::You && f.not_types.is_empty() {
                return Some(vec![TriggerCondition::Landfall]);
            }
            return Some(vec![TriggerCondition::Etb(TrigSubject::Matching(f))]);
        }
    }
    if let Some(subj) = c.strip_suffix(" dies") {
        let (f, _) = parse_obj_phrase(subj)?;
        return Some(vec![TriggerCondition::Dies(TrigSubject::Matching(f))]);
    }
    if let Some(subj) = c.strip_suffix(" attacks") {
        let (f, _) = parse_obj_phrase(subj)?;
        return Some(vec![TriggerCondition::Attacks(TrigSubject::Matching(f))]);
    }
    // "you cast a <desc> spell"
    if let Some(desc) = c.strip_prefix("you cast a ").or_else(|| c.strip_prefix("you cast an ")) {
        let desc = desc.strip_suffix(" spell")?;
        let mut sf = SpellFilter::default();
        for part in desc.split(" or ") {
            match part.trim() {
                "noncreature" => sf.not_types |= CardTypes::CREATURE,
                "creature" => sf.types |= CardTypes::CREATURE,
                "instant" => sf.types |= CardTypes::INSTANT,
                "sorcery" => sf.types |= CardTypes::SORCERY,
                "artifact" => sf.types |= CardTypes::ARTIFACT,
                "enchantment" => sf.types |= CardTypes::ENCHANTMENT,
                _ => return None,
            }
        }
        return Some(vec![TriggerCondition::CastSpell { whose: Whose::You, filter: sf }]);
    }
    None
}

fn try_activated_line(line: &str, cf: &mut mtg_ir::CompiledFace, riders: &mut Vec<String>) -> bool {
    let l = line.trim();
    let Some(colon) = l.find(": ") else { return false };
    let cost_text = &l[..colon];
    let mut body = l[colon + 2..].trim().to_string();
    if cost_text.starts_with("when") || cost_text.starts_with("at ") {
        return false;
    }
    let Some(cost) = parse_ability_cost(cost_text) else { return false };
    let mut sorcery_speed = false;
    for suffix in [
        " activate only as a sorcery.",
        " activate only as a sorcery",
        " activate this ability only any time you could cast a sorcery.",
    ] {
        if let Some(b) = body.strip_suffix(suffix) {
            body = b.trim().to_string();
            sorcery_speed = true;
        }
    }
    let mut once_per_turn = false;
    for suffix in [" activate only once each turn.", " activate only once each turn"] {
        if let Some(b) = body.strip_suffix(suffix) {
            body = b.trim().to_string();
            once_per_turn = true;
        }
    }
    let Some((specs, effect, dropped)) = parse_sentences(&body, true) else { return false };
    riders.extend(dropped);

    // Pure mana production is a mana ability: no stack, used by the solver.
    if specs.is_empty() {
        if let Effect::AddMana { produce } = &effect {
            cf.mana_abilities.push(ManaAbility { cost, produce: produce.clone() });
            return true;
        }
    }
    cf.activated.push(ActivatedAbility {
        cost,
        ability: SpellAbility { targets: specs, effect },
        sorcery_speed,
        once_per_turn,
        loyalty: None,
        zone: mtg_ir::AbilityZone::Battlefield,
    });
    true
}

fn parse_ability_cost(s: &str) -> Option<AbilityCost> {
    let mut cost = AbilityCost::default();
    for item in s.split(", ") {
        let item = item.trim();
        if item == "{t}" {
            cost.tap_self = true;
        } else if item.starts_with('{') {
            // One or more mana symbols, possibly with {t} at the end like
            // "{1}{g}, {t}" already split; a mixed item "{g}{t}" is rare.
            if item.ends_with("{t}") {
                cost.tap_self = true;
                let mana = &item[..item.len() - 3];
                if !mana.is_empty() {
                    cost.mana = Some(ManaCost::parse(mana)?);
                }
            } else {
                cost.mana = Some(ManaCost::parse(item)?);
            }
        } else if item == "sacrifice ~" {
            cost.sac_self = true;
        } else if let Some(rest) = item.strip_prefix("sacrifice ") {
            let (f, _) = parse_obj_phrase(rest)?;
            cost.sac = Some(f);
        } else if let Some(rest) = item.strip_prefix("pay ") {
            let w = rest.split_whitespace().next()?;
            cost.pay_life = fixed_count(w)? as u16;
            if !rest.ends_with("life") {
                return None;
            }
        } else if item == "discard a card" {
            cost.discard_cards = 1;
        } else if item == "remove a +1/+1 counter from ~" {
            cost.remove_counters = Some((CounterKind::PlusOne, 1));
        } else if item == "remove a charge counter from ~" {
            cost.remove_counters = Some((CounterKind::Charge, 1));
        } else {
            return None;
        }
    }
    Some(cost)
}

fn try_static_line(line: &str, cf: &mut mtg_ir::CompiledFace, riders: &mut Vec<String>) -> bool {
    let l = line.trim().trim_end_matches('.');
    if l.contains(" until end of turn") || l.starts_with("when") || l.starts_with("at ") {
        return false;
    }

    // Equipped/enchanted creature buffs.
    for prefix in ["equipped creature ", "enchanted creature "] {
        if let Some(rest) = l.strip_prefix(prefix) {
            let mut p = 0;
            let mut t = 0;
            let mut kw = KeywordSet::empty();
            for clause in rest.split(" and ") {
                if let Some(r) = clause.strip_prefix("gets ") {
                    let Some((pp, tt)) = parse_pt_mod(r) else { return false };
                    p = pp;
                    t = tt;
                } else if let Some(r) = clause.strip_prefix("has ") {
                    let Some(k) = parse_keyword_list(r) else { return false };
                    kw |= k;
                } else if let Some(r) = clause.strip_prefix("gains ") {
                    let Some(k) = parse_keyword_list(r) else { return false };
                    kw |= k;
                } else {
                    return false;
                }
            }
            cf.statics.push(StaticAbility::AttachedBuff { p, t, kw });
            return true;
        }
    }

    // Cost modifiers: "<desc> spells you cast cost {n} less to cast".
    if let Some(idx) = l.find(" cost ") {
        let (head, tail) = l.split_at(idx);
        let tail = &tail[6..];
        let less = tail.contains("less to cast");
        let more = tail.contains("more to cast");
        if less || more {
            let digits: String = tail
                .trim_start_matches('{')
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            let Ok(n) = digits.parse::<i16>() else { return false };
            let (whose, desc) = if let Some(d) = head.strip_suffix(" spells you cast") {
                (Whose::You, d)
            } else if let Some(d) = head.strip_suffix(" spells your opponents cast") {
                (Whose::Opponents, d)
            } else if let Some(d) = head.strip_suffix(" spells") {
                (Whose::Any, d)
            } else {
                return false;
            };
            let mut sf = SpellFilter::default();
            for w in desc.split_whitespace() {
                match w {
                    "creature" => sf.types |= CardTypes::CREATURE,
                    "artifact" => sf.types |= CardTypes::ARTIFACT,
                    "instant" => sf.types |= CardTypes::INSTANT,
                    "sorcery" => sf.types |= CardTypes::SORCERY,
                    "enchantment" => sf.types |= CardTypes::ENCHANTMENT,
                    "noncreature" => sf.not_types |= CardTypes::CREATURE,
                    _ => return false,
                }
            }
            cf.statics.push(StaticAbility::SpellCostDelta {
                whose,
                filter: sf,
                delta: if less { -n } else { n },
            });
            return true;
        }
        return false;
    }

    // Anthems: "<group> get +1/+1 [and have flying]".
    let (group_text, rest) = if let Some(idx) = l.find(" get ") {
        (&l[..idx], &l[idx + 5..])
    } else if let Some(idx) = l.find(" have ") {
        (&l[..idx], &l[idx..])
    } else {
        return false;
    };
    let Some((f, is_card)) = parse_obj_phrase(group_text) else { return false };
    if is_card {
        return false;
    }
    let include_self = !f.other_than_self;
    let mut p = 0;
    let mut t = 0;
    let mut kw = KeywordSet::empty();
    let rest = rest.trim_start_matches(" have ").trim();
    for clause in rest.split(" and ") {
        let clause = clause.trim();
        if let Some((pp, tt)) = parse_pt_mod(clause) {
            p = pp;
            t = tt;
        } else if let Some(r) = clause.strip_prefix("have ") {
            match parse_keyword_list(r) {
                Some(k) => kw |= k,
                // A lord whose buff parses but whose granted ability we do
                // not model (islandwalk and friends) stays useful; the
                // dropped grant is disclosed and forces Partial.
                None if p != 0 || t != 0 => riders.push(format!("unmodeled grant: {r}")),
                None => return false,
            }
        } else if let Some(k) = parse_keyword_list(clause) {
            kw |= k;
        } else if p != 0 || t != 0 {
            riders.push(format!("unmodeled clause: {clause}"));
        } else {
            return false;
        }
    }
    let affects = AffectSpec { filter: f, include_self };
    if p != 0 || t != 0 {
        cf.statics.push(StaticAbility::PtBuff { affects: affects.clone(), p, t });
    }
    if !kw.is_empty() {
        cf.statics.push(StaticAbility::GrantKeywords { affects, kw });
    }
    p != 0 || t != 0 || !kw.is_empty()
}

/// "+2/+2", "-1/-1", "+x/+0".
fn parse_pt_mod(s: &str) -> Option<(i32, i32)> {
    let s = s.trim().trim_end_matches('.').trim_end_matches(" until end of turn");
    let (p, t) = s.split_once('/')?;
    let parse_half = |h: &str| -> Option<i32> {
        let h = h.trim();
        if h.eq_ignore_ascii_case("+x") || h.eq_ignore_ascii_case("-x") {
            return None; // X pumps go through the sentence path
        }
        h.strip_prefix('+').unwrap_or(h).parse::<i32>().ok()
    };
    Some((parse_half(p)?, parse_half(t)?))
}

/// Split a body into sentences and parse each. Returns the collected target
/// specs, the composed effect, and dropped rider sentences. Fails if the
/// first sentence fails.
pub fn parse_sentences(
    body: &str,
    trigger_ctx: bool,
) -> Option<(Vec<TargetSpec>, Effect, Vec<String>)> {
    let mut specs: Vec<TargetSpec> = Vec::new();
    let mut effects: Vec<Effect> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();

    // Thoughtseize shape: "target player reveals their hand. you choose a
    // <desc> card from it. that player discards that card. [more]" The
    // victim's agent picks the discard, which underrates the caster's
    // selection slightly but keeps the card honest.
    let mut body = body;
    if body.starts_with("target player reveals their hand") {
        if let Some(idx) = body.find("discards that card.") {
            let i = push_spec(&mut specs, TargetSpec::one(TargetWhat::Player(mtg_ir::PlayerFilter::Any)));
            effects.push(Effect::Discard {
                who: PlayerSel::Target(i),
                n: ValueExpr::Fixed(1),
                random: false,
            });
            body = body[idx + 19..].trim();
            if body.is_empty() {
                return Some((specs, effects.pop().unwrap(), dropped));
            }
        }
    }

    let sentences: Vec<&str> = body
        .split(". ")
        .map(|s| s.trim().trim_end_matches('.'))
        .filter(|s| !s.is_empty())
        .collect();
    if sentences.is_empty() {
        return None;
    }
    for (i, s) in sentences.iter().enumerate() {
        match parse_sentence(s, &mut specs, trigger_ctx) {
            Some(e) => effects.push(e),
            None if i == 0 => return None,
            None => dropped.push((*s).to_string()),
        }
    }
    let effect = if effects.len() == 1 { effects.pop().unwrap() } else { Effect::Seq(effects) };
    Some((specs, effect, dropped))
}

fn push_spec(specs: &mut Vec<TargetSpec>, spec: TargetSpec) -> u8 {
    specs.push(spec);
    (specs.len() - 1) as u8
}

fn parse_player_sel(s: &str, trigger_ctx: bool) -> Option<PlayerSel> {
    Some(match s.trim() {
        "you" => PlayerSel::You,
        "each opponent" => PlayerSel::EachOpponent,
        "each player" => PlayerSel::EachPlayer,
        "that player" | "that player's" if trigger_ctx => PlayerSel::TriggerPlayer,
        _ => return None,
    })
}

fn parse_sentence(s: &str, specs: &mut Vec<TargetSpec>, trigger_ctx: bool) -> Option<Effect> {
    let mark = specs.len();
    if let Some(e) = parse_sentence_inner(s, specs, trigger_ctx) {
        return Some(e);
    }
    specs.truncate(mark);

    // Compound sentence fallback: "A and B" where both halves parse on
    // their own (Lightning Helix), or the second half is a damage
    // continuation aimed at the first target's controller (Searing Blaze).
    if let Some(idx) = s.find(" and ") {
        let (first, second) = (&s[..idx], &s[idx + 5..]);
        if let Some(e1) = parse_sentence_inner(first, specs, trigger_ctx) {
            let cont = second
                .split_once(" damage to ")
                .filter(|(_, tail)| {
                    matches!(
                        tail.trim().trim_end_matches('.'),
                        "that creature's controller" | "its controller" | "that player"
                    )
                })
                .and_then(|(n, _)| parse_count_word(n.trim()));
            if let Some(n) = cont {
                if !specs.is_empty() {
                    let t = (specs.len() - 1) as u8;
                    return Some(Effect::Seq(vec![
                        e1,
                        Effect::DealDamage {
                            n,
                            to: Recipient::Player(PlayerSel::ControllerOf(Box::new(
                                ObjSel::Target(t),
                            ))),
                        },
                    ]));
                }
            }
            if let Some(e2) = parse_sentence_inner(second, specs, trigger_ctx) {
                return Some(Effect::Seq(vec![e1, e2]));
            }
        }
        specs.truncate(mark);
    }
    None
}

fn parse_sentence_inner(s: &str, specs: &mut Vec<TargetSpec>, trigger_ctx: bool) -> Option<Effect> {
    let mut s = s.trim();
    for prefix in ["you may ", "then ", "if you do, "] {
        if let Some(r) = s.strip_prefix(prefix) {
            s = r.trim();
        }
    }
    if s.is_empty() {
        return None;
    }

    // Damage.
    for subject in ["~ deals ", "it deals "] {
        if let Some(rest) = s.strip_prefix(subject) {
            return parse_damage(rest, specs, trigger_ctx);
        }
    }

    // Destroy and exile.
    for (verb, exile) in [("destroy ", false), ("exile ", true)] {
        if let Some(rest) = s.strip_prefix(verb) {
            if exile {
                if let Some(idx) = rest.find(" until ~ leaves the battlefield") {
                    let (spec, _) = parse_target_phrase(&rest[..idx])?;
                    let i = push_spec(specs, spec);
                    return Some(Effect::ExileUntilSourceLeaves { what: ObjSel::Target(i) });
                }
            }
            if rest.starts_with("target ") || rest.starts_with("up to ") {
                let (mut spec, tail) = parse_target_phrase(rest)?;
                let tail = tail.trim();
                // Fatal Push shape: fold "if it has mana value N or less"
                // into the target filter.
                if let Some(clause) = tail.strip_prefix("if it has ") {
                    let (field, cmp, n) = crate::text::parse_value_clause(clause)?;
                    if let TargetWhat::Permanent(f) = &mut spec.what {
                        match field {
                            crate::text::ValueField::ManaValue => f.mana_value = Some((cmp, n)),
                            crate::text::ValueField::Power => f.power = Some((cmp, n)),
                            crate::text::ValueField::Toughness => f.toughness = Some((cmp, n)),
                        }
                    } else {
                        return None;
                    }
                } else if !tail.is_empty() && !tail.starts_with("it can't be regenerated") {
                    return None;
                }
                let i = push_spec(specs, spec);
                return Some(if exile {
                    Effect::Exile { what: ObjSel::Target(i) }
                } else {
                    Effect::Destroy { what: ObjSel::Target(i) }
                });
            }
            for all_word in ["all ", "each "] {
                if let Some(phrase) = rest.strip_prefix(all_word) {
                    let (f, is_card) = parse_obj_phrase(phrase)?;
                    if is_card {
                        return None;
                    }
                    return Some(if exile {
                        Effect::Exile { what: ObjSel::All(f) }
                    } else {
                        Effect::Destroy { what: ObjSel::All(f) }
                    });
                }
            }
            if exile && (rest == "it" || rest == "that card" || rest == "that creature") && trigger_ctx {
                return Some(Effect::Exile { what: ObjSel::TriggerSubject });
            }
            return None;
        }
    }

    // Counterspells.
    if let Some(rest) = s.strip_prefix("counter ") {
        let (unless, rest) = match rest.find(" unless its controller pays ") {
            Some(idx) => {
                let cost = ManaCost::parse(rest[idx + 28..].trim())?;
                (Some(cost), &rest[..idx])
            }
            None => (None, rest),
        };
        let (spec, tail) = parse_target_phrase(rest)?;
        if !matches!(spec.what, TargetWhat::SpellOnStack(_)) || !tail.trim().is_empty() {
            return None;
        }
        let i = push_spec(specs, spec);
        return Some(Effect::CounterSpell { target: i, unless_pay: unless });
    }

    // Draw.
    if let Some(rest) = s.strip_prefix("draw ") {
        return parse_draw_tail(rest, PlayerSel::You);
    }
    if let Some(rest) = s.strip_prefix("you draw ") {
        return parse_draw_tail(rest, PlayerSel::You);
    }
    if let Some(rest) = s.strip_prefix("target player draws ") {
        let i = push_spec(specs, TargetSpec::one(TargetWhat::Player(mtg_ir::PlayerFilter::Any)));
        return parse_draw_tail(rest, PlayerSel::Target(i));
    }
    if let Some(rest) = s.strip_prefix("each player draws ") {
        return parse_draw_tail(rest, PlayerSel::EachPlayer);
    }

    // Discard.
    if let Some(rest) = s.strip_prefix("target player discards ") {
        let i = push_spec(specs, TargetSpec::one(TargetWhat::Player(mtg_ir::PlayerFilter::Any)));
        return parse_discard_tail(rest, PlayerSel::Target(i));
    }
    if let Some(rest) = s.strip_prefix("each opponent discards ") {
        return parse_discard_tail(rest, PlayerSel::EachOpponent);
    }
    if let Some(rest) = s.strip_prefix("discard ") {
        return parse_discard_tail(rest, PlayerSel::You);
    }

    // Life.
    if let Some(e) = parse_life_sentence(s, specs) {
        return Some(e);
    }

    // Tokens.
    if let Some(rest) = s.strip_prefix("create ") {
        return parse_token_sentence(rest);
    }

    // Counters.
    if let Some(rest) = s.strip_prefix("put ") {
        if let Some(e) = parse_put_counters(rest, specs) {
            return Some(e);
        }
        return None;
    }

    // Pump and keyword grants on targets, self, or groups.
    if let Some(e) = parse_pump_sentence(s, specs, trigger_ctx) {
        return Some(e);
    }

    // Bounce and graveyard returns.
    if let Some(rest) = s.strip_prefix("return ") {
        return parse_return_sentence(rest, specs, trigger_ctx);
    }

    // Search.
    if let Some(rest) = s.strip_prefix("search your library for ") {
        return parse_search_sentence(rest);
    }
    if s == "shuffle your library" || s == "then shuffle" || s == "shuffle" {
        return Some(Effect::Shuffle { who: PlayerSel::You });
    }

    // Tap and untap.
    for (verb, tap) in [("tap ", true), ("untap ", false)] {
        if let Some(rest) = s.strip_prefix(verb) {
            if rest == "~" {
                return Some(Effect::TapObjects { what: ObjSel::This, tap });
            }
            if rest.starts_with("target ") || rest.starts_with("up to ") {
                let (spec, tail) = parse_target_phrase(rest)?;
                // A rider like "it doesn't untap during its controller's
                // next untap step" is droppable but fails here for
                // precision; the sentence splitter already cut on ". ".
                if !tail.trim().is_empty() {
                    return None;
                }
                let i = push_spec(specs, spec);
                return Some(Effect::TapObjects { what: ObjSel::Target(i), tap });
            }
            for all_word in ["all ", "each "] {
                if let Some(phrase) = rest.strip_prefix(all_word) {
                    let (f, _) = parse_obj_phrase(phrase)?;
                    return Some(Effect::TapObjects { what: ObjSel::All(f), tap });
                }
            }
            return None;
        }
    }

    // Scry, surveil, mill.
    if let Some(rest) = s.strip_prefix("scry ") {
        let n = parse_count_word(rest.trim())?;
        return Some(Effect::Scry { who: PlayerSel::You, n });
    }
    if let Some(rest) = s.strip_prefix("surveil ") {
        let n = parse_count_word(rest.trim())?;
        return Some(Effect::Surveil { who: PlayerSel::You, n });
    }
    if let Some(rest) = s.strip_prefix("target player mills ") {
        let n = parse_count_word(rest.split_whitespace().next()?)?;
        let i = push_spec(specs, TargetSpec::one(TargetWhat::Player(mtg_ir::PlayerFilter::Any)));
        return Some(Effect::Mill { who: PlayerSel::Target(i), n });
    }
    if let Some(rest) = s.strip_prefix("each opponent mills ") {
        let n = parse_count_word(rest.split_whitespace().next()?)?;
        return Some(Effect::Mill { who: PlayerSel::EachOpponent, n });
    }
    if let Some(rest) = s.strip_prefix("mill ") {
        let n = parse_count_word(rest.split_whitespace().next()?)?;
        return Some(Effect::Mill { who: PlayerSel::You, n });
    }

    // Fight.
    if let Some(rest) = s.strip_prefix("~ fights ") {
        let (spec, tail) = parse_target_phrase(rest)?;
        if !tail.trim().is_empty() {
            return None;
        }
        let i = push_spec(specs, spec);
        return Some(Effect::Fight { a: ObjSel::This, b: ObjSel::Target(i) });
    }
    if s.starts_with("target creature you control fights ") {
        let (spec_a, rest) = parse_target_phrase(s)?;
        let rest = rest.trim_start().strip_prefix("fights ")?;
        let (spec_b, tail) = parse_target_phrase(rest)?;
        if !tail.trim().is_empty() {
            return None;
        }
        let a = push_spec(specs, spec_a);
        let b = push_spec(specs, spec_b);
        return Some(Effect::Fight { a: ObjSel::Target(a), b: ObjSel::Target(b) });
    }

    // Sacrifice demands.
    for (prefix, who) in [
        ("each opponent sacrifices ", PlayerSel::EachOpponent),
        ("each player sacrifices ", PlayerSel::EachPlayer),
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let words: Vec<&str> = rest.splitn(2, ' ').collect();
            let (n, phrase) = match parse_count_word(words[0]) {
                Some(v) => (v, words.get(1).copied().unwrap_or("")),
                None => (ValueExpr::Fixed(1), rest),
            };
            let (f, _) = parse_obj_phrase(phrase)?;
            return Some(Effect::Sacrifice { who, filter: f, n });
        }
    }

    // Mana.
    if let Some(rest) = s.strip_prefix("add ") {
        return parse_add_mana(rest);
    }

    // Control.
    if let Some(rest) = s.strip_prefix("gain control of ") {
        let until = rest.contains("until end of turn");
        let rest = rest.replace(" until end of turn", "");
        let (spec, tail) = parse_target_phrase(&rest)?;
        if !tail.trim().is_empty() {
            return None;
        }
        let i = push_spec(specs, spec);
        return Some(Effect::GainControl {
            what: ObjSel::Target(i),
            dur: if until { Duration::EndOfTurn } else { Duration::Permanent },
        });
    }

    if s == "untap it" && trigger_ctx {
        return Some(Effect::TapObjects { what: ObjSel::TriggerSubject, tap: false });
    }
    if s == "transform ~" {
        return Some(Effect::Transform);
    }
    if s == "sacrifice ~" || s == "sacrifice it at the beginning of the next end step" {
        // Modeled as immediate for simplicity; close enough for value math.
        return Some(Effect::Custom(mtg_ir::OverrideId(0)));
    }

    None
}

fn parse_damage(rest: &str, specs: &mut Vec<TargetSpec>, trigger_ctx: bool) -> Option<Effect> {
    // "<n> damage to <recipient>"
    let mut it = rest.splitn(2, ' ');
    let n = parse_count_word(it.next()?)?;
    let rest = it.next()?.strip_prefix("damage to ")?;
    let rest = rest.trim().trim_end_matches('.');
    if rest.contains(" and ") || rest.contains("divided") {
        return None;
    }
    if rest == "any target" {
        let i = push_spec(specs, TargetSpec::one(TargetWhat::AnyDamageable));
        return Some(Effect::DealDamage { n, to: Recipient::Target(i) });
    }
    if rest.starts_with("target ") || rest.starts_with("up to ") {
        let (spec, tail) = parse_target_phrase(rest)?;
        if !tail.trim().is_empty() {
            return None;
        }
        let i = push_spec(specs, spec);
        return Some(Effect::DealDamage { n, to: Recipient::Target(i) });
    }
    if let Some(sel) = parse_player_sel(rest, trigger_ctx) {
        return Some(Effect::DealDamage { n, to: Recipient::Player(sel) });
    }
    for all_word in ["each ", "all "] {
        if let Some(phrase) = rest.strip_prefix(all_word) {
            if phrase == "creature" || phrase == "creatures" {
                return Some(Effect::DealDamage {
                    n,
                    to: Recipient::Object(ObjSel::All(ObjFilter::creature())),
                });
            }
            let (f, is_card) = parse_obj_phrase(phrase).filter(|_| !phrase.contains("player"))?;
            if is_card {
                return None;
            }
            return Some(Effect::DealDamage { n, to: Recipient::Object(ObjSel::All(f)) });
        }
    }
    if (rest == "it" || rest == "that creature") && trigger_ctx {
        return Some(Effect::DealDamage { n, to: Recipient::Object(ObjSel::TriggerSubject) });
    }
    None
}

fn parse_draw_tail(rest: &str, who: PlayerSel) -> Option<Effect> {
    let mut words = rest.split_whitespace();
    let n = parse_count_word(words.next()?)?;
    match words.next() {
        Some("card") | Some("cards") => Some(Effect::Draw { who, n }),
        _ => None,
    }
}

fn parse_discard_tail(rest: &str, who: PlayerSel) -> Option<Effect> {
    let mut words = rest.split_whitespace();
    let n = parse_count_word(words.next()?)?;
    let noun = words.next()?;
    if noun != "card" && noun != "cards" {
        return None;
    }
    let random = rest.contains("at random");
    Some(Effect::Discard { who, n, random })
}

fn parse_life_sentence(s: &str, specs: &mut Vec<TargetSpec>) -> Option<Effect> {
    for (prefix, who, gain) in [
        ("you gain ", Some(PlayerSel::You), true),
        ("you lose ", Some(PlayerSel::You), false),
        ("each opponent loses ", Some(PlayerSel::EachOpponent), false),
        ("each opponent gains ", Some(PlayerSel::EachOpponent), true),
        ("each player loses ", Some(PlayerSel::EachPlayer), false),
        ("target player gains ", None, true),
        ("target player loses ", None, false),
        ("target opponent loses ", None, false),
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let mut words = rest.split_whitespace();
            let n = parse_count_word(words.next()?)?;
            if words.next()? != "life" {
                return None;
            }
            let who = match who {
                Some(w) => w,
                None => {
                    let pf = if prefix.contains("opponent") {
                        mtg_ir::PlayerFilter::Opponent
                    } else {
                        mtg_ir::PlayerFilter::Any
                    };
                    let i = push_spec(specs, TargetSpec::one(TargetWhat::Player(pf)));
                    PlayerSel::Target(i)
                }
            };
            return Some(if gain {
                Effect::GainLife { who, n }
            } else {
                Effect::LoseLife { who, n }
            });
        }
    }
    None
}

fn parse_token_sentence(rest: &str) -> Option<Effect> {
    // "<n> <p>/<t> <colors> <subtypes> creature token[s] [with <kws>][, tapped...]"
    let rest = rest.trim().trim_end_matches('.');
    let mut words: Vec<&str> = rest.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let n = parse_count_word(words[0])?;
    words.remove(0);
    let mut tapped = false;
    let mut attacking = false;
    let mut kw = KeywordSet::empty();
    // Trailing "with <kwlist>".
    let joined = words.join(" ");
    let mut body = joined.as_str();
    if let Some(idx) = body.find(" with ") {
        let (b, w) = body.split_at(idx);
        kw = parse_keyword_list(&w[6..])?;
        body = b;
    }
    let mut body = body.to_string();
    if body.contains("tapped") {
        tapped = true;
        body = body.replace("tapped and attacking", "").replace("tapped", "");
    }
    if body.contains("attacking") {
        attacking = true;
        body = body.replace("attacking", "");
    }
    let body = body.replace(',', " ");
    let mut p = 0i32;
    let mut t = 0i32;
    let mut colors = ColorSet::empty();
    let mut subtypes: Vec<Box<str>> = Vec::new();
    let mut types = CardTypes::empty();
    let mut saw_token = false;
    for w in body.split_whitespace() {
        if let Some((pp, tt)) = w.split_once('/') {
            p = pp.parse().ok()?;
            t = tt.parse().ok()?;
            continue;
        }
        match w {
            "white" => colors |= ColorSet::W,
            "blue" => colors |= ColorSet::U,
            "black" => colors |= ColorSet::B,
            "red" => colors |= ColorSet::R,
            "green" => colors |= ColorSet::G,
            "colorless" => {}
            "creature" => types |= CardTypes::CREATURE,
            "artifact" => types |= CardTypes::ARTIFACT,
            "enchantment" => types |= CardTypes::ENCHANTMENT,
            "legendary" => {}
            "token" | "tokens" => saw_token = true,
            "and" | "that's" | "it's" => return None,
            _ if w.chars().all(|c| c.is_ascii_alphabetic()) => {
                subtypes.push(w.to_ascii_lowercase().into())
            }
            _ => return None,
        }
    }
    if !saw_token || !types.contains(CardTypes::CREATURE) {
        return None;
    }
    let name = subtypes.first().cloned().unwrap_or_else(|| "token".into());
    Some(Effect::CreateToken {
        proto: mtg_ir::TokenProto { name, power: p, toughness: t, types, subtypes, colors, keywords: kw },
        n,
        tapped,
        attacking,
    })
}

fn parse_put_counters(rest: &str, specs: &mut Vec<TargetSpec>) -> Option<Effect> {
    // "<n> +1/+1 counter[s] on <sel>"
    let mut words = rest.splitn(2, ' ');
    let n = parse_count_word(words.next()?)?;
    let rest = words.next()?;
    let kind = if rest.starts_with("+1/+1 counter") {
        CounterKind::PlusOne
    } else if rest.starts_with("-1/-1 counter") {
        CounterKind::MinusOne
    } else if rest.starts_with("charge counter") {
        CounterKind::Charge
    } else if rest.starts_with("loyalty counter") {
        CounterKind::Loyalty
    } else {
        return None;
    };
    let on = rest.find(" on ")?;
    let sel_text = rest[on + 4..].trim().trim_end_matches('.');
    let what = if sel_text == "~" || sel_text == "it" {
        ObjSel::This
    } else if sel_text.starts_with("target ") || sel_text.starts_with("up to ") {
        let (spec, tail) = parse_target_phrase(sel_text)?;
        if !tail.trim().is_empty() {
            return None;
        }
        ObjSel::Target(push_spec(specs, spec))
    } else if let Some(phrase) = sel_text.strip_prefix("each ") {
        let (f, _) = parse_obj_phrase(phrase)?;
        ObjSel::All(f)
    } else {
        return None;
    };
    Some(Effect::PutCounters { what, kind, n })
}

fn parse_pump_sentence(s: &str, specs: &mut Vec<TargetSpec>, trigger_ctx: bool) -> Option<Effect> {
    let until = s.contains(" until end of turn");
    let body = s.replace(" until end of turn", "");
    let dur = if until { Duration::EndOfTurn } else { Duration::EndOfTurn };

    // Subject.
    let (what, rest): (ObjSel, &str) = if let Some(r) = body.strip_prefix("~ gets ") {
        (ObjSel::This, r)
    } else if let Some(r) = body.strip_prefix("~ gains ") {
        let kw = parse_keyword_list(r)?;
        return Some(Effect::GrantKeywords { what: ObjSel::This, kw, dur });
    } else if let Some(r) = body.strip_prefix("it gets ").filter(|_| trigger_ctx) {
        (ObjSel::TriggerSubject, r)
    } else if let Some(r) = body.strip_prefix("it gains ").filter(|_| trigger_ctx) {
        let kw = parse_keyword_list(r)?;
        return Some(Effect::GrantKeywords { what: ObjSel::TriggerSubject, kw, dur });
    } else if body.starts_with("target ") || body.starts_with("up to ") {
        let (spec, tail) = parse_target_phrase(&body)?;
        let tail = tail.trim_start();
        let i = push_spec(specs, spec);
        let what = ObjSel::Target(i);
        if let Some(r) = tail.strip_prefix("gets ") {
            return finish_pump(what, r, dur);
        }
        if let Some(r) = tail.strip_prefix("gains ").or_else(|| tail.strip_prefix("gain ")) {
            if r.starts_with("haste") || parse_keyword_list(r).is_some() {
                let kw = parse_keyword_list(r)?;
                return Some(Effect::GrantKeywords { what, kw, dur });
            }
        }
        if tail.starts_with("can't block this turn") {
            return Some(Effect::GrantKeywords { what, kw: KeywordSet::CANT_BLOCK, dur });
        }
        specs.pop();
        return None;
    } else if let Some(idx) = body.find(" get ") {
        let (group, r) = body.split_at(idx);
        let (f, is_card) = parse_obj_phrase(group)?;
        if is_card {
            return None;
        }
        return finish_pump(ObjSel::All(f), &r[5..], dur);
    } else if let Some(idx) = body.find(" gain ") {
        let (group, r) = body.split_at(idx);
        let (f, is_card) = parse_obj_phrase(group)?;
        if is_card {
            return None;
        }
        let kw = parse_keyword_list(&r[6..])?;
        return Some(Effect::GrantKeywords { what: ObjSel::All(f), kw, dur });
    } else {
        return None;
    };
    finish_pump(what, rest, dur)
}

fn finish_pump(what: ObjSel, rest: &str, dur: Duration) -> Option<Effect> {
    // "+2/+2" possibly "and gains trample"
    let mut parts = rest.splitn(2, " and gains ");
    let pt = parts.next()?.trim();
    let (p, t) = {
        let (p, t) = pt.split_once('/')?;
        let parse_half = |h: &str| -> Option<ValueExpr> {
            let h = h.trim().trim_end_matches('.');
            let (sign, num) = if let Some(r) = h.strip_prefix('+') {
                (1, r)
            } else if let Some(r) = h.strip_prefix('-') {
                (-1, r)
            } else {
                (1, h)
            };
            if num == "x" {
                return Some(ValueExpr::X);
            }
            num.parse::<i32>().ok().map(|n| ValueExpr::Fixed(sign * n))
        };
        (parse_half(p)?, parse_half(t)?)
    };
    let pump = Effect::ModifyPt { what: what.clone(), p, t, dur };
    match parts.next() {
        Some(kws) => {
            let kw = parse_keyword_list(kws)?;
            Some(Effect::Seq(vec![pump, Effect::GrantKeywords { what, kw, dur }]))
        }
        None => Some(pump),
    }
}

fn parse_return_sentence(rest: &str, specs: &mut Vec<TargetSpec>, trigger_ctx: bool) -> Option<Effect> {
    let rest = rest.trim().trim_end_matches('.');
    if rest == "~ to its owner's hand" {
        return Some(Effect::Bounce { what: ObjSel::This });
    }
    if (rest == "it to its owner's hand" || rest == "that card to its owner's hand") && trigger_ctx {
        return Some(Effect::Bounce { what: ObjSel::TriggerSubject });
    }
    if rest.starts_with("target ") || rest.starts_with("up to ") {
        let (spec, tail) = parse_target_phrase(rest)?;
        let tail = tail.trim();
        let is_gy = matches!(spec.what, TargetWhat::CardInGraveyard(..));
        let i = push_spec(specs, spec);
        if tail.starts_with("to its owner's hand")
            || tail.starts_with("to their owners' hands")
            || (is_gy && tail.starts_with("to your hand"))
            || (is_gy && tail.is_empty() && rest.contains("to your hand"))
        {
            return Some(Effect::Bounce { what: ObjSel::Target(i) });
        }
        if is_gy && tail.starts_with("to the battlefield") {
            let tapped = tail.contains("tapped");
            return Some(Effect::Reanimate {
                what: ObjSel::Target(i),
                controller: PlayerSel::You,
                tapped,
            });
        }
        specs.pop();
        return None;
    }
    None
}

fn parse_search_sentence(rest: &str) -> Option<Effect> {
    // "<a|up to n> <phrase> card[s], ... put <it|them> <dest> ... shuffle"
    let rest = rest.trim().trim_end_matches('.');
    let (count, rest) = if let Some(r) = rest.strip_prefix("up to ") {
        let mut it = r.splitn(2, ' ');
        (fixed_count(it.next()?)?, it.next()?)
    } else if let Some(r) = rest.strip_prefix("a ").or_else(|| rest.strip_prefix("an ")) {
        (1u8, r)
    } else {
        return None;
    };
    let card_idx = rest.find(" card")?;
    let (f, _) = parse_obj_phrase(&rest[..card_idx + 5])?;
    let tail = &rest[card_idx..];
    let dest = if tail.contains("onto the battlefield") {
        mtg_ir::SearchDest::Battlefield
    } else if tail.contains("into your hand") || tail.contains("reveal") && tail.contains("hand") {
        mtg_ir::SearchDest::Hand
    } else if tail.contains("into your graveyard") {
        mtg_ir::SearchDest::Graveyard
    } else if tail.contains("on top of your library") {
        mtg_ir::SearchDest::TopOfLibrary
    } else {
        mtg_ir::SearchDest::Hand
    };
    let enters_tapped = tail.contains("battlefield tapped");
    Some(Effect::SearchLibrary { who: PlayerSel::You, filter: f, dest, count, enters_tapped })
}

fn parse_add_mana(rest: &str) -> Option<Effect> {
    let rest = rest.trim().trim_end_matches('.');
    if rest == "one mana of any color" {
        return Some(Effect::AddMana { produce: ManaProduction::AnyColor });
    }
    if rest.starts_with('{') {
        let cost = ManaCost::parse(rest)?;
        // Reuse the mana cost parser: pips become production.
        if cost.x_count > 0 || !cost.hybrid.is_empty() || !cost.phyrexian.is_empty() {
            return None;
        }
        return Some(Effect::AddMana {
            produce: ManaProduction::Fixed {
                w: cost.pips[0],
                u: cost.pips[1],
                b: cost.pips[2],
                r: cost.pips[3],
                g: cost.pips[4],
                c: (cost.colorless as u8).saturating_add(cost.generic.min(20) as u8),
            },
        });
    }
    None
}
