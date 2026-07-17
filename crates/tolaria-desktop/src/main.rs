//! Tolaria desktop: Tauri shell over the sim crates. Commands cover deck
//! analysis, the saved-deck library, meta browsing, run orchestration with
//! live progress events, and persisted run history.

#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use mtg_data::CardPool;
use mtg_sim::{MatchupProgress, SimDeck};

// DTOs

#[derive(Serialize, Clone)]
struct PoolInfo {
    cards: usize,
    updated_at: String,
    source: String,
}

#[derive(Serialize, Clone)]
struct CardRow {
    name: String,
    count: u8,
    mana_value: u32,
    type_line: String,
    tier: String,
    dropped: Vec<String>,
}

#[derive(Serialize, Clone)]
struct FormatFit {
    name: String,
    legal_frac: f64,
    size_ok: bool,
}

#[derive(Serialize, Clone)]
struct DeckInfo {
    name: String,
    total: u32,
    rows: Vec<CardRow>,
    full: u32,
    partial: u32,
    proxy: u32,
    unplayable: u32,
    curve: Vec<u32>,
    colors: String,
    unresolved: Vec<String>,
    commander: Option<String>,
    lands: u32,
    avg_mana_value: f64,
    formats: Vec<FormatFit>,
    recommended: String,
    pilot_warning: bool,
}

#[derive(Serialize, Clone)]
struct DeckFile {
    name: String,
    text: String,
}

#[derive(Serialize, Clone)]
struct NameCount {
    name: String,
    count: u8,
}

#[derive(Serialize, Clone)]
struct MetaEntry {
    name: String,
    share: f64,
    pilot_warning: bool,
    playable: f64,
    cards: Vec<NameCount>,
}

#[derive(Deserialize, Clone)]
struct RunConfig {
    mode: String,
    deck_text: String,
    deck_name: String,
    vs_text: Option<String>,
    format: String,
    games: String,
    precision: f64,
    days: i64,
    top: usize,
    /// "top" | "random" | "all".
    #[serde(default = "default_selection")]
    selection: String,
    seed: Option<u64>,
    early_stop: bool,
    per_hand: u32,
}

fn default_selection() -> String {
    "top".into()
}

/// Fresh entropy, masked to 53 bits so the value survives the trip through
/// JavaScript numbers and can be retyped to reproduce the run.
fn roll_seed() -> u64 {
    rand::random::<u64>() & ((1u64 << 53) - 1)
}

fn to_selection(kind: &str, n: usize) -> mtg_sim::meta_loader::MetaSelection {
    match kind {
        "random" => mtg_sim::meta_loader::MetaSelection::Random(n),
        "all" => mtg_sim::meta_loader::MetaSelection::All,
        _ => mtg_sim::meta_loader::MetaSelection::Top(n),
    }
}

#[derive(Serialize, Clone)]
struct MatchupProg {
    name: String,
    done: u32,
    target: u32,
    wins: u32,
    losses: u32,
    draws: u32,
    stopped: bool,
}

#[derive(Serialize, Clone)]
struct ProgressPayload {
    phase: String,
    status: String,
    matchups: Vec<MatchupProg>,
    games_per_sec: f64,
    elapsed: f64,
}

#[derive(Serialize, Deserialize, Clone)]
struct HandDto {
    cards: String,
    probability: f64,
    games: u32,
    win_rate: f64,
}

#[derive(Serialize, Deserialize, Clone)]
struct SweepDto {
    weighted_win_rate: f64,
    standard_error: f64,
    total_games: u64,
    distinct_hands: usize,
    panics: u32,
    best: Vec<HandDto>,
    worst: Vec<HandDto>,
    /// Probability mass per 5% win-rate bucket (20 buckets).
    histogram: Vec<f64>,
}

#[derive(Serialize, Deserialize, Clone)]
struct RunResult {
    kind: String,
    deck_name: String,
    format: String,
    when: u64,
    elapsed: f64,
    cancelled: bool,
    deck_full: f64,
    deck_playable: f64,
    gauntlet: Option<mtg_stats::GauntletStats>,
    sweep: Option<SweepDto>,
    pod: Option<mtg_stats::MatchupStats>,
    #[serde(default)]
    goldfish: Option<mtg_sim::goldfish::GoldfishStats>,
    /// The master seed actually used (rolled when the user left it blank).
    #[serde(default)]
    seed: u64,
    /// The user's own deck trips the low-creature pilot heuristic.
    #[serde(default)]
    deck_pilot_warning: bool,
}

#[derive(Serialize, Clone)]
struct RunMeta {
    file: String,
    deck_name: String,
    format: String,
    kind: String,
    when: u64,
    headline: f64,
    games: u64,
}

// State

#[derive(Default)]
struct AppState {
    pool: Arc<Mutex<Option<Arc<CardPool>>>>,
    cancel: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    running: Arc<AtomicBool>,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn get_pool(slot: &Arc<Mutex<Option<Arc<CardPool>>>>) -> Result<(Arc<CardPool>, PoolInfo), String> {
    {
        let guard = slot.lock().unwrap();
        if let Some(p) = guard.as_ref() {
            return Ok((
                p.clone(),
                PoolInfo { cards: p.len(), updated_at: String::new(), source: "loaded".into() },
            ));
        }
    }
    let paths = mtg_data::Paths::resolve().map_err(|e| e.to_string())?;
    let (pool, status) =
        mtg_data::ensure_pool(&paths, &mtg_data::EnsureOptions::default()).map_err(|e| e.to_string())?;
    let pool = Arc::new(pool);
    *slot.lock().unwrap() = Some(pool.clone());
    Ok((
        pool.clone(),
        PoolInfo {
            cards: pool.len(),
            updated_at: status.updated_at,
            source: format!("{:?}", status.source),
        },
    ))
}

fn decks_dir() -> Result<std::path::PathBuf, String> {
    let paths = mtg_data::Paths::resolve().map_err(|e| e.to_string())?;
    let dir = paths.data_dir.join("decks");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn runs_dir() -> Result<std::path::PathBuf, String> {
    let paths = mtg_data::Paths::resolve().map_err(|e| e.to_string())?;
    let dir = paths.data_dir.join("runs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect::<String>()
        .trim()
        .to_string()
}

fn build_deck_info(pool: &CardPool, text: &str, fallback: &str) -> DeckInfo {
    let parsed = mtg_sources::parse_deck_text(text);
    let name = parsed.name.clone().unwrap_or_else(|| fallback.to_string());
    let (resolved, unresolved) =
        mtg_sources::deck_import::resolve_deck_lossy(pool, &parsed, &name);

    let mut info = DeckInfo {
        name,
        total: 0,
        rows: Vec::new(),
        full: 0,
        partial: 0,
        proxy: 0,
        unplayable: 0,
        curve: vec![0; 8],
        colors: String::new(),
        unresolved,
        commander: None,
        lands: 0,
        avg_mana_value: 0.0,
        formats: Vec::new(),
        recommended: String::new(),
        pilot_warning: false,
    };
    let Some(resolved) = resolved else { return info };
    info.commander = resolved.commander.map(|c| pool.get(c).name.to_string());
    info.pilot_warning = mtg_sources::meta::pilot_warning(
        mtg_sim::meta_loader::creature_count(pool, &resolved.main),
    );

    let mut colors = mtg_ir::ColorSet::empty();
    for (cid, count) in &resolved.main {
        let card = pool.get(*cid);
        let compiled = mtg_cards::compile(card);
        let front = card.front();
        let tier = format!("{:?}", compiled.tier);
        let mv = card.cmc as u32;
        info.total += *count as u32;
        match compiled.tier {
            mtg_ir::CoverageTier::Full => info.full += *count as u32,
            mtg_ir::CoverageTier::Partial => info.partial += *count as u32,
            mtg_ir::CoverageTier::Proxy => info.proxy += *count as u32,
            mtg_ir::CoverageTier::Unplayable => info.unplayable += *count as u32,
        }
        if !front.is_land() {
            let bucket = (mv as usize).min(7);
            info.curve[bucket] += *count as u32;
            if let Some(cost) = mtg_ir::ManaCost::parse(&front.mana_cost) {
                colors |= cost.colors();
            }
        }
        info.rows.push(CardRow {
            name: card.name.to_string(),
            count: *count,
            mana_value: mv,
            type_line: front.type_line.to_string(),
            tier,
            dropped: compiled.dropped.iter().map(|d| d.to_string()).collect(),
        });
    }
    info.rows.sort_by(|a, b| a.mana_value.cmp(&b.mana_value).then(a.name.cmp(&b.name)));

    // Land count and average mana value of nonland cards.
    let mut mv_sum = 0u64;
    let mut nonland = 0u32;
    for (cid, count) in &resolved.main {
        let card = pool.get(*cid);
        if card.front().is_land() {
            info.lands += *count as u32;
        } else {
            nonland += *count as u32;
            mv_sum += card.cmc as u64 * *count as u64;
        }
    }
    info.avg_mana_value = if nonland > 0 { mv_sum as f64 / nonland as f64 } else { 0.0 };

    // Format fit: fraction of the list legal per format, plus size rules.
    // Recommendation is the most restrictive format the deck fully fits.
    for f in mtg_data::Format::ALL {
        let mut legal = 0u32;
        for (cid, count) in &resolved.main {
            let l = pool.get(*cid).legalities;
            if l.is_legal(f) {
                if l.is_restricted(f) && *count > 1 {
                    legal += 1;
                } else {
                    legal += *count as u32;
                }
            }
        }
        let legal_frac = legal as f64 / info.total.max(1) as f64;
        let size_ok = match f {
            mtg_data::Format::Commander => {
                info.commander.is_some() && info.total + 1 >= 100
            }
            _ => info.total >= 60,
        };
        info.formats.push(FormatFit { name: f.to_string(), legal_frac, size_ok });
    }
    for name in ["Standard", "Pauper", "Pioneer", "Modern", "Legacy", "Vintage", "Commander"] {
        if let Some(fit) = info.formats.iter().find(|x| x.name == name) {
            if fit.legal_frac >= 0.999 && fit.size_ok {
                info.recommended = name.to_string();
                break;
            }
        }
    }
    if info.recommended.is_empty() {
        if let Some(best) = info
            .formats
            .iter()
            .filter(|x| x.size_ok)
            .max_by(|a, b| a.legal_frac.partial_cmp(&b.legal_frac).unwrap())
        {
            info.recommended = format!("{} ({:.0}% legal)", best.name, best.legal_frac * 100.0);
        }
    }
    for (c, ch) in [
        (mtg_ir::ColorSet::W, 'W'),
        (mtg_ir::ColorSet::U, 'U'),
        (mtg_ir::ColorSet::B, 'B'),
        (mtg_ir::ColorSet::R, 'R'),
        (mtg_ir::ColorSet::G, 'G'),
    ] {
        if colors.contains(c) {
            info.colors.push(ch);
        }
    }
    info
}

fn to_sim_deck(pool: &CardPool, resolved: &mtg_sources::ResolvedDeck) -> SimDeck {
    // Same heuristic the meta opponents get; the user's deck is not exempt.
    let creatures = mtg_sim::meta_loader::creature_count(pool, &resolved.main);
    SimDeck {
        name: resolved.name.clone(),
        cards: resolved.main.clone(),
        commander: resolved.commander,
        meta_share: 1.0,
        pilot_warning: mtg_sources::meta::pilot_warning(creatures),
    }
}

fn parse_games(s: &str) -> (u32, bool) {
    if s.eq_ignore_ascii_case("auto") {
        (1_000_000, true)
    } else {
        (s.parse::<u32>().unwrap_or(1000).max(1), false)
    }
}

// Commands

#[tauri::command]
async fn pool_status(state: State<'_, AppState>) -> Result<PoolInfo, String> {
    let slot = state.pool.clone();
    tauri::async_runtime::spawn_blocking(move || get_pool(&slot).map(|(_, info)| info))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn parse_deck(
    state: State<'_, AppState>,
    text: String,
    name: String,
) -> Result<DeckInfo, String> {
    let slot = state.pool.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (pool, _) = get_pool(&slot)?;
        Ok(build_deck_info(&pool, &text, &name))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn save_deck(name: String, text: String) -> Result<(), String> {
    let clean = sanitize(&name);
    if clean.is_empty() {
        return Err("deck needs a name".into());
    }
    std::fs::write(decks_dir()?.join(format!("{clean}.txt")), text).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_decks() -> Result<Vec<DeckFile>, String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(decks_dir()?).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().map(|e| e == "txt").unwrap_or(false) {
            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            out.push(DeckFile { name, text });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[tauri::command]
async fn delete_deck(name: String) -> Result<(), String> {
    let clean = sanitize(&name);
    let path = decks_dir()?.join(format!("{clean}.txt"));
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn fetch_meta(
    app: AppHandle,
    state: State<'_, AppState>,
    format: String,
    days: i64,
    top: usize,
    selection: String,
) -> Result<MetaResponse, String> {
    let slot = state.pool.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (pool, _) = get_pool(&slot)?;
        let mut status = |s: String| {
            let _ = app.emit("meta-progress", s);
        };
        let (meta, info) = mtg_sim::meta_loader::load_meta(
            &pool,
            &format,
            days,
            to_selection(&selection, top),
            roll_seed(),
            &mut status,
        )
        .map_err(|e| e.to_string())?;
        let entries = meta
            .iter()
            .map(|m| {
                let mut playable = 0u32;
                let mut total = 0u32;
                for (cid, count) in &m.cards {
                    let tier = mtg_cards::compile(pool.get(*cid)).tier;
                    total += *count as u32;
                    if tier >= mtg_ir::CoverageTier::Partial {
                        playable += *count as u32;
                    }
                }
                MetaEntry {
                    name: m.name.clone(),
                    share: m.meta_share,
                    pilot_warning: m.pilot_warning,
                    playable: if total > 0 { playable as f64 / total as f64 } else { 0.0 },
                    cards: m
                        .cards
                        .iter()
                        .map(|(cid, count)| NameCount {
                            name: pool.get(*cid).name.to_string(),
                            count: *count,
                        })
                        .collect(),
                }
            })
            .collect();
        Ok(MetaResponse {
            entries,
            archetypes_total: info.archetypes_total,
            eligible: info.eligible,
            classified_decks: info.classified_decks,
            randomized: info.randomized,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize, Clone)]
struct MetaResponse {
    entries: Vec<MetaEntry>,
    archetypes_total: usize,
    eligible: usize,
    classified_decks: usize,
    randomized: bool,
}

fn deck_coverage_fracs(pool: &CardPool, deck: &SimDeck) -> (f64, f64) {
    let (_, _, cov) = mtg_sim::build_db(pool, &[deck]);
    (cov[0].full_frac(), cov[0].playable_frac())
}

#[tauri::command]
fn start_run(app: AppHandle, state: State<'_, AppState>, config: RunConfig) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("a run is already in progress".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *state.cancel.lock().unwrap() = Some(cancel.clone());
    let pool_slot = state.pool.clone();
    let running = state.running.clone();

    std::thread::spawn(move || {
        let outcome = run_thread(&app, pool_slot, cancel, config);
        if let Err(e) = outcome {
            let _ = app.emit("run-error", e);
        }
        running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

fn emit_prep(app: &AppHandle, status: &str) {
    let _ = app.emit(
        "run-progress",
        ProgressPayload {
            phase: "prep".into(),
            status: status.to_string(),
            matchups: Vec::new(),
            games_per_sec: 0.0,
            elapsed: 0.0,
        },
    );
}

fn run_thread(
    app: &AppHandle,
    pool_slot: Arc<Mutex<Option<Arc<CardPool>>>>,
    cancel: Arc<AtomicBool>,
    config: RunConfig,
) -> Result<(), String> {
    emit_prep(app, "loading card pool...");
    let (pool, _) = get_pool(&pool_slot)?;

    emit_prep(app, "reading deck...");
    let parsed = mtg_sources::parse_deck_text(&config.deck_text);
    let deck_name = if config.deck_name.trim().is_empty() {
        parsed.name.clone().unwrap_or_else(|| "deck".into())
    } else {
        config.deck_name.clone()
    };
    let (resolved, unresolved) =
        mtg_sources::deck_import::resolve_deck_lossy(&pool, &parsed, &deck_name);
    let resolved = resolved.ok_or("no cards resolved from the decklist")?;
    if !unresolved.is_empty() {
        emit_prep(app, &format!("{} unresolved names dropped", unresolved.len()));
    }
    let mut user = to_sim_deck(&pool, &resolved);

    // Pod convention: first card is the commander when no section names one.
    if config.mode == "pod" && user.commander.is_none() {
        if let Some((first, _)) = user.cards.first().copied() {
            user.commander = Some(first);
            if let Some(slot) = user.cards.iter_mut().find(|(id, _)| *id == first) {
                slot.1 = slot.1.saturating_sub(1);
            }
            user.cards.retain(|(_, c)| *c > 0);
        }
    }

    // The master seed is fixed before opponents load so random gauntlet
    // draws are pinned by it too.
    let master_seed = config.seed.unwrap_or_else(roll_seed);

    // Opponents.
    let opponents: Vec<SimDeck> = match config.mode.as_str() {
        "gauntlet" => {
            let mut status = |s: String| emit_prep(app, &s);
            let (decks, info) = mtg_sim::meta_loader::load_meta(
                &pool,
                &config.format,
                config.days,
                to_selection(&config.selection, config.top),
                master_seed,
                &mut status,
            )
            .map_err(|e| e.to_string())?;
            emit_prep(
                app,
                &format!(
                    "gauntlet: {} of {} eligible archetypes ({} seen in window){}",
                    info.selected,
                    info.eligible,
                    info.archetypes_total,
                    if info.randomized { ", randomly drawn" } else { "" }
                ),
            );
            decks
        }
        "pod" => {
            let mut status = |s: String| emit_prep(app, &s);
            let (decks, _) = mtg_sim::meta_loader::load_meta(
                &pool,
                "commander",
                config.days,
                to_selection(&config.selection, config.top),
                master_seed,
                &mut status,
            )
            .map_err(|e| e.to_string())?;
            decks
        }
        "goldfish" => Vec::new(),
        "duel" | "sweep" => {
            let vs = config.vs_text.clone().ok_or("pick an opponent deck")?;
            let vparsed = mtg_sources::parse_deck_text(&vs);
            let (vres, _) =
                mtg_sources::deck_import::resolve_deck_lossy(&pool, &vparsed, "opponent");
            let vres = vres.ok_or("no cards resolved from the opponent decklist")?;
            vec![to_sim_deck(&pool, &vres)]
        }
        m => return Err(format!("unknown mode: {m}")),
    };
    if opponents.is_empty() && config.mode != "goldfish" {
        return Err("no opponents resolved".into());
    }
    if config.mode == "pod" && opponents.len() < 3 {
        return Err("pods need at least 3 commander meta decks".into());
    }

    let (games, auto) = parse_games(&config.games);
    let is_commander = config.format.eq_ignore_ascii_case("commander") || config.mode == "pod";
    let rules = if config.mode == "pod" {
        mtg_engine::RulesConfig::commander_pod(4)
    } else if is_commander {
        mtg_engine::RulesConfig::commander_pod(2)
    } else {
        mtg_engine::RulesConfig::duel()
    };
    let cfg = mtg_sim::SimConfig {
        games_cap: games,
        floor: if auto { 1000.min(games) } else { 200.min(games) },
        early_stop: config.early_stop,
        precision_target: auto.then_some(config.precision / 100.0),
        cancel: Some(cancel.clone()),
        master_seed,
        rules,
    };

    let (deck_full, deck_playable) = deck_coverage_fracs(&pool, &user);

    // Progress plumbing: one MatchupProgress per opponent (one for sweeps
    // and pods), polled by a side thread into events.
    let n_progress = match config.mode.as_str() {
        "gauntlet" => opponents.len(),
        _ => 1,
    };
    let progress: Vec<Arc<MatchupProgress>> =
        (0..n_progress).map(|_| Arc::new(MatchupProgress::default())).collect();
    let names: Vec<String> = match config.mode.as_str() {
        "gauntlet" => opponents.iter().map(|o| o.name.clone()).collect(),
        "sweep" => vec![format!("all hands vs {}", opponents[0].name)],
        "duel" => vec![opponents[0].name.clone()],
        "goldfish" => vec!["goldfish (passive opponent)".to_string()],
        _ => vec![format!("4-player pods, {} meta decks", opponents.len())],
    };
    let done = Arc::new(AtomicBool::new(false));
    let poller = {
        let app = app.clone();
        let progress = progress.clone();
        let names = names.clone();
        let done = done.clone();
        let started = std::time::Instant::now();
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let matchups: Vec<MatchupProg> = progress
                    .iter()
                    .zip(&names)
                    .map(|(p, name)| MatchupProg {
                        name: name.clone(),
                        done: p.done.load(Ordering::Relaxed),
                        target: p.target.load(Ordering::Relaxed),
                        wins: p.wins.load(Ordering::Relaxed),
                        losses: p.losses.load(Ordering::Relaxed),
                        draws: p.draws.load(Ordering::Relaxed),
                        stopped: p.stopped.load(Ordering::Relaxed),
                    })
                    .collect();
                let total: u64 = matchups.iter().map(|m| m.done as u64).sum();
                let elapsed = started.elapsed().as_secs_f64();
                let _ = app.emit(
                    "run-progress",
                    ProgressPayload {
                        phase: "run".into(),
                        status: String::new(),
                        matchups,
                        games_per_sec: total as f64 / elapsed.max(0.001),
                        elapsed,
                    },
                );
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        })
    };

    let started = std::time::Instant::now();
    let mut result = RunResult {
        kind: config.mode.clone(),
        deck_name: deck_name.clone(),
        format: config.format.clone(),
        when: now_unix(),
        elapsed: 0.0,
        cancelled: false,
        deck_full,
        deck_playable,
        gauntlet: None,
        sweep: None,
        pod: None,
        goldfish: None,
        seed: master_seed,
        deck_pilot_warning: user.pilot_warning,
    };

    match config.mode.as_str() {
        "goldfish" => {
            let g = mtg_sim::goldfish::run_goldfish(&pool, &user, &cfg, &progress[0]);
            result.goldfish = Some(g);
        }
        "gauntlet" => {
            let mut stats = mtg_sim::run_gauntlet(&pool, &user, &opponents, &cfg, &progress);
            stats.deck_name = deck_name.clone();
            stats.format = config.format.clone();
            result.gauntlet = Some(stats);
        }
        "duel" => {
            let m = mtg_sim::run_matchup(&pool, &user, &opponents[0], &cfg, 0, &progress[0]);
            result.gauntlet = Some(mtg_stats::GauntletStats {
                deck_name: deck_name.clone(),
                format: "duel".into(),
                matchups: vec![m],
            });
        }
        "sweep" => {
            let n = mtg_sim::sweep::count_hands(&user.cards, 7);
            if n as usize > mtg_sim::sweep::MAX_SWEEP_HANDS {
                done.store(true, Ordering::Relaxed);
                let _ = poller.join();
                return Err(format!(
                    "{n} distinct hands is past the sweep limit; singleton decks explode \
                     combinatorially. Use gauntlet or duel with auto games instead."
                ));
            }
            let s =
                mtg_sim::sweep::run_hand_sweep(&pool, &user, &opponents[0], &cfg, config.per_hand, &progress[0]);
            let fmt_hand = |h: &mtg_sim::sweep::HandOutcome| -> HandDto {
                let cards: Vec<String> = h
                    .cards
                    .iter()
                    .map(|(id, n)| {
                        let name = pool.get(*id).name.to_string();
                        if *n > 1 {
                            format!("{n}x {name}")
                        } else {
                            name
                        }
                    })
                    .collect();
                HandDto {
                    cards: cards.join(", "),
                    probability: h.probability,
                    games: h.games,
                    win_rate: h.win_rate(),
                }
            };
            let mut ranked: Vec<&mtg_sim::sweep::HandOutcome> = s.hands.iter().collect();
            ranked.sort_by(|a, b| a.win_rate().partial_cmp(&b.win_rate()).unwrap());
            let mut histogram = vec![0.0f64; 20];
            for h in &s.hands {
                let bucket = ((h.win_rate() * 20.0) as usize).min(19);
                histogram[bucket] += h.probability;
            }
            result.sweep = Some(SweepDto {
                weighted_win_rate: s.weighted_win_rate,
                standard_error: s.standard_error,
                total_games: s.total_games,
                distinct_hands: s.distinct_hands,
                panics: s.panics,
                worst: ranked.iter().take(8).map(|h| fmt_hand(h)).collect(),
                best: ranked.iter().rev().take(8).map(|h| fmt_hand(h)).collect(),
                histogram,
            });
        }
        _ => {
            let m = mtg_sim::run_pod(&pool, &user, &opponents, &cfg, &progress[0]);
            result.pod = Some(m);
        }
    }

    result.elapsed = started.elapsed().as_secs_f64();
    result.cancelled = cancel.load(Ordering::Relaxed);
    done.store(true, Ordering::Relaxed);
    let _ = poller.join();

    // Persist history.
    if let Ok(dir) = runs_dir() {
        let file = format!("{}-{}.json", result.when, sanitize(&deck_name));
        if let Ok(json) = serde_json::to_vec_pretty(&result) {
            let _ = std::fs::write(dir.join(file), json);
        }
    }
    let _ = app.emit("run-done", result);
    Ok(())
}

#[tauri::command]
fn cancel_run(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(c) = state.cancel.lock().unwrap().as_ref() {
        c.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
async fn list_runs() -> Result<Vec<RunMeta>, String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(runs_dir()?).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().map(|e| e != "json").unwrap_or(true) {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else { continue };
        let Ok(r) = serde_json::from_str::<RunResult>(&body) else { continue };
        let (headline, games) = if let Some(g) = &r.gauntlet {
            (g.weighted_win_rate(), g.total_games() as u64)
        } else if let Some(s) = &r.sweep {
            (s.weighted_win_rate, s.total_games)
        } else if let Some(p) = &r.pod {
            (p.win_rate(), p.games as u64)
        } else if let Some(g) = &r.goldfish {
            (
                if g.games > 0 { g.kills as f64 / g.games as f64 } else { 0.0 },
                g.games as u64,
            )
        } else {
            (0.0, 0)
        };
        out.push(RunMeta {
            file: path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
            deck_name: r.deck_name,
            format: r.format,
            kind: r.kind,
            when: r.when,
            headline,
            games,
        });
    }
    out.sort_by(|a, b| b.when.cmp(&a.when));
    Ok(out)
}

#[derive(Serialize, Clone)]
struct LimitDto {
    id: String,
    category: String,
    rule_ref: String,
    summary: String,
    impact: String,
}

#[tauri::command]
fn list_limits() -> Vec<LimitDto> {
    mtg_sim::limits::all_limits()
        .into_iter()
        .map(|l| LimitDto {
            id: l.id.to_string(),
            category: l.category.label().to_string(),
            rule_ref: l.rule_ref.to_string(),
            summary: l.summary.to_string(),
            impact: l.impact.to_string(),
        })
        .collect()
}

#[tauri::command]
async fn load_run(file: String) -> Result<RunResult, String> {
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err("bad run file name".into());
    }
    let body = std::fs::read_to_string(runs_dir()?.join(file)).map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(AppState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pool_status,
            parse_deck,
            save_deck,
            list_decks,
            delete_deck,
            fetch_meta,
            start_run,
            cancel_run,
            list_runs,
            load_run,
            list_limits
        ])
        .run(tauri::generate_context!())
        .expect("tolaria desktop failed to start");
}
