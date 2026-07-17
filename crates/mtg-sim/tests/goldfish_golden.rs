//! Golden regression rails for goldfish kill-turn distributions. Runs two
//! reference decks against the passive dummy at a fixed seed and sample size
//! and pins the shape of the result: no engine panics, a stable average kill
//! turn, and stable "killed by turn N" fractions.

mod common;

use common::{golden_cfg, load_sim_deck, offline_pool, GOLDEN_GAMES};
use mtg_sim::goldfish::run_goldfish;
use mtg_sim::MatchupProgress;

// Recorded 2026-07-17 at GOLDEN_SEED over GOLDEN_GAMES games. Bands, not
// equality: Scryfall periodically reissues oracle text (reminder-text edits,
// templating changes) and that can nudge a card's compiled behavior and so
// the aggregate a little. Equality would make the suite flap on upstream data
// drift; these bands still catch real regressions (a broken combat step, a
// mana or damage bug) that move the distribution by more than the noise.
//
// burn.txt: avg_kill_turn 5.9185, kill_by(4) 0.1195, kill_by(6) 0.7265.
const BURN_AVG_KILL_TURN: f64 = 5.9185;
const BURN_KILL_BY_4: f64 = 0.1195;
const BURN_KILL_BY_6: f64 = 0.7265;

// stompy.txt: avg_kill_turn 6.0115, kill_by(4) 0.0000, kill_by(6) 0.7900.
const STOMPY_AVG_KILL_TURN: f64 = 6.0115;
const STOMPY_KILL_BY_4: f64 = 0.0000;
const STOMPY_KILL_BY_6: f64 = 0.7900;

#[test]
fn goldfish_burn_golden() {
    let Some(pool) = offline_pool() else { return };
    let deck = load_sim_deck(&pool, "burn.txt");
    let cfg = golden_cfg();
    let progress = MatchupProgress::default();
    let stats = run_goldfish(&pool, &deck, &cfg, &progress);

    eprintln!(
        "burn goldfish: games={} kills={} no_kill={} panics={} avg_kill_turn={:.4} kill_by(4)={:.4} kill_by(6)={:.4}",
        stats.games, stats.kills, stats.no_kill, stats.panics,
        stats.avg_kill_turn, stats.kill_by(4), stats.kill_by(6),
    );

    assert_eq!(stats.games, GOLDEN_GAMES, "fixed sample size");
    assert_eq!(stats.panics, 0, "a goldfish panic means a card blew up the engine");
    assert!(
        (stats.avg_kill_turn - BURN_AVG_KILL_TURN).abs() <= 0.25,
        "avg_kill_turn {:.4} drifted from golden {BURN_AVG_KILL_TURN} (+/-0.25)",
        stats.avg_kill_turn,
    );
    assert!(
        (stats.kill_by(4) - BURN_KILL_BY_4).abs() <= 0.04,
        "kill_by(4) {:.4} drifted from golden {BURN_KILL_BY_4} (+/-0.04)",
        stats.kill_by(4),
    );
    assert!(
        (stats.kill_by(6) - BURN_KILL_BY_6).abs() <= 0.04,
        "kill_by(6) {:.4} drifted from golden {BURN_KILL_BY_6} (+/-0.04)",
        stats.kill_by(6),
    );
}

#[test]
fn goldfish_stompy_golden() {
    let Some(pool) = offline_pool() else { return };
    let deck = load_sim_deck(&pool, "stompy.txt");
    let cfg = golden_cfg();
    let progress = MatchupProgress::default();
    let stats = run_goldfish(&pool, &deck, &cfg, &progress);

    eprintln!(
        "stompy goldfish: games={} kills={} no_kill={} panics={} avg_kill_turn={:.4} kill_by(4)={:.4} kill_by(6)={:.4}",
        stats.games, stats.kills, stats.no_kill, stats.panics,
        stats.avg_kill_turn, stats.kill_by(4), stats.kill_by(6),
    );

    assert_eq!(stats.games, GOLDEN_GAMES, "fixed sample size");
    assert_eq!(stats.panics, 0, "a goldfish panic means a card blew up the engine");
    assert!(
        (stats.avg_kill_turn - STOMPY_AVG_KILL_TURN).abs() <= 0.25,
        "avg_kill_turn {:.4} drifted from golden {STOMPY_AVG_KILL_TURN} (+/-0.25)",
        stats.avg_kill_turn,
    );
    assert!(
        (stats.kill_by(4) - STOMPY_KILL_BY_4).abs() <= 0.04,
        "kill_by(4) {:.4} drifted from golden {STOMPY_KILL_BY_4} (+/-0.04)",
        stats.kill_by(4),
    );
    assert!(
        (stats.kill_by(6) - STOMPY_KILL_BY_6).abs() <= 0.04,
        "kill_by(6) {:.4} drifted from golden {STOMPY_KILL_BY_6} (+/-0.04)",
        stats.kill_by(6),
    );
}
