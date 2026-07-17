//! Calibration: simulated matchup win rates against real tournament match
//! results from the local cache. This is the accuracy yardstick; every
//! report carries its own skip-counts and caveats because the comparison
//! has known structural biases that honesty requires printing, not fixing
//! silently.

use std::collections::HashMap;

use anyhow::Result;
use mtg_data::CardPool;
use serde::{Deserialize, Serialize};

use crate::meta_loader::{creature_count, ensure_meta_sources, MIN_LISTS};
use crate::{run_matchup, MatchupProgress, SimConfig, SimDeck};

/// One archetype pair: real record vs simulated record.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PairCalibration {
    pub a: String,
    pub b: String,
    pub real_games: u32,
    pub real_a_wins: u32,
    pub real_draws: u32,
    /// Real game win rate for A, draws as half wins.
    pub real_wr: f64,
    pub real_ci: (f64, f64),
    pub sim_games: u32,
    pub sim_wr: f64,
    pub sim_ci: (f64, f64),
    /// Signed sim minus real.
    pub divergence: f64,
    pub ci_overlap: bool,
    pub a_lists: usize,
    pub b_lists: usize,
    pub a_coverage_playable: f64,
    pub b_coverage_playable: f64,
    pub a_pilot_warning: bool,
    pub b_pilot_warning: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CalibrationReport {
    pub format: String,
    pub window_days: i64,
    pub generated_unix: u64,
    pub master_seed: u64,
    pub compiler_version: u16,
    pub min_games: u32,
    pub tournaments: u32,
    pub tournaments_with_rounds: u32,
    pub matches_total: u32,
    pub matches_used: u32,
    pub matches_skipped_bye: u32,
    pub matches_skipped_unjoined: u32,
    pub matches_skipped_unclassified: u32,
    pub matches_skipped_malformed: u32,
    pub matches_skipped_mirror: u32,
    pub draw_only_matches: u32,
    pub pairs: Vec<PairCalibration>,
    /// |sim - real| averaged over pairs, weighted by real game count.
    pub mean_abs_divergence: f64,
    /// Pearson r between real and sim win rates across pairs.
    pub correlation: f64,
    /// Structural biases of the comparison, serialized verbatim so every
    /// consumer of the numbers also gets the reasons to doubt them.
    pub caveats: Vec<String>,
}

/// Per-archetype trust signal derived from a report: how far the sim ran
/// from reality on pairs involving this archetype.
#[derive(Serialize, Clone, Debug)]
pub struct TrustSignal {
    pub pairs: u32,
    pub real_games: u32,
    pub mean_abs_divergence: f64,
}

pub const CAVEATS: &[&str] = &[
    "real results are best-of-3 games including sideboarded games 2 and 3; the sim plays game 1 with no sideboarding",
    "real data does not record play/draw per game; the sim alternates strictly, and aggregation across play/draw can shift win rates",
    "the sim pilots one consensus list per archetype; real archetypes contain list variance, and consensus-vs-consensus need not equal the average of list-vs-list",
    "both sim seats are the same greedy agent; real pilot skill varies and correlates with deck choice",
    "elimination rounds overweight winning players and decks",
    "byes and matches with no finished games (intentional draws) are excluded; game draws count as half wins for both sides",
    "games within one match share players and lists, so real confidence intervals are slightly narrower than independent samples would give",
];

struct PairRecord {
    a_wins: u32,
    b_wins: u32,
    draws: u32,
    matches: u32,
}

/// Parse "W-L-D" into (w, l, d).
fn parse_result(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.trim().split('-');
    let w = it.next()?.trim().parse().ok()?;
    let l = it.next()?.trim().parse().ok()?;
    let d = it.next().unwrap_or("0").trim().parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((w, l, d))
}

pub fn run_calibration(
    pool: &CardPool,
    format_str: &str,
    days: i64,
    min_games: u32,
    seed: u64,
    status: &mut dyn FnMut(String),
) -> Result<CalibrationReport> {
    let format = mtg_data::Format::parse(format_str)
        .ok_or_else(|| anyhow::anyhow!("unknown format: {format_str}"))?;
    if format == mtg_data::Format::Commander {
        anyhow::bail!("calibration needs 1v1 match results; commander pods have none cached");
    }
    let (cache_dir, rules_dir) = ensure_meta_sources(days, status)?;
    let rules = mtg_sources::archetypes::load_rules(&rules_dir, format)?;
    let records =
        mtg_sources::tournaments::load_tournaments(&cache_dir, &format.to_string(), days)?;

    let mut report = CalibrationReport {
        format: format.to_string(),
        window_days: days,
        generated_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        master_seed: seed,
        compiler_version: mtg_cards::COMPILER_VERSION,
        min_games,
        tournaments: records.len() as u32,
        tournaments_with_rounds: 0,
        matches_total: 0,
        matches_used: 0,
        matches_skipped_bye: 0,
        matches_skipped_unjoined: 0,
        matches_skipped_unclassified: 0,
        matches_skipped_malformed: 0,
        matches_skipped_mirror: 0,
        draw_only_matches: 0,
        pairs: Vec::new(),
        mean_abs_divergence: 0.0,
        correlation: 0.0,
        caveats: CAVEATS.iter().map(|s| s.to_string()).collect(),
    };

    // Real archetype-pair records, joined per tournament because player
    // names are only unique within one event.
    let mut pair_records: HashMap<(String, String), PairRecord> = HashMap::new();
    let mut all_decks: Vec<mtg_sources::tournaments::TournamentDeck> = Vec::new();

    for rec in &records {
        if rec.rounds.is_empty() {
            continue;
        }
        report.tournaments_with_rounds += 1;
        // player -> archetype for this event; classification is cached per
        // player, not per match.
        let mut by_player: HashMap<&str, Option<String>> = HashMap::new();
        for d in &rec.decks {
            if d.mainboard.is_empty() {
                continue;
            }
            let main: Vec<(String, u8)> =
                d.mainboard.iter().map(|c| (c.card_name.clone(), c.count.min(250) as u8)).collect();
            let side: Vec<(String, u8)> =
                d.sideboard.iter().map(|c| (c.card_name.clone(), c.count.min(250) as u8)).collect();
            let arch = mtg_sources::archetypes::classify(&rules, &main, &side);
            by_player.insert(d.player.as_str(), arch);
            all_decks.push(mtg_sources::tournaments::TournamentDeck { main, side, date_days: rec.date_days });
        }
        for round in &rec.rounds {
            for m in &round.matches {
                report.matches_total += 1;
                if m.p2.trim().is_empty() || m.p2.trim() == "-" {
                    report.matches_skipped_bye += 1;
                    continue;
                }
                let Some((w, l, d)) = parse_result(&m.result) else {
                    report.matches_skipped_malformed += 1;
                    continue;
                };
                if w + l == 0 {
                    report.draw_only_matches += 1;
                    continue;
                }
                let (Some(a1), Some(a2)) = (by_player.get(m.p1.as_str()), by_player.get(m.p2.as_str()))
                else {
                    report.matches_skipped_unjoined += 1;
                    continue;
                };
                let (Some(a1), Some(a2)) = (a1.as_ref(), a2.as_ref()) else {
                    report.matches_skipped_unclassified += 1;
                    continue;
                };
                if a1 == a2 {
                    report.matches_skipped_mirror += 1;
                    continue;
                }
                // Unordered pair key; wins are stored from side A's view.
                let (key, a_w, b_w) = if a1 <= a2 {
                    ((a1.clone(), a2.clone()), w, l)
                } else {
                    ((a2.clone(), a1.clone()), l, w)
                };
                let e = pair_records
                    .entry(key)
                    .or_insert(PairRecord { a_wins: 0, b_wins: 0, draws: 0, matches: 0 });
                e.a_wins += a_w;
                e.b_wins += b_w;
                e.draws += d;
                e.matches += 1;
                report.matches_used += 1;
            }
        }
    }

    // Qualifying pairs by real game count.
    let mut pairs: Vec<((String, String), PairRecord)> = pair_records
        .into_iter()
        .filter(|(_, r)| r.a_wins + r.b_wins + r.draws >= min_games)
        .collect();
    pairs.sort_by(|(ka, ra), (kb, rb)| {
        let ga = ra.a_wins + ra.b_wins + ra.draws;
        let gb = rb.a_wins + rb.b_wins + rb.draws;
        gb.cmp(&ga).then_with(|| ka.cmp(kb))
    });
    status(format!(
        "{} matches used across {} events; {} archetype pairs with {}+ real games",
        report.matches_used,
        report.tournaments_with_rounds,
        pairs.len(),
        min_games
    ));
    if pairs.is_empty() {
        anyhow::bail!(
            "no archetype pair reaches {min_games} real games in the window; \
             lower --min-games or widen --days"
        );
    }

    // Consensus lists straight from compute_meta: load_meta mangles names
    // with " (N lists)", which would break the archetype join here.
    let computation = mtg_sources::meta::compute_meta(&rules, &all_decks, MIN_LISTS);
    let mut sim_decks: HashMap<String, (SimDeck, usize, f64)> = HashMap::new();
    for m in &computation.decks {
        let parsed = mtg_sources::ParsedDeck {
            name: Some(m.archetype.clone()),
            main: m.main.clone(),
            side: Vec::new(),
            commanders: Vec::new(),
        };
        let (resolved, _) =
            mtg_sources::deck_import::resolve_deck_lossy(pool, &parsed, &m.archetype);
        if let Some(resolved) = resolved {
            let creatures = creature_count(pool, &resolved.main);
            let deck = SimDeck {
                name: m.archetype.clone(),
                cards: resolved.main,
                commander: None,
                meta_share: m.share,
                pilot_warning: mtg_sources::meta::pilot_warning(creatures),
            };
            let coverage = {
                let (_, _, cov) = crate::build_db(pool, &[&deck]);
                cov[0].playable_frac()
            };
            sim_decks.insert(m.archetype.clone(), (deck, m.sample_size, coverage));
        }
    }

    // Simulate each qualifying pair.
    for (idx, ((a, b), real)) in pairs.iter().enumerate() {
        let (Some((deck_a, a_lists, a_cov)), Some((deck_b, b_lists, b_cov))) =
            (sim_decks.get(a), sim_decks.get(b))
        else {
            status(format!("skipping {a} vs {b}: no consensus list (under {MIN_LISTS} lists)"));
            continue;
        };
        let real_games = real.a_wins + real.b_wins + real.draws;
        status(format!(
            "[{}/{}] {} vs {}: {} real games, simulating...",
            idx + 1,
            pairs.len(),
            a,
            b,
            real_games
        ));
        let cfg = SimConfig {
            games_cap: 5000,
            floor: 500,
            early_stop: false,
            precision_target: Some(0.015),
            cancel: None,
            master_seed: seed,
            rules: mtg_engine::RulesConfig::duel(),
        };
        let progress = std::sync::Arc::new(MatchupProgress::default());
        let stats = run_matchup(pool, deck_a, deck_b, &cfg, idx as u64, &progress);

        let real_wins_half = real.a_wins as f64 + real.draws as f64 * 0.5;
        let real_wr = real_wins_half / real_games as f64;
        let real_ci = mtg_stats::wilson(real_wins_half, real_games as f64, 1.96);
        let sim_wr = stats.win_rate();
        let sim_ci = stats.ci95();
        report.pairs.push(PairCalibration {
            a: a.clone(),
            b: b.clone(),
            real_games,
            real_a_wins: real.a_wins,
            real_draws: real.draws,
            real_wr,
            real_ci,
            sim_games: stats.games,
            sim_wr,
            sim_ci,
            divergence: sim_wr - real_wr,
            ci_overlap: sim_ci.0 <= real_ci.1 && real_ci.0 <= sim_ci.1,
            a_lists: *a_lists,
            b_lists: *b_lists,
            a_coverage_playable: *a_cov,
            b_coverage_playable: *b_cov,
            a_pilot_warning: deck_a.pilot_warning,
            b_pilot_warning: deck_b.pilot_warning,
        });
    }

    // Aggregates: games-weighted mean absolute divergence and Pearson r.
    let total_real: f64 = report.pairs.iter().map(|p| p.real_games as f64).sum();
    if total_real > 0.0 {
        report.mean_abs_divergence = report
            .pairs
            .iter()
            .map(|p| p.divergence.abs() * p.real_games as f64)
            .sum::<f64>()
            / total_real;
    }
    let n = report.pairs.len() as f64;
    if n >= 2.0 {
        let mx = report.pairs.iter().map(|p| p.real_wr).sum::<f64>() / n;
        let my = report.pairs.iter().map(|p| p.sim_wr).sum::<f64>() / n;
        let mut sxy = 0.0;
        let mut sxx = 0.0;
        let mut syy = 0.0;
        for p in &report.pairs {
            let dx = p.real_wr - mx;
            let dy = p.sim_wr - my;
            sxy += dx * dy;
            sxx += dx * dx;
            syy += dy * dy;
        }
        if sxx > 0.0 && syy > 0.0 {
            report.correlation = sxy / (sxx * syy).sqrt();
        }
    }
    Ok(report)
}

/// Where calibration reports live.
pub fn calibration_dir() -> Result<std::path::PathBuf> {
    let paths = mtg_data::Paths::resolve()?;
    let dir = paths.meta_dir().join("calibration");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Persist a report as <format>-<unix>.json; returns the path.
pub fn save_report(report: &CalibrationReport) -> Result<std::path::PathBuf> {
    let dir = calibration_dir()?;
    let path = dir.join(format!(
        "{}-{}.json",
        report.format.to_lowercase(),
        report.generated_unix
    ));
    std::fs::write(&path, serde_json::to_vec_pretty(report)?)?;
    Ok(path)
}

/// The newest saved report for a format, if any.
pub fn load_latest_calibration(format: &str) -> Option<CalibrationReport> {
    let dir = calibration_dir().ok()?;
    let prefix = format!("{}-", format.to_lowercase());
    let mut newest: Option<(u64, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        let name = path.file_stem()?.to_string_lossy().to_string();
        let Some(stamp) = name.strip_prefix(&prefix).and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        if newest.as_ref().map(|(t, _)| stamp > *t).unwrap_or(true) {
            newest = Some((stamp, path));
        }
    }
    let (_, path) = newest?;
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Games-weighted mean absolute divergence over pairs involving the
/// archetype, or None if the report never saw it.
pub fn archetype_trust(report: &CalibrationReport, archetype: &str) -> Option<TrustSignal> {
    let involved: Vec<&PairCalibration> =
        report.pairs.iter().filter(|p| p.a == archetype || p.b == archetype).collect();
    if involved.is_empty() {
        return None;
    }
    let games: u32 = involved.iter().map(|p| p.real_games).sum();
    let mad = involved
        .iter()
        .map(|p| p.divergence.abs() * p.real_games as f64)
        .sum::<f64>()
        / games.max(1) as f64;
    Some(TrustSignal {
        pairs: involved.len() as u32,
        real_games: games,
        mean_abs_divergence: mad,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_result;

    #[test]
    fn result_strings() {
        assert_eq!(parse_result("2-1-0"), Some((2, 1, 0)));
        assert_eq!(parse_result("2-0"), Some((2, 0, 0)));
        assert_eq!(parse_result("0-0-3"), Some((0, 0, 3)));
        assert_eq!(parse_result(""), None);
        assert_eq!(parse_result("x-1-0"), None);
        assert_eq!(parse_result("1-2-3-4"), None);
    }
}
