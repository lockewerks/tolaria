//! Meta computation: archetype shares from tournament frequency and
//! consensus decklists per archetype.

use std::collections::HashMap;

use crate::archetypes::{classify, FormatRules};
use crate::tournaments::TournamentDeck;

#[derive(Debug, Clone)]
pub struct MetaDeck {
    pub archetype: String,
    pub share: f64,
    pub sample_size: usize,
    pub main: Vec<(String, u8)>,
}

/// Build the top-N meta from classified tournament decks: shares from
/// archetype frequency, one consensus list per archetype.
pub fn build_meta(
    rules: &FormatRules,
    decks: &[TournamentDeck],
    top_n: usize,
) -> Vec<MetaDeck> {
    let mut buckets: HashMap<String, Vec<&TournamentDeck>> = HashMap::new();
    let mut classified_total = 0usize;
    for d in decks {
        if let Some(name) = classify(rules, &d.main, &d.side) {
            buckets.entry(name).or_default().push(d);
            classified_total += 1;
        }
    }
    if classified_total == 0 {
        return Vec::new();
    }
    let mut ranked: Vec<(String, Vec<&TournamentDeck>)> = buckets.into_iter().collect();
    ranked.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    ranked.truncate(top_n);

    ranked
        .into_iter()
        .map(|(name, lists)| {
            let share = lists.len() as f64 / classified_total as f64;
            let main = consensus_list(&lists);
            MetaDeck { archetype: name, share, sample_size: lists.len(), main }
        })
        .collect()
}

/// Median-count consensus: cards present in at least half the lists at
/// their median count, padded and trimmed to the modal deck size.
fn consensus_list(lists: &[&TournamentDeck]) -> Vec<(String, u8)> {
    let n = lists.len().max(1);
    let mut counts: HashMap<String, Vec<u8>> = HashMap::new();
    let mut display: HashMap<String, String> = HashMap::new();
    for deck in lists {
        // Merge duplicate lines within one list first.
        let mut merged: HashMap<String, u8> = HashMap::new();
        for (name, c) in &deck.main {
            *merged.entry(name.to_ascii_lowercase()).or_insert(0) += *c;
            display.entry(name.to_ascii_lowercase()).or_insert_with(|| name.clone());
        }
        for (k, c) in merged {
            counts.entry(k).or_default().push(c);
        }
    }
    // Modal deck size (60 for constructed, 40 for limited dumps).
    let mut sizes: Vec<usize> = lists
        .iter()
        .map(|d| d.main.iter().map(|(_, c)| *c as usize).sum())
        .collect();
    sizes.sort_unstable();
    let target = sizes.get(sizes.len() / 2).copied().unwrap_or(60);

    struct Cand {
        key: String,
        med: u8,
        freq: usize,
        avg: f64,
    }
    let mut cands: Vec<Cand> = counts
        .into_iter()
        .map(|(key, mut v)| {
            v.sort_unstable();
            let med = v[v.len() / 2];
            let freq = v.len();
            let avg = v.iter().map(|&c| c as f64).sum::<f64>() / n as f64;
            Cand { key, med, freq, avg }
        })
        .collect();
    cands.sort_by(|a, b| {
        b.freq
            .cmp(&a.freq)
            .then(b.avg.partial_cmp(&a.avg).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut out: Vec<(String, u8)> = Vec::new();
    let mut total = 0usize;
    // Core: cards in at least half the lists.
    for c in &cands {
        if c.freq * 2 >= n && total < target {
            let take = (c.med as usize).min(target - total).max(1) as u8;
            out.push((display[&c.key].clone(), take));
            total += take as usize;
        }
    }
    // Pad from the remaining most-frequent cards.
    for c in &cands {
        if total >= target {
            break;
        }
        if c.freq * 2 < n {
            let take = (c.med as usize).min(target - total).max(1) as u8;
            out.push((display[&c.key].clone(), take));
            total += take as usize;
        }
    }
    out
}

/// Crude pilot-fidelity heuristic: decks with very few creatures lean on
/// lines a greedy agent cannot pilot.
pub fn pilot_warning(main_creature_count: u32) -> bool {
    main_creature_count < 10
}
