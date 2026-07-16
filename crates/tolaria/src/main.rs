use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tolaria", about = "Mass simulator for Magic: The Gathering decks")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Download or refresh the Scryfall card database.
    Fetch {
        /// Re-check the manifest even if the local cache is fresh.
        #[arg(long)]
        force: bool,
    },
    /// Look up a card by name and print its oracle data.
    Card {
        /// Card name (multiple words allowed without quotes).
        name: Vec<String>,
    },
    /// Compile a card and print its coverage tier and parsed behaviors.
    Compile {
        name: Vec<String>,
    },
    /// Compile the entire card pool and print the coverage histogram.
    Coverage,
    /// Simulate one deck against another, both from decklist files.
    Duel {
        /// Your decklist file.
        #[arg(long)]
        deck: std::path::PathBuf,
        /// The opposing decklist file.
        #[arg(long)]
        vs: std::path::PathBuf,
        /// Games to simulate (early stopping may finish sooner).
        #[arg(long, default_value_t = 1000)]
        games: u32,
        /// Master seed; same seed reproduces identical results.
        #[arg(long, default_value_t = 0x544f4c41524941)]
        seed: u64,
        /// Disable early stopping.
        #[arg(long)]
        no_early_stop: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Fetch { force }) => cmd_fetch(force),
        Some(Command::Card { name }) => cmd_card(&name.join(" ")),
        Some(Command::Compile { name }) => cmd_compile(&name.join(" ")),
        Some(Command::Coverage) => cmd_coverage(),
        Some(Command::Duel { deck, vs, games, seed, no_early_stop }) => {
            cmd_duel(&deck, &vs, games, seed, !no_early_stop)
        }
        None => {
            println!("TUI not built yet; try `tolaria fetch`");
            Ok(())
        }
    }
}

fn to_sim_deck(d: &mtg_sources::ResolvedDeck, share: f64) -> mtg_sim::SimDeck {
    mtg_sim::SimDeck {
        name: d.name.clone(),
        cards: d.main.clone(),
        commander: d.commander,
        meta_share: share,
        pilot_warning: false,
    }
}

fn cmd_duel(
    deck: &std::path::Path,
    vs: &std::path::Path,
    games: u32,
    seed: u64,
    early_stop: bool,
) -> Result<()> {
    let (pool, _) = load_pool(false, false)?;
    let user = mtg_sources::load_deck_file(&pool, deck)?;
    let opp = mtg_sources::load_deck_file(&pool, vs)?;
    let user_sim = to_sim_deck(&user, 1.0);
    let opp_sim = to_sim_deck(&opp, 1.0);

    let (_, _, coverages) = mtg_sim::build_db(&pool, &[&user_sim, &opp_sim]);
    println!(
        "{}: {} cards, coverage {:.0}% full / {:.0}% playable",
        user_sim.name,
        coverages[0].total(),
        coverages[0].full_frac() * 100.0,
        coverages[0].playable_frac() * 100.0
    );
    println!(
        "{}: {} cards, coverage {:.0}% full / {:.0}% playable",
        opp_sim.name,
        coverages[1].total(),
        coverages[1].full_frac() * 100.0,
        coverages[1].playable_frac() * 100.0
    );

    let cfg = mtg_sim::SimConfig {
        games_cap: games,
        floor: 200.min(games),
        early_stop,
        master_seed: seed,
        rules: mtg_engine::RulesConfig::duel(),
    };
    let progress = std::sync::Arc::new(mtg_sim::MatchupProgress::default());
    let started = std::time::Instant::now();
    let stats = mtg_sim::run_matchup(&pool, &user_sim, &opp_sim, &cfg, 0, &progress);
    let elapsed = started.elapsed().as_secs_f64();
    let (lo, hi) = stats.ci95();
    println!(
        "\n{} vs {}: {} games in {:.2}s ({:.0} games/s)",
        user_sim.name,
        opp_sim.name,
        stats.games,
        elapsed,
        stats.games as f64 / elapsed
    );
    println!(
        "win rate {:.1}% (95% CI {:.1}%..{:.1}%){}",
        stats.win_rate() * 100.0,
        lo * 100.0,
        hi * 100.0,
        if stats.stopped_early { " [early stop]" } else { "" }
    );
    println!(
        "wins {} / losses {} / draws {} / panics {}",
        stats.wins, stats.losses, stats.draws, stats.panics
    );
    println!(
        "on the play {:.1}%, on the draw {:.1}%, avg game {:.1} turns",
        stats.on_play_rate() * 100.0,
        stats.on_draw_rate() * 100.0,
        stats.avg_turns()
    );
    Ok(())
}

fn cmd_compile(name: &str) -> Result<()> {
    let (pool, _) = load_pool(false, false)?;
    let Some(id) = pool.lookup(name) else {
        println!("not found: {name}");
        return Ok(());
    };
    let card = pool.get(id);
    let compiled = mtg_cards::compile(card);
    println!("{} -> {:?}", card.name, compiled.tier);
    for d in &compiled.dropped {
        println!("  dropped: {d}");
    }
    for (i, f) in compiled.faces.iter().enumerate() {
        println!(
            "  face {i}: cost={:?} kw={:?} spell={} mana_abilities={} activated={} triggered={} statics={} repl={}",
            f.cost.as_ref().map(|c| c.mana_value(0)),
            f.keywords,
            f.spell.is_some(),
            f.mana_abilities.len(),
            f.activated.len(),
            f.triggered.len(),
            f.statics.len(),
            f.replacements.len(),
        );
        if let Some(sa) = &f.spell {
            println!("    spell targets={} effect={:?}", sa.targets.len(), sa.effect);
        }
    }
    Ok(())
}

fn cmd_coverage() -> Result<()> {
    let (pool, _) = load_pool(false, false)?;
    let started = std::time::Instant::now();
    let stats = mtg_cards::compile_pool(&pool);
    let total = stats.total() as f64;
    println!(
        "compiled {} cards in {:.2}s",
        stats.total(),
        started.elapsed().as_secs_f32()
    );
    println!("  full:       {:>6} ({:.1}%)", stats.full, stats.full as f64 / total * 100.0);
    println!("  partial:    {:>6} ({:.1}%)", stats.partial, stats.partial as f64 / total * 100.0);
    println!("  proxy:      {:>6} ({:.1}%)", stats.proxy, stats.proxy as f64 / total * 100.0);
    println!("  unplayable: {:>6} ({:.1}%)", stats.unplayable, stats.unplayable as f64 / total * 100.0);
    Ok(())
}

fn load_pool(force: bool, offline: bool) -> Result<(mtg_data::CardPool, mtg_data::PoolStatus)> {
    let paths = mtg_data::Paths::resolve()?;
    let opts = mtg_data::EnsureOptions {
        user_agent: None,
        force_refresh: force,
        offline,
    };
    Ok(mtg_data::ensure_pool(&paths, &opts)?)
}

fn cmd_fetch(force: bool) -> Result<()> {
    let started = std::time::Instant::now();
    let (pool, status) = load_pool(force, false)?;
    let source = match status.source {
        mtg_data::PoolSource::FreshCache => "fresh cache",
        mtg_data::PoolSource::StaleCache => "stale cache (network unavailable)",
        mtg_data::PoolSource::Downloaded => "downloaded",
    };
    println!(
        "card pool: {} cards ({source}, scryfall updated {}) in {:.1}s",
        pool.len(),
        status.updated_at,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

fn cmd_card(name: &str) -> Result<()> {
    let (pool, _) = load_pool(false, false)?;
    match pool.lookup(name) {
        Some(id) => {
            let card = pool.get(id);
            println!("{} [{:?}]", card.name, card.layout);
            for f in &card.faces {
                println!("  {} {}", f.name, f.mana_cost);
                println!("    {}", f.type_line);
                if !f.oracle_text.is_empty() {
                    for line in f.oracle_text.lines() {
                        println!("    | {line}");
                    }
                }
                if let (Some(p), Some(t)) = (f.power, f.toughness) {
                    println!("    {p}/{t}");
                }
            }
            let legal: Vec<String> = mtg_data::Format::ALL
                .into_iter()
                .filter(|f| card.legalities.is_legal(*f))
                .map(|f| f.to_string())
                .collect();
            println!("  legal: {}", legal.join(", "));
        }
        None => {
            println!("not found: {name}");
            let sugg = pool.suggest(name, 3);
            if !sugg.is_empty() {
                println!("did you mean: {}", sugg.join(", "));
            }
        }
    }
    Ok(())
}
