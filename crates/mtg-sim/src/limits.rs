//! The divergence ledger, assembled. Each lower crate owns the limits for
//! its own behavior; this concatenates them and adds the statistics and
//! meta-construction caveats that live at the harness layer.

use mtg_ir::{Limit, LimitCategory::{Meta, Statistics}};

/// Limits owned by the harness: how numbers are computed and how the
/// opposing field is built.
pub const LIMITS: &[Limit] = &[
    Limit {
        id: "stats.draws-half-win",
        category: Statistics,
        rule_ref: "-",
        summary: "draws count as half a win for both sides in every win rate and confidence interval",
        impact: "matchups that draw often are pulled toward 50% rather than reported as unresolved",
    },
    Limit {
        id: "stats.panic-sample-shrink",
        category: Statistics,
        rule_ref: "-",
        summary: "a game that panics the engine is dropped from the sample (its count is reported separately)",
        impact: "a deterministically panicking card silently shrinks the sample instead of failing loudly",
    },
    Limit {
        id: "meta.consensus-list",
        category: Meta,
        rule_ref: "-",
        summary: "each opposing archetype is one consensus list built from its real decklists, min 3 lists",
        impact: "within-archetype list variance is collapsed, so a matchup is one point estimate, not a spread",
    },
    Limit {
        id: "meta.share-by-frequency",
        category: Meta,
        rule_ref: "-",
        summary: "meta share weights archetypes by how often they appear, not by how they placed",
        impact: "a popular but losing deck weighs as much as its play rate, not its win rate",
    },
];

/// Every limit across the simulator, lowest layer first.
pub fn all_limits() -> Vec<&'static Limit> {
    mtg_engine::LIMITS
        .iter()
        .chain(mtg_cards::LIMITS)
        .chain(mtg_ai::LIMITS)
        .chain(LIMITS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::all_limits;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for l in all_limits() {
            assert!(seen.insert(l.id), "duplicate limit id: {}", l.id);
        }
    }

    #[test]
    fn ledger_is_populated() {
        assert!(all_limits().len() >= 15);
    }
}
