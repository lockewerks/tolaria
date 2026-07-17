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
    /// Gauntlet size: a number or "all".
    #[arg(long, default_value = "12")]
    top: String,
    /// Draw archetypes at random from the eligible universe.
    #[arg(long)]
    random: bool,
    /// Master seed; omit for a fresh random seed (printed for reproduction).
    #[arg(long)]
    seed: Option<u64>,
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
    /// List what the simulator does not model: the divergence ledger.
    Limits {
        /// Emit as Markdown (for docs/limits.md).
        #[arg(long)]
        markdown: bool,
    },
    /// Compile the entire card pool and print the coverage histogram.
    Coverage {
        /// Also compute play-weighted coverage of this format's cached
        /// tournament decklists (the number that actually matters).
        #[arg(long)]
        format: Option<String>,
        /// Show the top-N dropped-clause patterns: the template backlog,
        /// ranked by played copies when --format is given.
        #[arg(long)]
        gaps: Option<usize>,
        /// Trailing window in days for the tournament cache.
        #[arg(long, default_value_t = 60)]
        days: i64,
        /// Write the full report as JSON.
        #[arg(long)]
        json: Option<std::path::PathBuf>,
    },
    /// Compare simulated matchup win rates against real tournament match
    /// results from the local cache: the accuracy report card.
    Calibrate {
        #[arg(long, default_value = "modern")]
        format: String,
        /// Trailing window in days.
        #[arg(long, default_value_t = 60)]
        days: i64,
        /// Minimum real games an archetype pair needs to qualify.
        #[arg(long, default_value_t = 50)]
        min_games: u32,
        /// Master seed; omit for a fresh random seed (printed for
        /// reproduction).
        #[arg(long)]
        seed: Option<u64>,
        /// Also write the report JSON here (it lands in the data dir
        /// regardless).
        #[arg(long)]
        json: Option<std::path::PathBuf>,
    },
    /// Goldfish: play against a passive opponent to measure the deck as it
    /// stands (kill turn, consistency). Any deck size.
    Goldfish {
        #[arg(long)]
        deck: std::path::PathBuf,
        #[arg(long, default_value_t = 1000)]
        games: u32,
        /// Master seed; omit for a fresh random seed (printed for
        /// reproduction).
        #[arg(long)]
        seed: Option<u64>,
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
        /// Gauntlet size: a number or "all" (every eligible archetype).
        #[arg(long, default_value = "12")]
        top: String,
        /// Draw the archetypes at random from the eligible universe instead
        /// of taking the most-played.
        #[arg(long)]
        random: bool,
        /// Master seed pinning random draws; omit for a fresh one.
        #[arg(long)]
        seed: Option<u64>,
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
        /// Gauntlet size: a number or "all" (every eligible archetype).
        #[arg(long, default_value = "12")]
        top: String,
        /// Draw archetypes at random from the eligible universe.
        #[arg(long)]
        random: bool,
        /// Master seed; omit for a fresh random seed (printed for
        /// reproduction).
        #[arg(long)]
        seed: Option<u64>,
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
        /// Master seed; omit for a fresh random seed (printed for
        /// reproduction).
        #[arg(long)]
        seed: Option<u64>,
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
        /// Master seed; omit for a fresh random seed (printed for
        /// reproduction).
        #[arg(long)]
        seed: Option<u64>,
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

/// Use the given seed, or roll fresh entropy and say so. Masked to 53 bits
/// to match what the desktop app can round-trip through JavaScript.
fn resolve_seed(seed: Option<u64>) -> u64 {
    match seed {
        Some(s) => s,
        None => {
            let s = rand::random::<u64>() & ((1u64 << 53) - 1);
            println!("master seed: {s} (pass --seed {s} to reproduce this run)");
            s
        }
    }
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
        Some(Command::Coverage { format, gaps, days, json }) => {
            cmd_coverage(format.as_deref(), gaps, days, json.as_deref())
        }
        Some(Command::Calibrate { format, days, min_games, seed, json }) => {
            cmd_calibrate(&format, days, min_games, seed, json.as_deref())
        }
        Some(Command::Limits { markdown }) => cmd_limits(markdown),
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
        Some(Command::FetchMeta { format, days, top, random, seed }) => {
            let (pool, _) = load_pool(false, false)?;
            let selection = mtg_sim::meta_loader::MetaSelection::parse(&top, random)?;
            let seed = resolve_seed(seed);
            let meta = load_meta(&pool, &format, days, selection, seed, true)?;
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
            random,
            seed,
            json,
            no_early_stop,
        }) => cmd_run(
            &deck,
            &format,
            &games,
            precision,
            days,
            &top,
            random,
            seed,
            json.as_deref(),
            !no_early_stop,
        ),
        Some(Command::Pod { deck, games, top, seed }) => cmd_pod(&deck, games, top, seed),
        Some(Command::Goldfish { deck, games, seed }) => cmd_goldfish(&deck, games, seed),
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
                    &t.top,
                    t.random,
                    t.seed,
                    t.json.as_deref(),
                    !t.no_early_stop,
                ),
                None => launch_desktop(),
            }
        }
    }
}

fn cmd_pod(deck: &std::path::Path, games: u32, top: usize, seed: Option<u64>) -> Result<()> {
    let seed = resolve_seed(seed);
    let (pool, _) = load_pool(false, false)?;
    let user = mtg_sources::load_deck_file(&pool, deck)?;
    let mut user_sim = to_sim_deck(&pool, &user, 1.0);
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
    let meta = load_meta(
        &pool,
        "commander",
        60,
        mtg_sim::meta_loader::MetaSelection::Top(top),
        seed,
        true,
    )?;
    if meta.len() < 3 {
        anyhow::bail!("need at least 3 commander meta decks");
    }
    print_meta(&meta);
    let (_, _, coverages) = mtg_sim::build_db(&pool, &[&user_sim]);
    println!(
        "\n{}: {} cards, coverage {:.0}% full / {:.0}% playable{}",
        user_sim.name,
        coverages[0].total(),
        coverages[0].full_frac() * 100.0,
        coverages[0].playable_frac() * 100.0,
        pilot_suffix(&user_sim)
    );
    let cfg = mtg_sim::SimConfig {
        games_cap: games,
        floor: games,
        early_stop: false,
        precision_target: None,
        cancel: None,
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

/// Bare `tolaria` opens the desktop app when it sits next to this binary.
fn launch_desktop() -> Result<()> {
    let exe = std::env::current_exe()?;
    let desktop = exe.with_file_name("tolaria-desktop.exe");
    if desktop.exists() {
        std::process::Command::new(desktop).spawn()?;
        Ok(())
    } else {
        println!(
            "tolaria-desktop.exe not found next to this binary.\n\
             Build it with: cargo build --release -p tolaria-desktop\n\
             Headless usage: tolaria --help"
        );
        Ok(())
    }
}

fn cmd_goldfish(deck: &std::path::Path, games: u32, seed: Option<u64>) -> Result<()> {
    let seed = resolve_seed(seed);
    let (pool, _) = load_pool(false, false)?;
    let user = mtg_sources::load_deck_file(&pool, deck)?;
    let user_sim = to_sim_deck(&pool, &user, 1.0);
    let (_, _, coverages) = mtg_sim::build_db(&pool, &[&user_sim]);
    println!(
        "{}: {} cards, coverage {:.0}% full / {:.0}% playable{}",
        user_sim.name,
        coverages[0].total(),
        coverages[0].full_frac() * 100.0,
        coverages[0].playable_frac() * 100.0,
        pilot_suffix(&user_sim)
    );
    let cfg = mtg_sim::SimConfig {
        games_cap: games,
        floor: games,
        early_stop: false,
        precision_target: None,
        cancel: None,
        master_seed: seed,
        rules: mtg_engine::RulesConfig::duel(),
    };
    let progress = std::sync::Arc::new(mtg_sim::MatchupProgress::default());
    let started = std::time::Instant::now();
    let g = mtg_sim::goldfish::run_goldfish(&pool, &user_sim, &cfg, &progress);
    let elapsed = started.elapsed().as_secs_f64();
    println!(
        "{}: {} goldfish games in {:.2}s ({:.0}/s), {} panics",
        user_sim.name,
        g.games,
        elapsed,
        g.games as f64 / elapsed.max(0.0001),
        g.panics
    );
    println!(
        "kills {} / no-kill-by-cap {}; average kill on your turn {:.2}",
        g.kills, g.no_kill, g.avg_kill_turn
    );
    println!(
        "killed by turn 4: {:.1}%, turn 5: {:.1}%, turn 6: {:.1}%, turn 8: {:.1}%",
        g.kill_by(4) * 100.0,
        g.kill_by(5) * 100.0,
        g.kill_by(6) * 100.0,
        g.kill_by(8) * 100.0
    );
    let total_mulls: u32 = g.mull_hist.iter().enumerate().map(|(i, n)| i as u32 * n).sum();
    println!(
        "mulligans: {:.2} per game ({} kept 7)",
        total_mulls as f64 / g.games.max(1) as f64,
        g.mull_hist.first().copied().unwrap_or(0)
    );
    Ok(())
}

/// Sync sources and compute the meta gauntlet, printing status lines.
fn load_meta(
    pool: &mtg_data::CardPool,
    format_str: &str,
    days: i64,
    selection: mtg_sim::meta_loader::MetaSelection,
    seed: u64,
    verbose: bool,
) -> Result<Vec<mtg_sim::SimDeck>> {
    let mut status = |s: String| {
        if verbose {
            println!("{s}");
        }
    };
    let (decks, info) =
        mtg_sim::meta_loader::load_meta(pool, format_str, days, selection, seed, &mut status)?;
    if verbose && info.randomized {
        println!(
            "randomly drew {} of {} eligible archetypes",
            info.selected, info.eligible
        );
    }
    Ok(decks)
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
    top: &str,
    random: bool,
    seed: Option<u64>,
    json: Option<&std::path::Path>,
    early_stop: bool,
) -> Result<()> {
    let seed = resolve_seed(seed);
    let (games, auto) = parse_games(games_str)?;
    let (pool, _) = load_pool(false, false)?;
    let user = mtg_sources::load_deck_file(&pool, deck)?;
    let is_commander = mtg_data::Format::parse(format) == Some(mtg_data::Format::Commander);
    let user_sim = to_sim_deck(&pool, &user, 1.0);

    let selection = mtg_sim::meta_loader::MetaSelection::parse(top, random)?;
    let meta = load_meta(&pool, format, days, selection, seed, true)?;
    if meta.is_empty() {
        anyhow::bail!("no meta decks resolved for {format}");
    }
    print_meta(&meta);

    let (_, _, coverages) = mtg_sim::build_db(&pool, &[&user_sim]);
    println!(
        "\n{}: {} cards, coverage {:.0}% full / {:.0}% playable{}",
        user_sim.name,
        coverages[0].total(),
        coverages[0].full_frac() * 100.0,
        coverages[0].playable_frac() * 100.0,
        pilot_suffix(&user_sim)
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
        cancel: None,
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

fn to_sim_deck(
    pool: &mtg_data::CardPool,
    d: &mtg_sources::ResolvedDeck,
    share: f64,
) -> mtg_sim::SimDeck {
    // The same crude heuristic meta opponents get; the user's own deck is
    // not exempt from it.
    let creatures = mtg_sim::meta_loader::creature_count(pool, &d.main);
    mtg_sim::SimDeck {
        name: d.name.clone(),
        cards: d.main.clone(),
        commander: d.commander,
        meta_share: share,
        pilot_warning: mtg_sources::meta::pilot_warning(creatures),
    }
}

/// Coverage-line suffix confessing when the greedy pilot is suspect.
fn pilot_suffix(d: &mtg_sim::SimDeck) -> &'static str {
    if d.pilot_warning {
        ", low pilot fidelity (under 10 creatures; the greedy pilot may misplay this list)"
    } else {
        ""
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_duel(
    deck: &std::path::Path,
    vs: &std::path::Path,
    games_str: &str,
    precision: f64,
    seed: Option<u64>,
    early_stop: bool,
    all_hands: bool,
    per_hand: u32,
) -> Result<()> {
    let seed = resolve_seed(seed);
    let (games, auto) = parse_games(games_str)?;
    let (pool, _) = load_pool(false, false)?;
    let user = mtg_sources::load_deck_file(&pool, deck)?;
    let opp = mtg_sources::load_deck_file(&pool, vs)?;
    let user_sim = to_sim_deck(&pool, &user, 1.0);
    let opp_sim = to_sim_deck(&pool, &opp, 1.0);

    let (_, _, coverages) = mtg_sim::build_db(&pool, &[&user_sim, &opp_sim]);
    println!(
        "{}: {} cards, coverage {:.0}% full / {:.0}% playable{}",
        user_sim.name,
        coverages[0].total(),
        coverages[0].full_frac() * 100.0,
        coverages[0].playable_frac() * 100.0,
        pilot_suffix(&user_sim)
    );
    println!(
        "{}: {} cards, coverage {:.0}% full / {:.0}% playable{}",
        opp_sim.name,
        coverages[1].total(),
        coverages[1].full_frac() * 100.0,
        coverages[1].playable_frac() * 100.0,
        pilot_suffix(&opp_sim)
    );

    let cfg = mtg_sim::SimConfig {
        games_cap: games,
        floor: if auto { 1000.min(games) } else { 200.min(games) },
        early_stop,
        precision_target: auto.then_some(precision / 100.0),
        cancel: None,
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
    if stats.wins > 0 {
        println!(
            "your wins end with you at {:+.1} life, opponent at {:+.1}",
            stats.avg_win_life(),
            stats.avg_win_opp_life()
        );
    }
    if stats.losses > 0 {
        println!(
            "your losses end with you at {:+.1} life, opponent at {:+.1}",
            stats.avg_loss_life(),
            stats.avg_loss_opp_life()
        );
    }
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

fn print_tier_histogram(label: &str, stats: &mtg_cards::CoverageStats) {
    let total = stats.total().max(1) as f64;
    println!("{label} ({} cards):", stats.total());
    println!("  full:       {:>6} ({:.1}%)", stats.full, stats.full as f64 / total * 100.0);
    println!("  partial:    {:>6} ({:.1}%)", stats.partial, stats.partial as f64 / total * 100.0);
    println!("  proxy:      {:>6} ({:.1}%)", stats.proxy, stats.proxy as f64 / total * 100.0);
    println!(
        "  unplayable: {:>6} ({:.1}%)",
        stats.unplayable,
        stats.unplayable as f64 / total * 100.0
    );
}

fn cmd_coverage(
    format: Option<&str>,
    gaps: Option<usize>,
    days: i64,
    json: Option<&std::path::Path>,
) -> Result<()> {
    let (pool, _) = load_pool(false, false)?;
    let started = std::time::Instant::now();
    let comp = mtg_cards::compile_pool_detailed(&pool, |_| true);
    println!("compiled {} cards in {:.2}s", comp.stats.total(), started.elapsed().as_secs_f32());
    print_tier_histogram("whole pool", &comp.stats);

    // Format-legal subset of the pool, when asked.
    let format_stats = match format.and_then(mtg_data::Format::parse) {
        Some(f) => {
            let sub = mtg_cards::compile_pool_detailed(&pool, |c| c.legalities.is_legal(f));
            println!();
            print_tier_histogram(&format!("legal in {f}"), &sub.stats);
            Some(sub.stats)
        }
        None => None,
    };

    // Play-weighted coverage from the cached tournament decklists: the
    // honest headline. Printed beside the pool number, never instead of it.
    let meta = match format {
        Some(f) => {
            let m = mtg_sim::coverage::meta_coverage(&pool, f, days)?;
            println!(
                "\nas played in {} ({} decks, last {} days): {} card-slots",
                m.format, m.decks, m.window_days, m.total_copies
            );
            println!(
                "  meta coverage: {:.1}% full / {:.1}% playable (copies-weighted)",
                m.full_frac() * 100.0,
                m.playable_frac() * 100.0
            );
            if m.unresolved_copies > 0 {
                let line = format!(
                    "  unresolved names: {} copies ({:.1}%)",
                    m.unresolved_copies,
                    m.unresolved_frac() * 100.0
                );
                if m.unresolved_frac() > 0.02 {
                    println!("{line}  <- over 2%, treat the meta numbers with suspicion");
                } else {
                    println!("{line}");
                }
            }
            println!("\n  most-played cards below Full:");
            for g in m.top_card_gaps.iter().take(12) {
                println!("    {:>6} copies  {:<10} {}", g.copies, g.tier, g.name);
            }
            Some(m)
        }
        None => None,
    };

    // The ranked template backlog.
    let pool_gaps = gaps.map(|n| {
        let all = mtg_cards::gaps::aggregate_gaps(&pool, &comp);
        println!("\ntop dropped-clause patterns ({} distinct):", all.len());
        match &meta {
            Some(m) => {
                println!("  {:>6}  {:>11}  pattern", "cards", "meta copies");
                for g in m.clause_gaps.iter().take(n) {
                    println!("  {:>6}  {:>11}  {}", g.cards, g.meta_copies, g.pattern);
                    println!("          e.g. {}", g.example_cards.join(", "));
                }
            }
            None => {
                println!("  {:>6}  pattern", "cards");
                for g in all.iter().take(n) {
                    println!("  {:>6}  {}", g.cards, g.pattern);
                    println!("          e.g. {}", g.example_cards.join(", "));
                }
            }
        }
        all
    });

    if let Some(path) = json {
        #[derive(serde::Serialize)]
        struct Tiers {
            full: usize,
            partial: usize,
            proxy: usize,
            unplayable: usize,
        }
        #[derive(serde::Serialize)]
        struct GapRow {
            pattern: String,
            cards: u32,
            example_cards: Vec<String>,
            example_text: String,
        }
        #[derive(serde::Serialize)]
        struct Report {
            pool: Tiers,
            format_pool: Option<Tiers>,
            meta: Option<mtg_sim::coverage::MetaCoverage>,
            pool_gaps: Option<Vec<GapRow>>,
        }
        let tiers = |s: &mtg_cards::CoverageStats| Tiers {
            full: s.full,
            partial: s.partial,
            proxy: s.proxy,
            unplayable: s.unplayable,
        };
        let report = Report {
            pool: tiers(&comp.stats),
            format_pool: format_stats.as_ref().map(tiers),
            meta,
            pool_gaps: pool_gaps.map(|v| {
                v.into_iter()
                    .map(|g| GapRow {
                        pattern: g.pattern,
                        cards: g.cards,
                        example_cards: g.example_cards,
                        example_text: g.example_text,
                    })
                    .collect()
            }),
        };
        std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
        println!("\nwrote {}", path.display());
    }
    Ok(())
}

fn cmd_limits(markdown: bool) -> Result<()> {
    let limits = mtg_sim::limits::all_limits();
    if markdown {
        println!("# What Tolaria does not model\n");
        println!(
            "The simulator is an honestly scoped subset of Magic. Every item below \
             is a known divergence, with the direction it pushes the numbers. This \
             file is generated by `tolaria limits --markdown`.\n"
        );
        let mut current = "";
        for l in &limits {
            if l.category.label() != current {
                current = l.category.label();
                println!("\n## {current}\n");
            }
            let rule = if l.rule_ref == "-" { String::new() } else { format!(" ({})", l.rule_ref) };
            println!("- **{}**{}: {}. _{}_", l.id, rule, l.summary, l.impact);
        }
        return Ok(());
    }
    let mut current = "";
    for l in &limits {
        if l.category.label() != current {
            current = l.category.label();
            println!("\n{current}:");
        }
        let rule = if l.rule_ref == "-" { String::new() } else { format!("  [{}]", l.rule_ref) };
        println!("  {}{}", l.summary, rule);
        println!("      -> {}", l.impact);
    }
    println!("\n{} limitations across {} categories", limits.len(), 5);
    Ok(())
}

fn cmd_calibrate(
    format: &str,
    days: i64,
    min_games: u32,
    seed: Option<u64>,
    json: Option<&std::path::Path>,
) -> Result<()> {
    let seed = resolve_seed(seed);
    let (pool, _) = load_pool(false, false)?;
    let started = std::time::Instant::now();
    let mut status = |s: String| println!("{s}");
    let report = mtg_sim::calibrate::run_calibration(&pool, format, days, min_games, seed, &mut status)?;

    println!(
        "\ncalibration: {} ({} days): {} tournaments, {} with round data",
        report.format, report.window_days, report.tournaments, report.tournaments_with_rounds
    );
    println!(
        "matches: {} total, {} used; skipped {} byes, {} unjoined, {} unclassified, \
         {} malformed, {} mirrors, {} draw-only",
        report.matches_total,
        report.matches_used,
        report.matches_skipped_bye,
        report.matches_skipped_unjoined,
        report.matches_skipped_unclassified,
        report.matches_skipped_malformed,
        report.matches_skipped_mirror,
        report.draw_only_matches
    );
    println!(
        "\n{:<44} {:>14} {:>22} {:>22} {:>7}",
        "pair (A vs B)", "real games", "real WR(A)", "sim WR(A)", "diverge"
    );
    for p in &report.pairs {
        let flags = format!(
            "{}{}{}",
            if !p.ci_overlap { " CI!" } else { "" },
            if p.a_coverage_playable < 0.85 || p.b_coverage_playable < 0.85 { " cov" } else { "" },
            if p.a_pilot_warning || p.b_pilot_warning { " pilot" } else { "" },
        );
        println!(
            "{:<44} {:>14} {:>6.1}% ({:>4.1}..{:<4.1}) {:>6.1}% ({:>4.1}..{:<4.1}) {:>+6.1}%{}",
            format!("{} vs {}", p.a, p.b),
            p.real_games,
            p.real_wr * 100.0,
            p.real_ci.0 * 100.0,
            p.real_ci.1 * 100.0,
            p.sim_wr * 100.0,
            p.sim_ci.0 * 100.0,
            p.sim_ci.1 * 100.0,
            p.divergence * 100.0,
            flags,
        );
    }
    println!(
        "\nmean absolute divergence (games-weighted): {:.1} percentage points",
        report.mean_abs_divergence * 100.0
    );
    println!("real-vs-sim correlation across {} pairs: {:.2}", report.pairs.len(), report.correlation);
    println!("(CI! = intervals disjoint, cov = a side under 85% coverage, pilot = fidelity flag)");
    println!("\nwhy these numbers wobble, structurally:");
    for c in &report.caveats {
        println!("  - {c}");
    }
    println!("\n{} games simulated in {:.1}s", report.pairs.iter().map(|p| p.sim_games).sum::<u32>(), started.elapsed().as_secs_f64());

    let saved = mtg_sim::calibrate::save_report(&report)?;
    println!("saved {}", saved.display());
    if let Some(path) = json {
        std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
        println!("wrote {}", path.display());
    }
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
