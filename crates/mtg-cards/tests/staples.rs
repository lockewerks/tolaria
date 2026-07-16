//! Coverage-floor regression: staples that must stay fully modeled. Skips
//! when the local Scryfall cache is absent (fresh checkout, no network).

use mtg_ir::CoverageTier;

#[test]
fn staple_coverage_floor() {
    let Ok(paths) = mtg_data::Paths::resolve() else { return };
    let opts = mtg_data::EnsureOptions { offline: true, ..Default::default() };
    let Ok((pool, _)) = mtg_data::ensure_pool(&paths, &opts) else {
        eprintln!("skipping: no cached card pool");
        return;
    };

    let must_be_full = [
        "Lightning Bolt",
        "Counterspell",
        "Llanowar Elves",
        "Sol Ring",
        "Glorious Anthem",
        "Doom Blade",
        "Giant Growth",
        "Serra Angel",
        "Opt",
        "Divination",
        "Cultivate",
        "Raise the Alarm",
        "Monastery Swiftspear",
        "Thoughtseize",
        "Murder",
        "Shock",
        "Grizzly Bears",
        "Forest",
        "Steam Vents",
    ];
    let mut failures = Vec::new();
    for name in must_be_full {
        let Some(id) = pool.lookup(name) else {
            failures.push(format!("{name}: not in pool"));
            continue;
        };
        let compiled = mtg_cards::compile(pool.get(id));
        let floor = if name == "Steam Vents" { CoverageTier::Partial } else { CoverageTier::Full };
        if compiled.tier < floor {
            failures.push(format!("{name}: {:?} (wanted at least {floor:?})", compiled.tier));
        }
    }
    assert!(failures.is_empty(), "coverage regressions:\n{}", failures.join("\n"));

    // Pool-wide floor: at least half of all cards playable.
    let stats = mtg_cards::compile_pool(&pool);
    let playable = (stats.full + stats.partial) as f64 / stats.total() as f64;
    assert!(playable > 0.5, "pool playable coverage fell to {playable:.2}");
}
