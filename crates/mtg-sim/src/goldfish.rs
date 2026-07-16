//! Goldfish mode: the deck plays against a passive opponent who does
//! nothing but exist. Measures the deck as it stands, no interaction:
//! kill-turn distribution, consistency, mulligans. Any deck size.

use std::sync::atomic::Ordering;

use rayon::prelude::*;

use mtg_data::CardPool;
use mtg_engine::{Agents, GameEnd, GameSetup, PassAgent};
use serde::{Deserialize, Serialize};

use crate::{build_db, game_seed, MatchupProgress, SimConfig, SimDeck};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldfishStats {
    pub games: u32,
    pub kills: u32,
    pub no_kill: u32,
    pub panics: u32,
    /// Kills by the user's own turn number, buckets turn 1..=20 (index 0 is
    /// turn 1, last bucket is 20 or later).
    pub kill_hist: Vec<u32>,
    /// Mulligans taken per game: 0, 1, 2, 3+.
    pub mull_hist: Vec<u32>,
    pub avg_kill_turn: f64,
}

impl GoldfishStats {
    /// Fraction of games won by the end of the given user turn.
    pub fn kill_by(&self, turn: usize) -> f64 {
        if self.games == 0 {
            return 0.0;
        }
        let upto: u32 = self.kill_hist.iter().take(turn.min(self.kill_hist.len())).sum();
        upto as f64 / self.games as f64
    }
}

/// A passive opponent list: one giant pile of Wastes that never acts.
fn dummy_deck(pool: &CardPool) -> Option<SimDeck> {
    let land = pool
        .lookup("wastes")
        .or_else(|| pool.lookup("island"))
        .or_else(|| pool.lookup("plains"))?;
    Some(SimDeck {
        name: "goldfish dummy".into(),
        cards: vec![(land, 250)],
        commander: None,
        meta_share: 0.0,
        pilot_warning: false,
    })
}

pub fn run_goldfish(
    pool: &CardPool,
    user: &SimDeck,
    cfg: &SimConfig,
    progress: &MatchupProgress,
) -> GoldfishStats {
    let dummy = match dummy_deck(pool) {
        Some(d) => d,
        None => {
            return GoldfishStats {
                games: 0,
                kills: 0,
                no_kill: 0,
                panics: 0,
                kill_hist: vec![0; 20],
                mull_hist: vec![0; 4],
                avg_kill_turn: 0.0,
            }
        }
    };
    let (db, lists, _) = build_db(pool, &[user, &dummy]);
    progress.target.store(cfg.games_cap, Ordering::Relaxed);

    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let mut stats = GoldfishStats {
        games: 0,
        kills: 0,
        no_kill: 0,
        panics: 0,
        kill_hist: vec![0; 20],
        mull_hist: vec![0; 4],
        avg_kill_turn: 0.0,
    };
    let mut kill_turn_sum = 0u64;

    const BLOCK: u32 = 128;
    let mut next = 0u32;
    while next < cfg.games_cap {
        if cfg
            .cancel
            .as_ref()
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            break;
        }
        let end = (next + BLOCK).min(cfg.games_cap);
        let results: Vec<Option<(Option<u8>, u32, u8)>> = (next..end)
            .into_par_iter()
            .map(|g| {
                let seed = game_seed(cfg.master_seed ^ 0x474f4c44, 0, g);
                let setup = GameSetup {
                    cfg: cfg.rules,
                    first: Some(0),
                    trace: false,
                    forced_top: None,
                };
                let db = db.clone();
                let lists = lists.clone();
                let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    let mut agents = Agents {
                        seats: vec![Box::new(mtg_ai::GreedyAgent), Box::new(PassAgent)],
                    };
                    mtg_engine::run_game(db, &lists, &setup, &mut agents, seed)
                }));
                match out {
                    Ok(o) => {
                        let winner = match o.end {
                            GameEnd::Winner(s) => Some(s),
                            GameEnd::Draw => None,
                        };
                        Some((winner, o.turns, o.mulligans.first().copied().unwrap_or(0)))
                    }
                    Err(_) => None,
                }
            })
            .collect();
        for r in results {
            match r {
                Some((winner, turns, mulls)) => {
                    stats.games += 1;
                    stats.mull_hist[(mulls as usize).min(3)] += 1;
                    if winner == Some(0) {
                        // The user always plays first, so their turn number
                        // is the ceiling of half the total turn count.
                        let user_turn = ((turns + 1) / 2) as usize;
                        stats.kills += 1;
                        kill_turn_sum += user_turn as u64;
                        stats.kill_hist[user_turn.saturating_sub(1).min(19)] += 1;
                    } else {
                        stats.no_kill += 1;
                    }
                }
                None => stats.panics += 1,
            }
        }
        progress.done.store(stats.games, Ordering::Relaxed);
        progress.wins.store(stats.kills, Ordering::Relaxed);
        next = end;
    }
    std::panic::set_hook(old_hook);
    stats.avg_kill_turn = if stats.kills > 0 {
        kill_turn_sum as f64 / stats.kills as f64
    } else {
        0.0
    };
    stats
}
