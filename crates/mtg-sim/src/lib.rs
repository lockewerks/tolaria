//! Simulation harness: matchup scheduling, rayon parallelism, seed
//! derivation, early stopping, panic isolation.

pub mod calibrate;
pub mod coverage;
pub mod goldfish;
pub mod limits;
pub mod meta_loader;
pub mod replay;
pub mod sweep;
pub mod trust;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use rayon::prelude::*;

use mtg_data::{CardId, CardPool};
use mtg_engine::{Agents, CardDb, DeckList, GameEnd, GameSetup, RulesConfig};
use mtg_ir::CoverageTier;
use mtg_stats::{early_stop_decided, GauntletStats, MatchupStats};

/// A deck ready to simulate: counts of pool cards.
#[derive(Debug, Clone)]
pub struct SimDeck {
    pub name: String,
    pub cards: Vec<(CardId, u8)>,
    pub commander: Option<CardId>,
    pub meta_share: f64,
    /// Set when the list has fewer than 10 mainboard creatures: a crude
    /// proxy for decks whose real lines (combo chains, control finesse) a
    /// greedy agent cannot execute. Applied to the user's deck and meta
    /// opponents alike; creature-based combo passes undetected.
    pub pilot_warning: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DeckCoverage {
    pub full: u32,
    pub partial: u32,
    pub proxy: u32,
    pub unplayable: u32,
}

impl DeckCoverage {
    pub fn total(&self) -> u32 {
        self.full + self.partial + self.proxy + self.unplayable
    }

    pub fn full_frac(&self) -> f64 {
        if self.total() == 0 {
            return 0.0;
        }
        self.full as f64 / self.total() as f64
    }

    pub fn playable_frac(&self) -> f64 {
        if self.total() == 0 {
            return 0.0;
        }
        (self.full + self.partial) as f64 / self.total() as f64
    }
}

/// Compile the union of two decks into a compact per-match CardDb.
pub fn build_db(pool: &CardPool, decks: &[&SimDeck]) -> (Arc<CardDb>, Vec<DeckList>, Vec<DeckCoverage>) {
    let mut db = CardDb::default();
    let mut map: std::collections::HashMap<CardId, mtg_engine::CardRef> =
        std::collections::HashMap::new();
    let mut lists = Vec::new();
    let mut coverages = Vec::new();
    for deck in decks {
        let mut cards = Vec::new();
        let mut cov = DeckCoverage::default();
        for &(cid, count) in &deck.cards {
            let r = *map.entry(cid).or_insert_with(|| {
                let oracle = pool.get(cid).clone();
                let compiled = mtg_cards::compile(&oracle);
                db.add(oracle, compiled)
            });
            let tier = db.get(r).compiled.tier;
            for _ in 0..count {
                cards.push(r);
                match tier {
                    CoverageTier::Full => cov.full += 1,
                    CoverageTier::Partial => cov.partial += 1,
                    CoverageTier::Proxy => cov.proxy += 1,
                    CoverageTier::Unplayable => cov.unplayable += 1,
                }
            }
        }
        let commander = deck.commander.map(|cid| {
            *map.entry(cid).or_insert_with(|| {
                let oracle = pool.get(cid).clone();
                let compiled = mtg_cards::compile(&oracle);
                db.add(oracle, compiled)
            })
        });
        lists.push(DeckList { cards, commander });
        coverages.push(cov);
    }
    (Arc::new(db), lists, coverages)
}

#[derive(Debug, Clone)]
pub struct SimConfig {
    pub games_cap: u32,
    pub floor: u32,
    pub early_stop: bool,
    /// When set, keep playing until the 95% CI half-width shrinks to this
    /// fraction (games_cap stays a hard ceiling). This is the "auto" mode:
    /// the matchup's own variance decides the sample size.
    pub precision_target: Option<f64>,
    /// Cooperative cancellation: checked between game blocks and matchups.
    pub cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub master_seed: u64,
    pub rules: RulesConfig,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            games_cap: 1000,
            floor: 200,
            early_stop: true,
            precision_target: None,
            cancel: None,
            // "TOLARIA" in ASCII.
            master_seed: 0x544f4c41524941,
            rules: RulesConfig::duel(),
        }
    }
}

impl SimConfig {
    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(false)
    }
}

/// Index into win_reasons/loss_reasons: life, poison, deckout, commander,
/// other.
fn loss_reason_index(r: Option<mtg_engine::LossReason>) -> u8 {
    match r {
        Some(mtg_engine::LossReason::Life) => 0,
        Some(mtg_engine::LossReason::Poison) => 1,
        Some(mtg_engine::LossReason::DeckOut) => 2,
        Some(mtg_engine::LossReason::CommanderDamage) => 3,
        _ => 4,
    }
}

pub(crate) fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

pub fn game_seed(master: u64, matchup: u64, game: u32) -> u64 {
    splitmix64(master ^ splitmix64(matchup).wrapping_add(game as u64))
}

/// One finished game's outcome, gathered in the parallel map and folded
/// into stats sequentially so aggregation order stays deterministic.
struct GameRow {
    winner: Option<u8>,
    turns: u32,
    decisions: u32,
    first: u8,
    my_mulls: u8,
    reason: u8,
    my_life: i32,
    opp_life: i32,
}

/// Keep a small, deterministic set of games worth replaying: the first of
/// each outcome kind plus the longest game seen. Bounded and order-stable.
fn note_sample(samples: &mut Vec<mtg_stats::SampleGame>, game: u32, outcome: &str, turns: u32) {
    let have_kind = samples.iter().any(|s| s.outcome == outcome);
    if !have_kind {
        samples.push(mtg_stats::SampleGame { game, outcome: outcome.to_string(), turns });
        return;
    }
    // Replace the recorded "long" exemplar when this game ran longer.
    if let Some(longest) = samples.iter_mut().find(|s| s.outcome == "long") {
        if turns > longest.turns {
            *longest = mtg_stats::SampleGame { game, outcome: "long".to_string(), turns };
        }
    } else if turns > samples.iter().map(|s| s.turns).max().unwrap_or(0) {
        samples.push(mtg_stats::SampleGame { game, outcome: "long".to_string(), turns });
    }
}

/// Live counters a UI can poll while a matchup runs.
#[derive(Debug, Default)]
pub struct MatchupProgress {
    pub done: AtomicU32,
    pub wins: AtomicU32,
    pub losses: AtomicU32,
    pub draws: AtomicU32,
    pub panics: AtomicU32,
    pub stopped: AtomicBool,
    pub target: AtomicU32,
}

/// Run one matchup: seat 0 is the user's deck.
pub fn run_matchup(
    pool: &CardPool,
    user: &SimDeck,
    opp: &SimDeck,
    cfg: &SimConfig,
    matchup_index: u64,
    progress: &MatchupProgress,
) -> MatchupStats {
    let (db, lists, coverages) = build_db(pool, &[user, opp]);
    progress.target.store(cfg.games_cap, Ordering::Relaxed);

    let mut stats = MatchupStats {
        opponent: opp.name.clone(),
        meta_share: opp.meta_share,
        opp_coverage_full_frac: coverages[1].full_frac(),
        opp_coverage_playable_frac: coverages[1].playable_frac(),
        opp_pilot_warning: opp.pilot_warning,
        turn_hist: vec![0; 40],
        win_reasons: vec![0; 5],
        loss_reasons: vec![0; 5],
        mull_hist: vec![0; 4],
        ..Default::default()
    };

    let setup = GameSetup { cfg: cfg.rules, first: None, trace: false, forced_top: None };
    // Block size trades early-stop granularity for fewer rayon barriers.
    // At 32 threads and roughly a millisecond per game, 256 keeps the pool
    // saturated; the first stop check still lands right after the 200-game
    // floor.
    const BLOCK: u32 = 256;
    let mut next_game = 0u32;

    while next_game < cfg.games_cap {
        if cfg.cancelled() {
            break;
        }
        let block_end = (next_game + BLOCK).min(cfg.games_cap);
        let results: Vec<(u32, Option<GameRow>)> = (next_game..block_end)
            .into_par_iter()
            .map(|g| {
                let seed = game_seed(cfg.master_seed, matchup_index, g);
                // Alternate who is on the play.
                let mut setup = setup.clone();
                setup.first = Some((g % 2) as u8);
                let db = db.clone();
                let lists = lists.clone();
                let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    let mut agents = Agents {
                        seats: vec![Box::new(mtg_ai::GreedyAgent), Box::new(mtg_ai::GreedyAgent)],
                    };
                    mtg_engine::run_game(db, &lists, &setup, &mut agents, seed)
                }));
                let row = out.ok().map(|o| {
                    let winner = match o.end {
                        GameEnd::Winner(s) => Some(s),
                        GameEnd::Draw => None,
                    };
                    let reason = winner
                        .map(|w| {
                            let loser = 1 - w as usize;
                            loss_reason_index(o.losses.get(loser).copied().flatten())
                        })
                        .unwrap_or(4);
                    GameRow {
                        winner,
                        turns: o.turns,
                        decisions: o.decisions,
                        first: o.first,
                        my_mulls: o.mulligans.first().copied().unwrap_or(0),
                        reason,
                        my_life: o.life.first().copied().unwrap_or(0),
                        opp_life: o.life.get(1).copied().unwrap_or(0),
                    }
                });
                (g, row)
            })
            .collect();

        for (g, r) in results {
            stats.games += 1;
            match r {
                Some(row) => {
                    stats.turns_sum += row.turns as u64;
                    stats.my_mulligans += row.my_mulls as u32;
                    stats.turn_hist[(row.turns as usize).saturating_sub(1).min(39)] += 1;
                    stats.mull_hist[(row.my_mulls as usize).min(3)] += 1;
                    let on_play = row.first == 0;
                    if on_play {
                        stats.on_play_games += 1;
                    }
                    match row.winner {
                        Some(0) => {
                            stats.wins += 1;
                            stats.win_reasons[row.reason as usize] += 1;
                            stats.win_life_sum += row.my_life as i64;
                            stats.win_opp_life_sum += row.opp_life as i64;
                            if on_play {
                                stats.on_play_wins += 1;
                            }
                            note_sample(&mut stats.sample_games, g, "win", row.turns);
                        }
                        Some(_) => {
                            stats.losses += 1;
                            stats.loss_reasons[row.reason as usize] += 1;
                            stats.loss_life_sum += row.my_life as i64;
                            stats.loss_opp_life_sum += row.opp_life as i64;
                            note_sample(&mut stats.sample_games, g, "loss", row.turns);
                        }
                        None => {
                            stats.draws += 1;
                            if row.turns > cfg.rules.turn_cap {
                                stats.turn_cap_draws += 1;
                            } else if row.decisions > cfg.rules.decision_cap {
                                stats.decision_cap_draws += 1;
                            }
                            note_sample(&mut stats.sample_games, g, "draw", row.turns);
                        }
                    }
                }
                None => {
                    stats.panics += 1;
                    stats.games -= 1;
                    note_sample(&mut stats.sample_games, g, "panic", 0);
                }
            }
        }
        progress.done.store(stats.games, Ordering::Relaxed);
        progress.wins.store(stats.wins, Ordering::Relaxed);
        progress.losses.store(stats.losses, Ordering::Relaxed);
        progress.draws.store(stats.draws, Ordering::Relaxed);
        progress.panics.store(stats.panics, Ordering::Relaxed);

        next_game = block_end;
        let done = match cfg.precision_target {
            Some(target) => {
                stats.games >= cfg.floor
                    && mtg_stats::ci_half_width(stats.wins, stats.draws, stats.games) <= target
            }
            None => {
                cfg.early_stop
                    && early_stop_decided(stats.wins, stats.draws, stats.games, cfg.floor)
            }
        };
        if done {
            stats.stopped_early = true;
            progress.stopped.store(true, Ordering::Relaxed);
            break;
        }
    }
    stats
}

/// Four-player Commander pods: the user plus three opponents sampled from
/// the meta by share. Win rate is seat 0's share of finished games; an even
/// pod baseline is 25%.
pub fn run_pod(
    pool: &CardPool,
    user: &SimDeck,
    opponents: &[SimDeck],
    cfg: &SimConfig,
    progress: &MatchupProgress,
) -> MatchupStats {
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let mut stats = MatchupStats {
        opponent: format!("pods from {} decks", opponents.len()),
        meta_share: 1.0,
        ..Default::default()
    };
    progress.target.store(cfg.games_cap, Ordering::Relaxed);

    // Cumulative shares for sampling opponents per game.
    let total_share: f64 = opponents.iter().map(|o| o.meta_share.max(0.001)).sum();
    let pick = |r: f64| -> &SimDeck {
        let mut acc = 0.0;
        for o in opponents {
            acc += o.meta_share.max(0.001) / total_share;
            if r <= acc {
                return o;
            }
        }
        opponents.last().unwrap()
    };

    const BLOCK: u32 = 32;
    let mut next_game = 0u32;
    while next_game < cfg.games_cap {
        if cfg.cancelled() {
            break;
        }
        let block_end = (next_game + BLOCK).min(cfg.games_cap);
        let results: Vec<(u32, Option<(Option<u8>, u32, u32, u8, i32)>)> = (next_game..block_end)
            .into_par_iter()
            .map(|g| {
                let seed = game_seed(cfg.master_seed, u64::MAX, g);
                // Sample three opponents deterministically from the seed.
                let mut s = seed;
                let mut rand01 = || {
                    s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    (s >> 11) as f64 / (1u64 << 53) as f64
                };
                let opps = [pick(rand01()), pick(rand01()), pick(rand01())];
                let (db, lists, _) =
                    build_db(pool, &[user, opps[0], opps[1], opps[2]]);
                let setup = GameSetup {
                    cfg: cfg.rules,
                    first: Some((g % 4) as u8),
                    trace: false,
                    forced_top: None,
                };
                let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    let mut agents = Agents {
                        seats: (0..4)
                            .map(|_| Box::new(mtg_ai::GreedyAgent) as Box<dyn mtg_engine::Agent>)
                            .collect(),
                    };
                    mtg_engine::run_game(db, &lists, &setup, &mut agents, seed)
                }));
                let row = out.ok().map(|o| {
                    let winner = match o.end {
                        GameEnd::Winner(s) => Some(s),
                        GameEnd::Draw => None,
                    };
                    (winner, o.turns, o.decisions, o.first, o.life.first().copied().unwrap_or(0))
                });
                (g, row)
            })
            .collect();
        for (g, r) in results {
            stats.games += 1;
            match r {
                Some((winner, turns, decisions, first, my_life)) => {
                    stats.turns_sum += turns as u64;
                    if first == 0 {
                        stats.on_play_games += 1;
                    }
                    match winner {
                        Some(0) => {
                            stats.wins += 1;
                            stats.win_life_sum += my_life as i64;
                            if first == 0 {
                                stats.on_play_wins += 1;
                            }
                            note_sample(&mut stats.sample_games, g, "win", turns);
                        }
                        Some(_) => {
                            stats.losses += 1;
                            stats.loss_life_sum += my_life as i64;
                            note_sample(&mut stats.sample_games, g, "loss", turns);
                        }
                        None => {
                            stats.draws += 1;
                            if turns > cfg.rules.turn_cap {
                                stats.turn_cap_draws += 1;
                            } else if decisions > cfg.rules.decision_cap {
                                stats.decision_cap_draws += 1;
                            }
                            note_sample(&mut stats.sample_games, g, "draw", turns);
                        }
                    }
                }
                None => {
                    stats.panics += 1;
                    stats.games -= 1;
                    note_sample(&mut stats.sample_games, g, "panic", 0);
                }
            }
        }
        progress.done.store(stats.games, Ordering::Relaxed);
        progress.wins.store(stats.wins, Ordering::Relaxed);
        next_game = block_end;
    }
    std::panic::set_hook(old_hook);
    stats
}

/// Run the full gauntlet sequentially over matchups (each matchup is
/// internally parallel). Progress is observable per matchup.
pub fn run_gauntlet(
    pool: &CardPool,
    user: &SimDeck,
    opponents: &[SimDeck],
    cfg: &SimConfig,
    progress: &[Arc<MatchupProgress>],
) -> GauntletStats {
    // A panicking game must not spam stderr; silence the hook for the run.
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut out = GauntletStats {
        deck_name: user.name.clone(),
        format: String::new(),
        matchups: Vec::new(),
    };
    for (i, opp) in opponents.iter().enumerate() {
        if cfg.cancelled() {
            break;
        }
        let default_progress = Arc::new(MatchupProgress::default());
        let p = progress.get(i).unwrap_or(&default_progress);
        out.matchups.push(run_matchup(pool, user, opp, cfg, i as u64, p));
    }
    std::panic::set_hook(old_hook);
    out
}
