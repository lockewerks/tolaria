//! Play-weighted coverage: what fraction of real tournament card-slots the
//! compiler actually models, per format. Raw pool percentages are dominated
//! by draft chaff nobody sleeves; this is the honest headline number, and
//! both always print together so neither can be cherry-picked.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use mtg_data::{CardId, CardPool};
use mtg_ir::CoverageTier;
use serde::{Deserialize, Serialize};

/// A most-played card the compiler does not fully model.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CardGap {
    pub name: String,
    pub copies: u64,
    pub tier: String,
    pub dropped: Vec<String>,
}

/// A dropped-clause bucket weighted by real play.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WeightedClauseGap {
    pub pattern: String,
    /// Distinct played cards contributing to the bucket.
    pub cards: u32,
    /// Copies across all cached decklists in the window.
    pub meta_copies: u64,
    pub example_cards: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MetaCoverage {
    pub format: String,
    pub window_days: i64,
    /// Tournament decks in the window.
    pub decks: u32,
    /// Main-deck card-slots, copies-weighted, including unresolved names.
    pub total_copies: u64,
    /// Copies whose names the pool could not resolve. Disclosed, never
    /// silently dropped from the denominator story.
    pub unresolved_copies: u64,
    pub full_copies: u64,
    pub partial_copies: u64,
    pub proxy_copies: u64,
    pub unplayable_copies: u64,
    /// Most-played cards below Full, worst offenders first.
    pub top_card_gaps: Vec<CardGap>,
    /// Dropped-clause buckets ranked by played copies: the template
    /// backlog in priority order.
    pub clause_gaps: Vec<WeightedClauseGap>,
}

impl MetaCoverage {
    pub fn resolved_copies(&self) -> u64 {
        self.full_copies + self.partial_copies + self.proxy_copies + self.unplayable_copies
    }

    /// The headline: Full+Partial over resolved played copies.
    pub fn playable_frac(&self) -> f64 {
        (self.full_copies + self.partial_copies) as f64 / self.resolved_copies().max(1) as f64
    }

    pub fn full_frac(&self) -> f64 {
        self.full_copies as f64 / self.resolved_copies().max(1) as f64
    }

    pub fn unresolved_frac(&self) -> f64 {
        self.unresolved_copies as f64 / self.total_copies.max(1) as f64
    }
}

/// Copies-weighted play counts per card name from the cached tournament
/// decklists. Returns (deck count, lowercase name -> copies).
pub fn card_play_counts(
    cache_dir: &Path,
    format: &str,
    days: i64,
) -> Result<(u32, HashMap<String, u64>)> {
    let decks = mtg_sources::tournaments::load_decks(cache_dir, format, days)?;
    let mut counts: HashMap<String, u64> = HashMap::new();
    for d in &decks {
        for (name, n) in &d.main {
            *counts.entry(name.to_lowercase()).or_insert(0) += u64::from(*n);
        }
    }
    Ok((decks.len() as u32, counts))
}

/// Compute play-weighted coverage for a format from the local tournament
/// cache. Purely offline: errors if nothing is cached rather than fetching.
pub fn meta_coverage(pool: &CardPool, format_str: &str, days: i64) -> Result<MetaCoverage> {
    let format = mtg_data::Format::parse(format_str)
        .ok_or_else(|| anyhow::anyhow!("unknown format: {format_str}"))?;
    let paths = mtg_data::Paths::resolve()?;
    let cache_dir = paths.meta_dir().join("fbettega");
    let (decks, counts) = card_play_counts(&cache_dir, &format.to_string(), days)?;
    if decks == 0 {
        anyhow::bail!(
            "no cached tournament decks for {format} in the last {days} days; \
             run `tolaria fetch-meta --format {format}` first"
        );
    }

    let mut cov = MetaCoverage {
        format: format.to_string(),
        window_days: days,
        decks,
        total_copies: 0,
        unresolved_copies: 0,
        full_copies: 0,
        partial_copies: 0,
        proxy_copies: 0,
        unplayable_copies: 0,
        top_card_gaps: Vec::new(),
        clause_gaps: Vec::new(),
    };

    struct ClauseAcc {
        cards: u32,
        meta_copies: u64,
        examples: Vec<String>,
    }
    let mut compiled: HashMap<CardId, (CoverageTier, Vec<String>)> = HashMap::new();
    let mut clause_map: HashMap<String, ClauseAcc> = HashMap::new();

    for (name, copies) in &counts {
        cov.total_copies += copies;
        let Some(id) = pool.lookup(name) else {
            cov.unresolved_copies += copies;
            continue;
        };
        let (tier, dropped) = compiled
            .entry(id)
            .or_insert_with(|| {
                let cc = mtg_cards::compile(pool.get(id));
                (cc.tier, cc.dropped.iter().map(|d| d.to_string()).collect())
            })
            .clone();
        match tier {
            CoverageTier::Full => cov.full_copies += copies,
            CoverageTier::Partial => cov.partial_copies += copies,
            CoverageTier::Proxy => cov.proxy_copies += copies,
            CoverageTier::Unplayable => cov.unplayable_copies += copies,
        }
        if tier < CoverageTier::Full {
            let display = pool.get(id).name.to_string();
            cov.top_card_gaps.push(CardGap {
                name: display.clone(),
                copies: *copies,
                tier: format!("{tier:?}"),
                dropped: dropped.clone(),
            });
            let mut seen: Vec<String> = Vec::new();
            for clause in &dropped {
                let pattern = mtg_cards::gaps::normalize_gap_pattern(clause);
                if seen.contains(&pattern) {
                    continue;
                }
                seen.push(pattern.clone());
                let acc = clause_map.entry(pattern).or_insert_with(|| ClauseAcc {
                    cards: 0,
                    meta_copies: 0,
                    examples: Vec::new(),
                });
                acc.cards += 1;
                acc.meta_copies += copies;
                if acc.examples.len() < 3 {
                    acc.examples.push(display.clone());
                }
            }
        }
    }

    cov.top_card_gaps.sort_by(|a, b| b.copies.cmp(&a.copies).then_with(|| a.name.cmp(&b.name)));
    cov.top_card_gaps.truncate(40);
    cov.clause_gaps = clause_map
        .into_iter()
        .map(|(pattern, a)| WeightedClauseGap {
            pattern,
            cards: a.cards,
            meta_copies: a.meta_copies,
            example_cards: a.examples,
        })
        .collect();
    cov.clause_gaps
        .sort_by(|a, b| b.meta_copies.cmp(&a.meta_copies).then_with(|| a.pattern.cmp(&b.pattern)));
    cov.clause_gaps.truncate(100);
    Ok(cov)
}
