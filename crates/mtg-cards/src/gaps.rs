//! Aggregation of dropped clauses across a pool compilation: the ranked
//! backlog of template work. Buckets are deliberately modest so a bucket
//! maps to one template fix; targets stay distinct on purpose.

use std::collections::HashMap;

use crate::compiler::PoolCompilation;
use mtg_data::CardPool;

pub struct ClauseGap {
    /// Normalized bucket key.
    pub pattern: String,
    /// Distinct cards with at least one clause in this bucket.
    pub cards: u32,
    /// Up to three example card names.
    pub example_cards: Vec<String>,
    /// One raw clause as compiled text, so the pattern stays legible.
    pub example_text: String,
}

/// Bucket key for a dropped clause: digit runs and count words become `n`,
/// every run of mana/tap symbol groups becomes `{m}`. The clause text is
/// already lowercased, reminder-stripped, and self-named `~` upstream.
pub fn normalize_gap_pattern(clause: &str) -> String {
    let mut collapsed = String::with_capacity(clause.len());
    let mut rest = clause;
    while let Some(open) = rest.find('{') {
        collapsed.push_str(&rest[..open]);
        match rest[open..].find('}') {
            Some(close) => {
                if !collapsed.ends_with("{m}") {
                    collapsed.push_str("{m}");
                }
                rest = &rest[open + close + 1..];
            }
            None => {
                collapsed.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    collapsed.push_str(rest);

    let mut out = String::with_capacity(collapsed.len());
    for (i, w) in collapsed.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let trail_len = w.chars().rev().take_while(|c| ",.;:!?\"'".contains(*c)).count();
        let core = &w[..w.len() - trail_len];
        let is_count = !core.is_empty()
            && (core.chars().all(|c| c.is_ascii_digit())
                || crate::text::parse_count_word(core).is_some());
        if is_count {
            out.push('n');
        } else {
            out.push_str(core);
        }
        out.push_str(&w[w.len() - trail_len..]);
    }
    out
}

/// Bucket every dropped clause in the compilation, most-common first.
pub fn aggregate_gaps(pool: &CardPool, comp: &PoolCompilation) -> Vec<ClauseGap> {
    struct Acc {
        cards: u32,
        examples: Vec<String>,
        example_text: String,
    }
    let mut map: HashMap<String, Acc> = HashMap::new();
    for (id, _tier, dropped) in &comp.cards {
        if dropped.is_empty() {
            continue;
        }
        let name = pool.get(*id).name.to_string();
        let mut seen: Vec<String> = Vec::new();
        for clause in dropped {
            let pattern = normalize_gap_pattern(clause);
            if seen.contains(&pattern) {
                continue;
            }
            seen.push(pattern.clone());
            let acc = map.entry(pattern).or_insert_with(|| Acc {
                cards: 0,
                examples: Vec::new(),
                example_text: clause.to_string(),
            });
            acc.cards += 1;
            if acc.examples.len() < 3 {
                acc.examples.push(name.clone());
            }
        }
    }
    let mut out: Vec<ClauseGap> = map
        .into_iter()
        .map(|(pattern, a)| ClauseGap {
            pattern,
            cards: a.cards,
            example_cards: a.examples,
            example_text: a.example_text,
        })
        .collect();
    out.sort_by(|a, b| b.cards.cmp(&a.cards).then_with(|| a.pattern.cmp(&b.pattern)));
    out
}

#[cfg(test)]
mod tests {
    use super::normalize_gap_pattern;

    #[test]
    fn digits_and_count_words_bucket_together() {
        assert_eq!(
            normalize_gap_pattern("draw two cards."),
            normalize_gap_pattern("draw 3 cards.")
        );
        assert_eq!(normalize_gap_pattern("draw a card."), "draw n card.");
    }

    #[test]
    fn mana_symbol_runs_collapse() {
        assert_eq!(
            normalize_gap_pattern("add {r}{r}{r}."),
            normalize_gap_pattern("add {g}.")
        );
        assert_eq!(normalize_gap_pattern("{t}: add {c}."), "{m}: add {m}.");
    }

    #[test]
    fn unrelated_words_stay_distinct() {
        assert_ne!(
            normalize_gap_pattern("destroy target artifact."),
            normalize_gap_pattern("destroy target enchantment.")
        );
    }
}
