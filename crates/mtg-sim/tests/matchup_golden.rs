//! Golden regression rail for one head-to-head matchup: aggressive mono-red
//! burn against mono-green stompy at a fixed seed and sample size. Pins the
//! win rate and average game length so a change in combat, damage, or the
//! greedy pilot that moves this matchup shows up as a failing test.

mod common;

use common::{golden_cfg, load_sim_deck, offline_pool, GOLDEN_GAMES};
use mtg_sim::{run_matchup, MatchupProgress};

// Recorded 2026-07-17 at GOLDEN_SEED over GOLDEN_GAMES games (burn on seat 0):
// win_rate 0.2670 (534/2000), avg_turns 14.0830. Bands, not equality: Scryfall
// reissues oracle text now and then and that can shift a card's compiled
// behavior enough to move the aggregate slightly. The bands absorb that noise
// while still failing on a real regression in combat, damage, or the pilot.
const BURN_VS_STOMPY_WIN_RATE: f64 = 0.2670;
const BURN_VS_STOMPY_AVG_TURNS: f64 = 14.0830;

#[test]
fn matchup_burn_vs_stompy_golden() {
    let Some(pool) = offline_pool() else { return };
    let user = load_sim_deck(&pool, "burn.txt");
    let opp = load_sim_deck(&pool, "stompy.txt");
    let cfg = golden_cfg();
    let progress = MatchupProgress::default();
    let stats = run_matchup(&pool, &user, &opp, &cfg, 0, &progress);

    eprintln!(
        "burn vs stompy: games={} wins={} losses={} draws={} panics={} win_rate={:.4} avg_turns={:.4}",
        stats.games, stats.wins, stats.losses, stats.draws, stats.panics,
        stats.win_rate(), stats.avg_turns(),
    );

    assert_eq!(stats.panics, 0, "a panic means a card blew up the engine");
    assert_eq!(stats.games, GOLDEN_GAMES, "fixed sample size (early_stop off)");
    assert!(
        (stats.win_rate() - BURN_VS_STOMPY_WIN_RATE).abs() <= 0.04,
        "win_rate {:.4} drifted from golden {BURN_VS_STOMPY_WIN_RATE} (+/-0.04)",
        stats.win_rate(),
    );
    assert!(
        (stats.avg_turns() - BURN_VS_STOMPY_AVG_TURNS).abs() <= 0.5,
        "avg_turns {:.4} drifted from golden {BURN_VS_STOMPY_AVG_TURNS} (+/-0.5)",
        stats.avg_turns(),
    );
}
