//! Builds the trust report from a finished run: coverage with tier counts
//! both sides, pilot flags, cap-forced draws, panics, and the derived
//! warnings. One builder feeds the CLI, the desktop, and every export.

use mtg_data::CardPool;
use mtg_ir::CoverageTier;
use mtg_stats::trust::{
    DeckTrust, DroppedCard, RenderedWarning, TierCounts, TrustReport, Warning, CI_METHOD,
    SCHEMA_VERSION,
};
use mtg_stats::MatchupStats;

use crate::SimDeck;

/// Coverage tiers and dropped-clause detail for one deck. `full_detail`
/// collects per-card dropped clauses (do this for the user's deck, skip it
/// for opponents to keep reports small).
pub fn deck_trust(pool: &CardPool, deck: &SimDeck, full_detail: bool) -> DeckTrust {
    let mut tiers = TierCounts::default();
    let mut dropped = Vec::new();
    let mut list = Vec::new();
    for &(cid, count) in &deck.cards {
        let card = pool.get(cid);
        let compiled = mtg_cards::compile(card);
        for _ in 0..count {
            match compiled.tier {
                CoverageTier::Full => tiers.full += 1,
                CoverageTier::Partial => tiers.partial += 1,
                CoverageTier::Proxy => tiers.proxy += 1,
                CoverageTier::Unplayable => tiers.unplayable += 1,
            }
        }
        list.push((card.name.to_string(), count));
        if full_detail && !compiled.dropped.is_empty() {
            dropped.push(DroppedCard {
                name: card.name.to_string(),
                count,
                tier: format!("{:?}", compiled.tier),
                clauses: compiled.dropped.iter().map(|d| d.to_string()).collect(),
            });
        }
    }
    let total = tiers.total().max(1) as f64;
    let difficulty = crate::pilot::pilot_difficulty(pool, &deck.cards);
    DeckTrust {
        name: deck.name.clone(),
        coverage_full_frac: tiers.full as f64 / total,
        coverage_playable_frac: tiers.playable() as f64 / total,
        pilot_warning: deck.pilot_warning,
        pilot_grade: Some(difficulty.grade),
        pilot_factors: difficulty.factors,
        dropped,
        list,
        tiers,
    }
}

/// Assemble the full report. `matchups` is every simulated matchup (one
/// for duel/pod, many for a gauntlet); `panics_override` lets goldfish
/// pass its own panic count since it has no MatchupStats.
pub fn build_trust_report(
    pool: &CardPool,
    user: &SimDeck,
    opponents: &[&SimDeck],
    matchups: &[MatchupStats],
    cfg: &crate::SimConfig,
    gauntlet_seeded: bool,
    panics_override: Option<u32>,
) -> TrustReport {
    let user_deck = deck_trust(pool, user, true);
    let opp_trust: Vec<DeckTrust> = opponents.iter().map(|o| deck_trust(pool, o, false)).collect();

    let early_stopped_matchups = matchups.iter().filter(|m| m.stopped_early).count() as u32;
    let panics = panics_override.unwrap_or_else(|| matchups.iter().map(|m| m.panics).sum());
    let turn_cap_draws: u32 = matchups.iter().map(|m| m.turn_cap_draws).sum();
    let decision_cap_draws: u32 = matchups.iter().map(|m| m.decision_cap_draws).sum();
    let total_games: u32 = matchups.iter().map(|m| m.games).sum();

    let mut report = TrustReport {
        schema_version: SCHEMA_VERSION,
        tolaria_version: env!("CARGO_PKG_VERSION").to_string(),
        compiler_version: mtg_cards::COMPILER_VERSION,
        ci_method: CI_METHOD.to_string(),
        seed: cfg.master_seed,
        gauntlet_seeded,
        user_deck,
        opponents: opp_trust,
        early_stopped_matchups,
        panics,
        turn_cap_draws,
        decision_cap_draws,
        turn_cap: cfg.rules.turn_cap,
        decision_cap: cfg.rules.decision_cap,
        total_games,
        warnings: Vec::new(),
        calibration: None,
    };
    report.warnings = standard_warnings(&report).iter().map(Warning::render).collect();
    report
}

/// Derive the caveats a report should raise from its own contents.
pub fn standard_warnings(report: &TrustReport) -> Vec<Warning> {
    let mut out = Vec::new();

    if report.user_deck.coverage_playable_frac < 0.85 {
        out.push(Warning::LowOwnCoverage { playable: report.user_deck.coverage_playable_frac });
    }
    let proxy_plus = report.user_deck.tiers.proxy + report.user_deck.tiers.unplayable;
    let total = report.user_deck.tiers.total().max(1);
    if proxy_plus as f64 / total as f64 >= 0.10 {
        out.push(Warning::ProxyHeavyOwnDeck {
            count: proxy_plus,
            frac: proxy_plus as f64 / total as f64,
        });
    }
    if report.user_deck.pilot_warning {
        out.push(Warning::OwnPilotFidelity { creatures: report.user_deck.tiers.total() });
    }

    if !report.opponents.is_empty() {
        let avg: f64 = report.opponents.iter().map(|o| o.coverage_playable_frac).sum::<f64>()
            / report.opponents.len() as f64;
        if avg < 0.85 {
            out.push(Warning::LowOppCoverage { avg_playable: avg });
        }
        let flagged: Vec<String> = report
            .opponents
            .iter()
            .filter(|o| o.pilot_warning)
            .map(|o| o.name.clone())
            .collect();
        if !flagged.is_empty() {
            out.push(Warning::OppPilotFidelity { archetypes: flagged });
        }
    }

    if report.early_stopped_matchups > 0 {
        out.push(Warning::EarlyStopped { matchups: report.early_stopped_matchups });
    }
    if report.panics > 0 {
        out.push(Warning::Panics { games: report.panics });
    }
    let cap_draws = report.turn_cap_draws + report.decision_cap_draws;
    if report.total_games > 0 && cap_draws as f64 / report.total_games as f64 > 0.02 {
        out.push(Warning::CapForcedDraws {
            draws: cap_draws,
            frac: cap_draws as f64 / report.total_games as f64,
        });
    }
    out
}

/// Render an already-built warning list for callers that only have the
/// rendered form (mirrors the CLI/UI split).
pub fn render_all(warnings: &[Warning]) -> Vec<RenderedWarning> {
    warnings.iter().map(Warning::render).collect()
}
