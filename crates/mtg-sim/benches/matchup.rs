//! Criterion bench: a fixed 200-game duel, burn vs stompy. stompy.txt is
//! built by a sibling task; until it lands the opponent falls back to burn so
//! the bench always runs. Uses the offline pool gate from
//! mtg-cards/tests/staples.rs; with no local cache it registers nothing and
//! criterion_main still exits cleanly.

use std::hint::black_box;
use std::path::{Path, PathBuf};

use criterion::{criterion_group, criterion_main, Criterion};

use mtg_sim::{MatchupProgress, SimConfig, SimDeck};

fn decks_dir() -> PathBuf {
    // Benches run with cwd at the crate root; the decks live at the workspace
    // root, two levels up.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("decks")
}

/// Opponent list: prefer stompy.txt when present, otherwise burn.txt. Keeping
/// this a helper means the bench always has a valid opponent regardless of
/// which decks exist on disk.
fn opp_deck_path() -> PathBuf {
    let dir = decks_dir();
    let stompy = dir.join("stompy.txt");
    if stompy.exists() {
        stompy
    } else {
        dir.join("burn.txt")
    }
}

fn to_sim_deck(pool: &mtg_data::CardPool, d: &mtg_sources::ResolvedDeck) -> SimDeck {
    let creatures = mtg_sim::meta_loader::creature_count(pool, &d.main);
    SimDeck {
        name: d.name.clone(),
        cards: d.main.clone(),
        commander: d.commander,
        meta_share: 1.0,
        pilot_warning: mtg_sources::meta::pilot_warning(creatures),
    }
}

fn bench_matchup(c: &mut Criterion) {
    let Ok(paths) = mtg_data::Paths::resolve() else {
        eprintln!("skipping matchup bench: no data paths");
        return;
    };
    let opts = mtg_data::EnsureOptions { offline: true, ..Default::default() };
    let Ok((pool, _)) = mtg_data::ensure_pool(&paths, &opts) else {
        eprintln!("skipping matchup bench: no cached card pool");
        return;
    };

    let burn = decks_dir().join("burn.txt");
    let Ok(user) = mtg_sources::load_deck_file(&pool, &burn) else {
        eprintln!("skipping matchup bench: cannot load {}", burn.display());
        return;
    };
    let opp_path = opp_deck_path();
    let Ok(opp) = mtg_sources::load_deck_file(&pool, &opp_path) else {
        eprintln!("skipping matchup bench: cannot load {}", opp_path.display());
        return;
    };

    let user = to_sim_deck(&pool, &user);
    let opp = to_sim_deck(&pool, &opp);

    // Fixed seed, early stop off, floor == cap: exactly 200 games every
    // iteration so the timing is comparable run to run.
    let cfg = SimConfig {
        games_cap: 200,
        floor: 200,
        early_stop: false,
        master_seed: 0x544f4c41524941,
        rules: mtg_engine::RulesConfig::duel(),
        ..Default::default()
    };

    let progress = MatchupProgress::default();
    let mut group = c.benchmark_group("matchup");
    group.sample_size(10);
    group.bench_function("burn_vs_opp_200", |b| {
        b.iter(|| black_box(mtg_sim::run_matchup(&pool, &user, &opp, &cfg, 0, &progress)))
    });
    group.finish();
}

criterion_group!(benches, bench_matchup);
criterion_main!(benches);
