//! The behavior compiler: keyword short-circuit, payload keywords, the
//! template bank, and coverage grading. Overrides pre-empt all of it.

use mtg_data::{OracleCard, OracleFace};
use mtg_ir::{
    AbilityCost, CardTypes, ColorSet, CompiledCard, CompiledFace, CoverageTier, KeywordSet,
    ManaAbility, ManaCost, ManaProduction,
};

pub const COMPILER_VERSION: u16 = 2;

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

fn parse_ward(text: &str) -> Option<ManaCost> {
    let idx = text.find("ward {")?;
    let rest = &text[idx + 5..];
    let end = rest
        .find(|c: char| c != '{' && c != '}' && !c.is_ascii_alphanumeric() && c != '/')
        .unwrap_or(rest.len());
    ManaCost::parse(&rest[..end].to_ascii_uppercase())
}

fn parse_toxic(text: &str) -> u8 {
    if let Some(idx) = text.find("toxic ") {
        text[idx + 6..]
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
    let mut out = ColorSet::empty();
    for (name, set) in [
        ("protection from white", ColorSet::W),
        ("protection from blue", ColorSet::U),
        ("protection from black", ColorSet::B),
        ("protection from red", ColorSet::R),
        ("protection from green", ColorSet::G),
    ] {
        if text.contains(name) {
            out |= set;
        }
    }
    if text.contains("protection from all colors") || text.contains("protection from each color") {
        out = ColorSet::W | ColorSet::U | ColorSet::B | ColorSet::R | ColorSet::G;
    }
    out
}

/// Compile one card.
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

    let text = crate::text::normalize(&face.oracle_text, &card.name, &face.name);
    cf.ward = parse_ward(&text);
    cf.toxic = parse_toxic(&text);
    cf.protection_from = parse_protection(&text);

    let is_land = face.types.contains(CardTypes::LAND);
    if is_land {
        if !card.produced_mana.is_empty() {
            cf.mana_abilities.push(ManaAbility {
                cost: AbilityCost::tap(),
                produce: ManaProduction::AnyOneOf(card.produced_mana),
            });
        }
    } else {
        match ManaCost::parse(&face.mana_cost) {
            Some(cost) => {
                cf.x_spell = cost.x_count > 0;
                cf.cost = Some(cost);
            }
            None => return (cf, CoverageTier::Unplayable, dropped),
        }
        // Back faces of transforming cards are reached by transforming, not
        // casting.
        if face.mana_cost.is_empty() && card.faces.len() > 1 {
            cf.cost = None;
        }
    }

    if text.trim().is_empty() {
        return (cf, CoverageTier::Full, dropped);
    }

    let outcome = crate::templates::parse_face(&text, &mut cf, face.types);
    for u in &outcome.unmatched {
        dropped.push(u.as_str().into());
    }

    let is_spell_face =
        face.types.contains(CardTypes::INSTANT) || face.types.contains(CardTypes::SORCERY);

    let tier = if outcome.unmatched.is_empty() {
        if is_spell_face && cf.spell.is_none() {
            // Nothing unmatched but no effect either: text was all
            // mechanic/keyword lines on a spell we cannot resolve.
            CoverageTier::Unplayable
        } else {
            CoverageTier::Full
        }
    } else if is_spell_face {
        if cf.spell.is_some() {
            CoverageTier::Partial
        } else {
            // A spell whose resolution we cannot model must not be cast.
            CoverageTier::Unplayable
        }
    } else if outcome.matched_lines > 0 || (is_land && !cf.mana_abilities.is_empty()) {
        // A land that makes its mana is functionally present even when its
        // rules text is not modeled.
        CoverageTier::Partial
    } else {
        CoverageTier::Proxy
    };

    (cf, tier, dropped)
}

/// A whole-pool compilation keeping the per-card detail the coverage and
/// gap reports need, not just the tier histogram.
pub struct PoolCompilation {
    pub stats: CoverageStats,
    /// (card id, tier, dropped clauses) in pool iteration order.
    pub cards: Vec<(mtg_data::CardId, CoverageTier, Vec<Box<str>>)>,
}

/// Compile every pool card passing the filter, in parallel. The filter
/// carves format-legal subsets without a second entry point.
pub fn compile_pool_detailed(
    pool: &mtg_data::CardPool,
    filter: impl Fn(&OracleCard) -> bool + Sync,
) -> PoolCompilation {
    use rayon::prelude::*;
    let cards: Vec<(mtg_data::CardId, &OracleCard)> =
        pool.iter().filter(|(_, c)| filter(c)).collect();
    let compiled: Vec<(mtg_data::CardId, CoverageTier, Vec<Box<str>>)> = cards
        .par_iter()
        .map(|(id, c)| {
            let cc = compile(c);
            (*id, cc.tier, cc.dropped)
        })
        .collect();
    let mut stats = CoverageStats::default();
    for (_, t, _) in &compiled {
        match t {
            CoverageTier::Full => stats.full += 1,
            CoverageTier::Partial => stats.partial += 1,
            CoverageTier::Proxy => stats.proxy += 1,
            CoverageTier::Unplayable => stats.unplayable += 1,
        }
    }
    PoolCompilation { stats, cards: compiled }
}

/// Compile the whole pool in parallel; returns per-tier counts.
pub fn compile_pool(pool: &mtg_data::CardPool) -> CoverageStats {
    compile_pool_detailed(pool, |_| true).stats
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CoverageStats {
    pub full: usize,
    pub partial: usize,
    pub proxy: usize,
    pub unplayable: usize,
}

impl CoverageStats {
    pub fn total(&self) -> usize {
        self.full + self.partial + self.proxy + self.unplayable
    }
}

/// Basic land production for tests and deck padding.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_stripping() {
        assert_eq!(strip_reminders("Flying (It can fly.)").trim(), "Flying");
    }

    #[test]
    fn ward_parses() {
        let w = parse_ward("ward {2}").unwrap();
        assert_eq!(w.generic, 2);
    }
}
