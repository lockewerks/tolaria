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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Fetch { force }) => cmd_fetch(force),
        Some(Command::Card { name }) => cmd_card(&name.join(" ")),
        Some(Command::Compile { name }) => cmd_compile(&name.join(" ")),
        Some(Command::Coverage) => cmd_coverage(),
        None => {
            println!("TUI not built yet; try `tolaria fetch`");
            Ok(())
        }
    }
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
