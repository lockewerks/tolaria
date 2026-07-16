use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "tolaria",
    about = "Mass simulator for Magic: The Gathering decks",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    top: TopArgs,
}

/// Flags accepted without a subcommand: `tolaria --deck x.txt` is shorthand
/// for `tolaria run --deck x.txt`; with no flags at all, the TUI launches.
#[derive(clap::Args)]
struct TopArgs {
    /// Your decklist file (runs the meta gauntlet headless).
    #[arg(long)]
    deck: Option<std::path::PathBuf>,
    #[arg(long, default_value = "modern")]
    format: String,
    /// A number (cap with early stopping) or "auto".
    #[arg(long, default_value = "1000")]
    games: String,
    /// Auto mode target: CI half-width in percentage points.
    #[arg(long, default_value_t = 1.0)]
    precision: f64,
    #[arg(long, default_value_t = 60)]
    days: i64,
    #[arg(long, default_value_t = 12)]
    top: usize,
    #[arg(long, default_value_t = 0x544f4c41524941)]
    seed: u64,
    /// Write full results as JSON.
    #[arg(long)]
    json: Option<std::path::PathBuf>,
    /// Play every requested game even after the result is decided.
    #[arg(long)]
    no_early_stop: bool,
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
    /// Launch the interactive terminal UI (the default).
    Tui {
        #[arg(long)]
        deck: Option<std::path::PathBuf>,
        #[arg(long, default_value = "modern")]
        format: String,
        #[arg(long, default_value_t = 1000)]
        games: u32,
    },
    /// Sync tournament data and print the computed metagame.
    FetchMeta {
        /// Format: standard, pioneer, modern, legacy, vintage, pauper,
        /// commander.
        #[arg(long, default_value = "modern")]
        format: String,
        /// Trailing window in days.
        #[arg(long, default_value_t = 60)]
        days: i64,
        /// How many archetypes to keep.
        #[arg(long, default_value_t = 12)]
        top: usize,
    },
    /// Run your deck against the format's meta gauntlet.
    Run {
        /// Your decklist file.
        #[arg(long)]
        deck: std::path::PathBuf,
        #[arg(long, default_value = "modern")]
        format: String,
        /// Games per matchup: a number (cap; a decided matchup may stop at
        /// the 200-game floor) or "auto" (play until the CI is tighter than
        /// --precision).
        #[arg(long, default_value = "1000")]
        games: String,
        /// Auto mode target: CI half-width in percentage points.
        #[arg(long, default_value_t = 1.0)]
        precision: f64,
        #[arg(long, default_value_t = 60)]
        days: i64,
        #[arg(long, default_value_t = 12)]
        top: usize,
        #[arg(long, default_value_t = 0x544f4c41524941)]
        seed: u64,
        /// Write full results as JSON.
        #[arg(long)]
        json: Option<std::path::PathBuf>,
        /// Play every requested game even after the result is decided.
        #[arg(long)]
        no_early_stop: bool,
    },
    /// Simulate 4-player Commander pods against the EDHREC meta.
    Pod {
        /// Your Commander decklist file (Commander section or first card).
        #[arg(long)]
        deck: std::path::PathBuf,
        #[arg(long, default_value_t = 250)]
        games: u32,
        #[arg(long, default_value_t = 10)]
        top: usize,
        #[arg(long, default_value_t = 0x544f4c41524941)]
        seed: u64,
    },
    /// Simulate one deck against another, both from decklist files.
    Duel {
        /// Your decklist file.
        #[arg(long)]
        deck: std::path::PathBuf,
        /// The opposing decklist file.
        #[arg(long)]
        vs: std::path::PathBuf,
        /// Games: a number (cap) or "auto" (play until the CI is tighter
        /// than --precision).
        #[arg(long, default_value = "1000")]
        games: String,
        /// Auto mode target: CI half-width in percentage points.
        #[arg(long, default_value_t = 1.0)]
        precision: f64,
        /// Master seed; same seed reproduces identical results.
        #[arg(long, default_value_t = 0x544f4c41524941)]
        seed: u64,
        /// Disable early stopping.
        #[arg(long)]
        no_early_stop: bool,
        /// Enumerate every distinct opening hand of your deck, exactly
        /// weighted, with sampled continuations per hand.
        #[arg(long)]
        all_hands: bool,
        /// Continuations per hand in --all-hands mode.
        #[arg(long, default_value_t = 50)]
        per_hand: u32,
    },
}

/// "auto" or a number. Auto returns a million-game ceiling; precision does
/// the real stopping.
fn parse_games(s: &str) -> Result<(u32, bool)> {
    if s.eq_ignore_ascii_case("auto") {
        return Ok((1_000_000, true));
    }
    Ok((s.parse::<u32>().map_err(|_| anyhow::anyhow!("--games takes a number or 'auto'"))?, false))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Fetch { force }) => cmd_fetch(force),
        Some(Command::Card { name }) => cmd_card(&name.join(" ")),
        Some(Command::Compile { name }) => cmd_compile(&name.join(" ")),
        Some(Command::Coverage) => cmd_coverage(),
        Some(Command::Duel {
            deck,
            vs,
            games,
            precision,
            seed,
            no_early_stop,
            all_hands,
            per_hand,
        }) => cmd_duel(&deck, &vs, &games, precision, seed, !no_early_stop, all_hands, per_hand),
        Some(Command::FetchMeta { format, days, top }) => {
            let (pool, _) = load_pool(false, false)?;
            let meta = load_meta(&pool, &format, days, top, true)?;
            print_meta(&meta);
            Ok(())
        }
        Some(Command::Run {
            deck,
            format,
            games,
            precision,
            days,
            top,
            seed,
            json,
            no_early_stop,
        }) => cmd_run(
            &deck,
            &format,
            &games,
            precision,
            days,
            top,
            seed,
            json.as_deref(),
            !no_early_stop,
        ),
        Some(Command::Pod { deck, games, top, seed }) => cmd_pod(&deck, games, top, seed),
        Some(Command::Tui { deck, format, games }) => launch_tui(deck, format, games),
        None => {
            let t = cli.top;
            match t.deck {
                // Flags without a subcommand mean a headless gauntlet run.
                Some(deck) => cmd_run(
                    &deck,
                    &t.format,
                    &t.games,
                    t.precision,
                    t.days,
                    t.top,
                    t.seed,
                    t.json.as_deref(),
                    !t.no_early_stop,
                ),
                None => launch_tui(None, t.format, t.games.parse().unwrap_or(1000)),
            }
        }
    }
}

fn cmd_pod(deck: &std::path::Path, games: u32, top: usize, seed: u64) -> Result<()> {
    let (pool, _) = load_pool(false, false)?;
    let user = mtg_sources::load_deck_file(&pool, deck)?;
    let mut user_sim = to_sim_deck(&user, 1.0);
    if user_sim.commander.is_none() {
        // Convention: the first card is the commander when no section says.
        if let Some((first, _)) = user_sim.cards.first().copied() {
            user_sim.commander = Some(first);
            if let Some(slot) = user_sim.cards.iter_mut().find(|(id, _)| *id == first) {
                slot.1 = slot.1.saturating_sub(1);
            }
            user_sim.cards.retain(|(_, c)| *c > 0);
        }
    }
    let meta = load_meta(&pool, "commander", 60, top, true)?;
    if meta.len() < 3 {
        anyhow::bail!("need at least 3 commander meta decks");
    }
    print_meta(&meta);
    let cfg = mtg_sim::SimConfig {
        games_cap: games,
        floor: games,
        early_stop: false,
        precision_target: None,
        master_seed: seed,
        rules: mtg_engine::RulesConfig::commander_pod(4),
    };
    let progress = std::sync::Arc::new(mtg_sim::MatchupProgress::default());
    let started = std::time::Instant::now();
    let stats = mtg_sim::run_pod(&pool, &user_sim, &meta, &cfg, &progress);
    let elapsed = started.elapsed().as_secs_f64();
    let (lo, hi) = stats.ci95();
    println!(
        "\n{} in 4-player pods: {} games in {:.1}s ({:.0} games/s)",
        user_sim.name,
        stats.games,
        elapsed,
        stats.games as f64 / elapsed.max(0.0001)
    );
    println!(
        "seat win rate {:.1}% (95% CI {:.1}..{:.1}); even pod baseline is 25%",
        stats.win_rate() * 100.0,
        lo * 100.0,
        hi * 100.0
    );
    println!(
        "wins {} / losses {} / draws {} / panics {} / avg {:.1} turns",
        stats.wins, stats.losses, stats.draws, stats.panics, stats.avg_turns()
    );
    Ok(())
}

fn launch_tui(deck: Option<std::path::PathBuf>, format: String, games: u32) -> Result<()> {
    mtg_tui::run_tui(mtg_tui::TuiArgs {
        deck,
        format,
        games,
        days: 60,
        top: 12,
        seed: 0x544f4c41524941,
    })
}

/// Sync sources and compute the meta gauntlet, printing status lines.
fn load_meta(
    pool: &mtg_data::CardPool,
    format_str: &str,
    days: i64,
    top: usize,
    verbose: bool,
) -> Result<Vec<mtg_sim::SimDeck>> {
    let mut status = |s: String| {
        if verbose {
            println!("{s}");
        }
    };
    mtg_tui::meta_loader::load_meta(pool, format_str, days, top, &mut status)
}

fn print_meta(meta: &[mtg_sim::SimDeck]) {
    println!("\nmeta gauntlet ({} decks):", meta.len());
    for m in meta {
        println!(
            "  {:>5.1}%  {}{}",
            m.meta_share * 100.0,
            m.name,
            if m.pilot_warning { "  [low pilot fidelity]" } else { "" }
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_run(
    deck: &std::path::Path,
    format: &str,
    games_str: &str,
    precision: f64,
    days: i64,
    top: usize,
    seed: u64,
    json: Option<&std::path::Path>,
    early_stop: bool,
) -> Result<()> {
    let (games, auto) = parse_games(games_str)?;
    let (pool, _) = load_pool(false, false)?;
    let user = mtg_sources::load_deck_file(&pool, deck)?;
    let is_commander = mtg_data::Format::parse(format) == Some(mtg_data::Format::Commander);
    let user_sim = to_sim_deck(&user, 1.0);

    let meta = load_meta(&pool, format, days, top, true)?;
    if meta.is_empty() {
        anyhow::bail!("no meta decks resolved for {format}");
    }
    print_meta(&meta);

    let (_, _, coverages) = mtg_sim::build_db(&pool, &[&user_sim]);
    println!(
        "\n{}: {} cards, coverage {:.0}% full / {:.0}% playable",
        user_sim.name,
        coverages[0].total(),
        coverages[0].full_frac() * 100.0,
        coverages[0].playable_frac() * 100.0
    );

    let rules = if is_commander {
        mtg_engine::RulesConfig::commander_pod(2)
    } else {
        mtg_engine::RulesConfig::duel()
    };
    let cfg = mtg_sim::SimConfig {
        games_cap: games,
        floor: if auto { 1000.min(games) } else { 200.min(games) },
        early_stop,
        precision_target: auto.then_some(precision / 100.0),
        master_seed: seed,
        rules,
    };
    let progress: Vec<std::sync::Arc<mtg_sim::MatchupProgress>> =
        (0..meta.len()).map(|_| Default::default()).collect();

    let started = std::time::Instant::now();
    let mut stats = mtg_sim::run_gauntlet(&pool, &user_sim, &meta, &cfg, &progress);
    stats.format = format.to_string();
    let elapsed = started.elapsed().as_secs_f64();

    println!(
        "\nresults: {} games in {:.2}s ({:.0} games/s)",
        stats.total_games(),
        elapsed,
        stats.total_games() as f64 / elapsed.max(0.000_1)
    );
    println!(
        "{:<38} {:>6} {:>7} {:>15} {:>7} {:>7} {:>8}",
        "matchup", "share", "games", "win rate", "play", "draw", "opp cov"
    );
    for m in stats.sorted_worst_first() {
        let (lo, hi) = m.ci95();
        println!(
            "{:<38} {:>5.1}% {:>7} {:>5.1}% ({:>4.1}..{:<4.1}) {:>6.1}% {:>6.1}% {:>7.0}%{}{}",
            m.opponent,
            m.meta_share * 100.0,
            m.games,
            m.win_rate() * 100.0,
            lo * 100.0,
            hi * 100.0,
            m.on_play_rate() * 100.0,
            m.on_draw_rate() * 100.0,
            m.opp_coverage_playable_frac * 100.0,
            if m.stopped_early { " *" } else { "" },
            if m.opp_pilot_warning { " !" } else { "" },
        );
    }
    println!(
        "\nweighted win rate vs the field: {:.1}%",
        stats.weighted_win_rate() * 100.0
    );
    println!("(* early stop, ! low pilot fidelity, opp cov = opponent playable coverage)");
    let avg_cov: f64 = stats
        .matchups
        .iter()
        .map(|m| m.opp_coverage_playable_frac)
        .sum::<f64>()
        / stats.matchups.len().max(1) as f64;
    if avg_cov < 0.85 {
        println!(
            "warning: average opponent coverage is {:.0}%; treat absolute win rates with care",
            avg_cov * 100.0
        );
    }
    explain_stopping(&stats.matchups, games, auto, precision, early_stop);

    if let Some(path) = json {
        std::fs::write(path, serde_json::to_vec_pretty(&stats)?)?;
        println!("wrote {}", path.display());
    }
    Ok(())
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

#[allow(clippy::too_many_arguments)]
fn cmd_duel(
    deck: &std::path::Path,
    vs: &std::path::Path,
    games_str: &str,
    precision: f64,
    seed: u64,
    early_stop: bool,
    all_hands: bool,
    per_hand: u32,
) -> Result<()> {
    let (games, auto) = parse_games(games_str)?;
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
        floor: if auto { 1000.min(games) } else { 200.min(games) },
        early_stop,
        precision_target: auto.then_some(precision / 100.0),
        master_seed: seed,
        rules: mtg_engine::RulesConfig::duel(),
    };

    if all_hands {
        return run_sweep(&pool, &user_sim, &opp_sim, &cfg, per_hand);
    }

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
        "win rate {:.1}% (95% CI {:.1}%..{:.1}%)",
        stats.win_rate() * 100.0,
        lo * 100.0,
        hi * 100.0,
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
    explain_stopping(&[stats], games, auto, precision, early_stop);
    Ok(())
}

/// Say plainly why fewer games than requested were played.
fn explain_stopping(
    matchups: &[mtg_stats::MatchupStats],
    cap: u32,
    auto: bool,
    precision: f64,
    early_stop: bool,
) {
    let stopped: usize = matchups.iter().filter(|m| m.stopped_early).count();
    if stopped == 0 {
        return;
    }
    if auto {
        println!(
            "{stopped} matchup(s) reached the +/-{precision:.1}% precision target before the \
             {cap}-game ceiling."
        );
    } else if early_stop {
        println!(
            "{stopped} matchup(s) stopped early: the 95% CI cleared 50%, so more games would \
             not change the verdict. Pass --no-early-stop to play all {cap}, or --games auto \
             to target a precision instead."
        );
    }
}

fn run_sweep(
    pool: &mtg_data::CardPool,
    user: &mtg_sim::SimDeck,
    opp: &mtg_sim::SimDeck,
    cfg: &mtg_sim::SimConfig,
    per_hand: u32,
) -> Result<()> {
    let n_hands = mtg_sim::sweep::count_hands(&user.cards, 7);
    println!(
        "\n{} has {} distinct opening hands; {} continuations each = {} games",
        user.name,
        n_hands,
        per_hand,
        n_hands.saturating_mul(per_hand as u128)
    );
    if n_hands as usize > mtg_sim::sweep::MAX_SWEEP_HANDS {
        anyhow::bail!(
            "{} distinct hands is past the sweep limit ({}). Singleton decks explode \
             combinatorially; use Monte Carlo (--games auto) instead.",
            n_hands,
            mtg_sim::sweep::MAX_SWEEP_HANDS
        );
    }
    let progress = std::sync::Arc::new(mtg_sim::MatchupProgress::default());
    let started = std::time::Instant::now();
    let sweep = mtg_sim::sweep::run_hand_sweep(pool, user, opp, cfg, per_hand, &progress);
    let elapsed = started.elapsed().as_secs_f64();
    let (lo, hi) = sweep.ci95();
    println!(
        "swept {} hands, {} games in {:.1}s ({:.0} games/s), {} panics",
        sweep.distinct_hands,
        sweep.total_games,
        elapsed,
        sweep.total_games as f64 / elapsed.max(0.0001),
        sweep.panics
    );
    println!(
        "hand-exact weighted win rate: {:.2}% (95% CI {:.2}%..{:.2}%)",
        sweep.weighted_win_rate * 100.0,
        lo * 100.0,
        hi * 100.0
    );

    let mut ranked: Vec<&mtg_sim::sweep::HandOutcome> = sweep.hands.iter().collect();
    ranked.sort_by(|a, b| a.win_rate().partial_cmp(&b.win_rate()).unwrap());
    let fmt_hand = |h: &mtg_sim::sweep::HandOutcome| -> String {
        let cards: Vec<String> = h
            .cards
            .iter()
            .map(|(id, n)| {
                let name = &pool.get(*id).name;
                if *n > 1 {
                    format!("{n}x {name}")
                } else {
                    name.to_string()
                }
            })
            .collect();
        cards.join(", ")
    };
    println!("\nworst opening hands (dealt probability, win rate):");
    for h in ranked.iter().take(5) {
        println!("  {:>6.3}%  {:>5.1}%  {}", h.probability * 100.0, h.win_rate() * 100.0, fmt_hand(h));
    }
    println!("best opening hands:");
    for h in ranked.iter().rev().take(5) {
        println!("  {:>6.3}%  {:>5.1}%  {}", h.probability * 100.0, h.win_rate() * 100.0, fmt_hand(h));
    }
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
