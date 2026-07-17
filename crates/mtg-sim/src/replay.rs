//! Deterministic single-game replay with tracing on. The hot loop never
//! traces (35k games/s stays 35k games/s); instead a recorded sample game
//! is regenerated here from its seed, because every game is a pure
//! function of (master seed, matchup index, game index).

use mtg_data::CardPool;
use mtg_engine::{Agents, GameEnd, GameSetup};

use crate::{build_db, game_seed, SimConfig, SimDeck};

pub struct ReplayResult {
    pub summary: String,
    pub winner: Option<u8>,
    pub turns: u32,
    pub trace: Vec<String>,
    /// The panic message, when the game crashed the engine.
    pub panic: Option<String>,
}

/// Downcast a caught panic payload to its message.
fn panic_message(e: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = e.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = e.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with a non-string payload".to_string()
    }
}

fn run_traced(
    pool: &CardPool,
    decks: &[&SimDeck],
    rules: mtg_engine::RulesConfig,
    seed: u64,
    first: u8,
    seats: usize,
) -> ReplayResult {
    let (db, lists, _) = build_db(pool, decks);
    let setup = GameSetup { cfg: rules, first: Some(first), trace: true, forced_top: None };
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let mut agents = Agents {
            seats: (0..seats)
                .map(|_| Box::new(mtg_ai::GreedyAgent) as Box<dyn mtg_engine::Agent>)
                .collect(),
        };
        mtg_engine::run_game(db, &lists, &setup, &mut agents, seed)
    }));
    std::panic::set_hook(old_hook);

    match out {
        Ok(o) => {
            let winner = match o.end {
                GameEnd::Winner(s) => Some(s),
                GameEnd::Draw => None,
            };
            let summary = match winner {
                Some(0) => format!("seat 0 wins in {} turns", o.turns),
                Some(w) => format!("seat {w} wins in {} turns", o.turns),
                None => format!("draw after {} turns", o.turns),
            };
            ReplayResult {
                summary,
                winner,
                turns: o.turns,
                trace: o.trace.unwrap_or_default(),
                panic: None,
            }
        }
        Err(e) => {
            let msg = panic_message(e);
            ReplayResult {
                summary: format!("game panicked: {msg}"),
                winner: None,
                turns: 0,
                trace: Vec::new(),
                panic: Some(msg),
            }
        }
    }
}

/// Prepend a divergence note when the replay does not match what the run
/// recorded (meta drift or code change since).
fn guard(mut r: ReplayResult, expected: Option<(&str, u32)>) -> ReplayResult {
    if let Some((exp_outcome, exp_turns)) = expected {
        let got = match r.winner {
            Some(0) => "win",
            Some(_) => "loss",
            None if r.panic.is_some() => "panic",
            None => "draw",
        };
        // "long" is a bucket label, not an outcome; only compare real ones.
        let mismatch = !matches!(exp_outcome, "long")
            && (got != exp_outcome || (exp_turns != 0 && r.turns != exp_turns));
        if mismatch {
            r.trace.insert(
                0,
                format!(
                    "** replay diverges from the recorded run (recorded {exp_outcome} in {exp_turns} turns): decks or engine changed since **"
                ),
            );
        }
    }
    r
}

/// Replay one matchup game exactly as run_matchup would have produced it.
pub fn replay_matchup_game(
    pool: &CardPool,
    user: &SimDeck,
    opp: &SimDeck,
    cfg: &SimConfig,
    matchup_index: u64,
    game: u32,
    expected: Option<(&str, u32)>,
) -> ReplayResult {
    let seed = game_seed(cfg.master_seed, matchup_index, game);
    let first = (game % 2) as u8;
    guard(run_traced(pool, &[user, opp], cfg.rules, seed, first, 2), expected)
}

/// Replay one goldfish game (passive opponent, user always on the play).
pub fn replay_goldfish_game(
    pool: &CardPool,
    user: &SimDeck,
    cfg: &SimConfig,
    game: u32,
    expected: Option<(&str, u32)>,
) -> ReplayResult {
    // Matches goldfish.rs: master seed salted with "GOLD", user first, a
    // passive opponent in seat 1.
    let seed = game_seed(cfg.master_seed ^ 0x474f4c44, 0, game);
    let setup = GameSetup { cfg: cfg.rules, first: Some(0), trace: true, forced_top: None };
    let (db, lists, _) = build_db(pool, &[user, user]);
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let mut agents = Agents {
            seats: vec![
                Box::new(mtg_ai::GreedyAgent) as Box<dyn mtg_engine::Agent>,
                Box::new(mtg_engine::PassAgent),
            ],
        };
        mtg_engine::run_game(db, &lists, &setup, &mut agents, seed)
    }));
    std::panic::set_hook(old_hook);
    let r = match out {
        Ok(o) => {
            let winner = match o.end {
                GameEnd::Winner(s) => Some(s),
                GameEnd::Draw => None,
            };
            let summary = match winner {
                Some(0) => format!("killed the goldfish on turn {}", (o.turns + 1) / 2),
                _ => format!("no kill after {} turns", o.turns),
            };
            ReplayResult { summary, winner, turns: o.turns, trace: o.trace.unwrap_or_default(), panic: None }
        }
        Err(e) => {
            let msg = panic_message(e);
            ReplayResult { summary: format!("game panicked: {msg}"), winner: None, turns: 0, trace: Vec::new(), panic: Some(msg) }
        }
    };
    guard(r, expected)
}
