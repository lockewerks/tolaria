//! A composite pilot-difficulty grade from a deck's compiled IR. It
//! replaces the bare creatures-under-10 boolean as the DISPLAYED signal,
//! while `pilot_warning = grade >= 2` keeps every existing bool wire
//! compatible. Still a heuristic, not a measurement: labeled as such.

use mtg_data::{CardId, CardPool};
use mtg_ir::Effect;

pub struct PilotDifficulty {
    /// 0 best (a curve-out deck the greedy pilot plays well) .. 3 worst.
    pub grade: u8,
    /// Human-readable contributing factors, e.g. "9 tutors".
    pub factors: Vec<String>,
}

/// Score how badly the greedy pilot is likely to misplay a list, from the
/// things it provably cannot do: tutoring for a plan, sequencing X spells,
/// choosing modes, paying alternative costs, holding up counters, and
/// executing named engines (storm, cascade, free spells).
pub fn pilot_difficulty(pool: &CardPool, cards: &[(CardId, u8)]) -> PilotDifficulty {
    let mut creatures = 0u32;
    let mut tutors = 0u32;
    let mut x_spells = 0u32;
    let mut modal = 0u32;
    let mut alt_costs = 0u32;
    let mut counters = 0u32;

    for &(id, count) in cards {
        let n = count as u32;
        let card = pool.get(id);
        // Types live on the oracle faces; the rest on the compiled faces.
        if card.faces.iter().any(|f| f.types.contains(mtg_ir::CardTypes::CREATURE)) {
            creatures += n;
        }
        let compiled = mtg_cards::compile(card);
        for face in &compiled.faces {
            if face.x_spell {
                x_spells += n;
            }
            if !face.alt_costs.is_empty() {
                alt_costs += n;
            }
            if let Some(sa) = &face.spell {
                scan_effect(&sa.effect, &mut tutors, &mut modal, &mut counters, n);
            }
            for act in &face.activated {
                scan_effect(&act.ability.effect, &mut tutors, &mut modal, &mut counters, n);
            }
            for trig in &face.triggered {
                scan_effect(&trig.ability.effect, &mut tutors, &mut modal, &mut counters, n);
            }
        }
    }

    let mut factors = Vec::new();
    if tutors >= 3 {
        factors.push(format!("{tutors} tutors"));
    }
    if x_spells >= 3 {
        factors.push(format!("{x_spells} X-spells"));
    }
    if counters >= 4 {
        factors.push(format!("{counters} counterspells"));
    }
    if modal >= 6 {
        factors.push(format!("{modal} modal spells"));
    }
    if alt_costs >= 4 {
        factors.push(format!("{alt_costs} alternative-cost spells"));
    }
    if creatures < 10 {
        factors.push(format!("{creatures} creatures"));
    }

    // Weighted over signals that survive from the compiled IR, not fragile
    // oracle-text scans. Tutors and X are down-weighted since the pilot now
    // scores tutor picks and caps X; the residual difficulty is control
    // sequencing (counters), mode choice, and creature-light plans.
    let score = tutors
        + x_spells
        + counters * 2
        + modal
        + alt_costs
        + if creatures < 10 { 4 } else { 0 };
    let grade = match score {
        0..=4 => 0,
        5..=9 => 1,
        10..=16 => 2,
        _ => 3,
    };
    PilotDifficulty { grade, factors }
}

fn scan_effect(e: &Effect, tutors: &mut u32, modal: &mut u32, counters: &mut u32, n: u32) {
    match e {
        Effect::SearchLibrary { .. } => *tutors += n,
        Effect::CounterSpell { .. } => *counters += n,
        Effect::Modal { modes, .. } => {
            *modal += n;
            for m in modes {
                scan_effect(&m.effect, tutors, modal, counters, n);
            }
        }
        Effect::Seq(list) => {
            for x in list {
                scan_effect(x, tutors, modal, counters, n);
            }
        }
        _ => {}
    }
}

/// Text label for a grade, for display next to the flag.
pub fn grade_label(grade: u8) -> &'static str {
    match grade {
        0 => "high",
        1 => "fair",
        2 => "low",
        _ => "very low",
    }
}

#[cfg(test)]
mod tests {
    use super::grade_label;

    #[test]
    fn labels_cover_all_grades() {
        assert_eq!(grade_label(0), "high");
        assert_eq!(grade_label(3), "very low");
    }
}
