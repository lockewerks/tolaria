//! Shared fixtures for the golden regression tests. Loading the pool needs a
//! local Scryfall cache; without it the callers return early and the suite
//! skips cleanly (same offline gate as mtg-cards/tests/staples.rs).

// Not every golden test binary uses every helper here.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use mtg_data::CardPool;
use mtg_engine::RulesConfig;
use mtg_sim::{SimConfig, SimDeck};

/// One seed for every golden so a rerun reproduces byte-identical stats.
/// "ACCOLADE" spelled in hex.
pub const GOLDEN_SEED: u64 = 0xACC0_1ADE;

/// Fixed sample size: floor == cap with early_stop off pins the run to
/// exactly this many games.
pub const GOLDEN_GAMES: u32 = 2000;

/// Cached pool, or `None` when there is no local cache (fresh checkout, no
/// network). Callers turn `None` into an early return so the test skips.
pub fn offline_pool() -> Option<CardPool> {
    let paths = mtg_data::Paths::resolve().ok()?;
    let opts = mtg_data::EnsureOptions { offline: true, ..Default::default() };
    match mtg_data::ensure_pool(&paths, &opts) {
        Ok((pool, _)) => Some(pool),
        Err(_) => {
            eprintln!("skipping: no cached card pool");
            None
        }
    }
}

fn deck_path(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../decks").join(file)
}

/// Load a repo deck file into a `SimDeck` the same way the CLI does, so the
/// golden numbers reflect a real run and not a synthetic list.
pub fn load_sim_deck(pool: &CardPool, file: &str) -> SimDeck {
    let path = deck_path(file);
    let resolved = mtg_sources::load_deck_file(pool, &path)
        .unwrap_or_else(|e| panic!("load {}: {e}", path.display()));
    let creatures = mtg_sim::meta_loader::creature_count(pool, &resolved.main);
    SimDeck {
        name: resolved.name,
        cards: resolved.main,
        commander: resolved.commander,
        meta_share: 1.0,
        pilot_warning: mtg_sources::meta::pilot_warning(creatures),
    }
}

/// Fixed-sample config: floor == cap and no early stop or precision gate, so
/// the master seed alone determines the result.
pub fn golden_cfg() -> SimConfig {
    SimConfig {
        games_cap: GOLDEN_GAMES,
        floor: GOLDEN_GAMES,
        early_stop: false,
        precision_target: None,
        cancel: None,
        master_seed: GOLDEN_SEED,
        rules: RulesConfig::duel(),
    }
}
