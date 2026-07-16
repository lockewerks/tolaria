//! The behavior compiler, stage one: keyword short-circuit, mana-producing
//! lands, ward and toxic payloads, enters-tapped replacements, and coverage
//! grading for vanilla and keyword-only cards. The template bank extends
//! this; overrides pre-empt it.

use mtg_data::{OracleCard, OracleFace};
use mtg_ir::{
    AbilityCost, CardTypes, ColorSet, CompiledCard, CompiledFace, CoverageTier, KeywordSet,
    ManaAbility, ManaCost, ManaProduction, ReplKind, Replacement, ReplScope, ValueExpr,
};

pub const COMPILER_VERSION: u16 = 1;

/// Strip reminder text (parenthesized) and normalize whitespace.
pub fn strip_reminders(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0u32;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Is every line of this text purely a keyword list we model?
fn all_lines_are_keywords(text: &str, known: &mut KeywordSet) -> bool {
    for line in text.lines() {
        let line = line.trim().trim_end_matches('.');
        if line.is_empty() {
            continue;
        }
        for part in line.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match KeywordSet::from_scryfall(part) {
                Some(k) => *known |= k,
                None => {
                    // Payload keywords handled elsewhere still count as
                    // covered lines.
                    let lower = part.to_ascii_lowercase();
                    if lower.starts_with("ward ")
                        || lower.starts_with("ward{")
                        || lower.starts_with("toxic ")
                        || lower.starts_with("protection from ")
                    {
                        continue;
                    }
                    return false;
                }
            }
        }
    }
    true
}

fn parse_ward(text: &str) -> Option<ManaCost> {
    let lower = text.to_ascii_lowercase();
    let idx = lower.find("ward {")?;
    let rest = &text[idx + 5..];
    let end = rest.find(|c: char| c != '{' && c != '}' && !c.is_ascii_alphanumeric() && c != '/')
        .unwrap_or(rest.len());
    ManaCost::parse(rest[..end].trim())
}

fn parse_toxic(text: &str) -> u8 {
    let lower = text.to_ascii_lowercase();
    if let Some(idx) = lower.find("toxic ") {
        lower[idx + 6..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0)
    } else {
        0
    }
}

fn parse_protection(text: &str) -> ColorSet {
    let lower = text.to_ascii_lowercase();
    let mut out = ColorSet::empty();
    for (name, set) in [
        ("protection from white", ColorSet::W),
        ("protection from blue", ColorSet::U),
        ("protection from black", ColorSet::B),
        ("protection from red", ColorSet::R),
        ("protection from green", ColorSet::G),
    ] {
        if lower.contains(name) {
            out |= set;
        }
    }
    if lower.contains("protection from all colors") || lower.contains("protection from each color")
    {
        out = ColorSet::W | ColorSet::U | ColorSet::B | ColorSet::R | ColorSet::G;
    }
    out
}

/// Compile one card. Stage one alone produces Full for vanilla, keyword-only
/// creatures, and plain mana lands; everything else grades Proxy here and is
/// upgraded by the template stage.
pub fn compile(card: &OracleCard) -> CompiledCard {
    let mut faces = Vec::with_capacity(card.faces.len());
    let mut tier = CoverageTier::Full;
    let mut dropped: Vec<Box<str>> = Vec::new();

    for face in &card.faces {
        let (cf, face_tier, mut face_dropped) = compile_face(card, face);
        tier = tier.min(face_tier);
        dropped.append(&mut face_dropped);
        faces.push(cf);
    }
    if faces.is_empty() {
        faces.push(CompiledFace::default());
        tier = CoverageTier::Unplayable;
    }
    CompiledCard { tier, dropped, faces, compiler_version: COMPILER_VERSION }
}

fn compile_face(card: &OracleCard, face: &OracleFace) -> (CompiledFace, CoverageTier, Vec<Box<str>>) {
    let mut cf = CompiledFace::default();
    let mut dropped: Vec<Box<str>> = Vec::new();

    for kw in &card.keywords {
        if let Some(k) = KeywordSet::from_scryfall(kw) {
            cf.keywords |= k;
        }
    }
    let text = strip_reminders(&face.oracle_text);
    cf.ward = parse_ward(&text);
    cf.toxic = parse_toxic(&text);
    cf.protection_from = parse_protection(&text);

    let is_land = face.types.contains(CardTypes::LAND);
    if !is_land {
        match ManaCost::parse(&face.mana_cost) {
            Some(cost) => {
                cf.x_spell = cost.x_count > 0;
                cf.cost = Some(cost);
            }
            None => {
                // Unparseable cost: the card can never be cast.
                return (cf, CoverageTier::Unplayable, dropped);
            }
        }
        // Back faces of transforming cards have empty costs and are only
        // reached by transforming; empty cost on a non-land front face of a
        // normal card means a truly free spell, which parse handles.
        if face.mana_cost.is_empty() && card.faces.len() > 1 {
            cf.cost = None;
        }
    }

    let lower = text.to_ascii_lowercase();
    let mut tier = CoverageTier::Full;

    if is_land {
        // Mana production from Scryfall's produced_mana.
        if !card.produced_mana.is_empty() {
            cf.mana_abilities.push(ManaAbility {
                cost: AbilityCost::tap(),
                produce: ManaProduction::AnyOneOf(card.produced_mana),
            });
        }
        if lower.contains("enters the battlefield tapped")
            || lower.contains("enters tapped")
            || lower.contains("this land enters tapped")
        {
            if lower.contains("unless") || lower.contains(" if ") {
                // Conditionally tapped: play it untapped and disclose.
                tier = tier.min(CoverageTier::Partial);
                dropped.push("conditional enters-tapped treated as untapped".into());
            } else {
                cf.replacements.push(Replacement {
                    scope: ReplScope::This,
                    kind: ReplKind::EntersTapped,
                });
            }
        }
        // A land whose text is only tapped-clauses and mana abilities is
        // fully modeled; anything else is partially modeled at this stage.
        let residual = land_residual_text(&lower);
        if residual && tier == CoverageTier::Full {
            tier = CoverageTier::Partial;
            dropped.push(face.oracle_text.clone());
        }
        return (cf, tier, dropped);
    }

    // Non-land faces.
    if text.trim().is_empty() {
        return (cf, CoverageTier::Full, dropped);
    }
    let mut known = cf.keywords;
    if all_lines_are_keywords(&text, &mut known) {
        cf.keywords = known;
        return (cf, CoverageTier::Full, dropped);
    }

    // The template stage upgrades from here; stage one grades honest Proxy:
    // right body, right cost, inert text.
    dropped.push(face.oracle_text.clone());
    (cf, CoverageTier::Proxy, dropped)
}

/// Does land text contain anything beyond tapped-clauses and add-mana lines?
fn land_residual_text(lower: &str) -> bool {
    for line in lower.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let is_mana = line.contains("add ") && line.starts_with("{t}");
        let is_tapped = line.contains("enters the battlefield tapped")
            || line.contains("enters tapped");
        if !is_mana && !is_tapped {
            return true;
        }
    }
    false
}

/// Basic Forest and friends for tests and padding.
pub fn basic_land_production(subtype: &str) -> Option<ManaProduction> {
    let set = match subtype {
        "plains" => ColorSet::W,
        "island" => ColorSet::U,
        "swamp" => ColorSet::B,
        "mountain" => ColorSet::R,
        "forest" => ColorSet::G,
        "wastes" => ColorSet::C,
        _ => return None,
    };
    Some(ManaProduction::AnyOneOf(set))
}

// Silence the unused warning until the template stage lands.
#[allow(dead_code)]
fn placeholder_value() -> ValueExpr {
    ValueExpr::ONE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_stripping() {
        assert_eq!(strip_reminders("Flying (It can fly.)").trim(), "Flying");
    }

    #[test]
    fn ward_parses() {
        let w = parse_ward("Ward {2}").unwrap();
        assert_eq!(w.generic, 2);
    }
}
