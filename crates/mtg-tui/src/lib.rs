//! The Tolaria terminal UI: setup, live gauntlet progress, results.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table};

use mtg_sim::{MatchupProgress, SimDeck};
use mtg_stats::GauntletStats;

pub struct TuiArgs {
    pub deck: Option<PathBuf>,
    pub format: String,
    pub games: u32,
    pub days: i64,
    pub top: usize,
    pub seed: u64,
}

enum Phase {
    Setup,
    Loading,
    Running,
    Results,
    Failed,
}

enum WorkerMsg {
    Status(String),
    MetaReady { names: Vec<String>, user_coverage: (f64, f64) },
    Done(Box<GauntletStats>),
    Error(String),
}

struct App {
    phase: Phase,
    deck_input: String,
    format: String,
    games_input: String,
    status: String,
    matchup_names: Vec<String>,
    progress: Vec<Arc<MatchupProgress>>,
    results: Option<GauntletStats>,
    user_coverage: (f64, f64),
    error: String,
    field_idx: usize,
    started: std::time::Instant,
}

const FORMATS: [&str; 7] =
    ["modern", "standard", "pioneer", "legacy", "vintage", "pauper", "commander"];

pub fn run_tui(args: TuiArgs) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = run_app(&mut terminal, args);
    ratatui::restore();
    result
}

fn run_app(terminal: &mut ratatui::DefaultTerminal, args: TuiArgs) -> Result<()> {
    let mut app = App {
        phase: Phase::Setup,
        deck_input: args
            .deck
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        format: args.format.clone(),
        games_input: args.games.to_string(),
        status: String::new(),
        matchup_names: Vec::new(),
        progress: Vec::new(),
        results: None,
        user_coverage: (0.0, 0.0),
        error: String::new(),
        field_idx: 0,
        started: std::time::Instant::now(),
    };
    let (tx, rx): (Sender<WorkerMsg>, Receiver<WorkerMsg>) = std::sync::mpsc::channel();
    let shared_progress: Arc<Mutex<Vec<Arc<MatchupProgress>>>> = Arc::new(Mutex::new(Vec::new()));

    loop {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                WorkerMsg::Status(s) => app.status = s,
                WorkerMsg::MetaReady { names, user_coverage } => {
                    app.matchup_names = names;
                    app.user_coverage = user_coverage;
                    app.progress = shared_progress.lock().unwrap().clone();
                    app.phase = Phase::Running;
                    app.started = std::time::Instant::now();
                }
                WorkerMsg::Done(stats) => {
                    app.results = Some(*stats);
                    app.phase = Phase::Results;
                }
                WorkerMsg::Error(e) => {
                    app.error = e;
                    app.phase = Phase::Failed;
                }
            }
        }

        terminal.draw(|f| draw(f, &app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                let ctrl_c = key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL);
                match app.phase {
                    Phase::Setup => match key.code {
                        KeyCode::Esc => return Ok(()),
                        _ if ctrl_c => return Ok(()),
                        KeyCode::Tab | KeyCode::Down => app.field_idx = (app.field_idx + 1) % 3,
                        KeyCode::BackTab | KeyCode::Up => {
                            app.field_idx = (app.field_idx + 2) % 3
                        }
                        KeyCode::Left | KeyCode::Right if app.field_idx == 1 => {
                            let cur = FORMATS
                                .iter()
                                .position(|f| *f == app.format)
                                .unwrap_or(0);
                            let next = if key.code == KeyCode::Right {
                                (cur + 1) % FORMATS.len()
                            } else {
                                (cur + FORMATS.len() - 1) % FORMATS.len()
                            };
                            app.format = FORMATS[next].to_string();
                        }
                        KeyCode::Char(c) => match app.field_idx {
                            0 => app.deck_input.push(c),
                            2 if c.is_ascii_digit() => app.games_input.push(c),
                            _ => {}
                        },
                        KeyCode::Backspace => match app.field_idx {
                            0 => {
                                app.deck_input.pop();
                            }
                            2 => {
                                app.games_input.pop();
                            }
                            _ => {}
                        },
                        KeyCode::Enter => {
                            if app.deck_input.trim().is_empty() {
                                app.status = "enter a deck file path first".to_string();
                            } else {
                                app.phase = Phase::Loading;
                                app.status = "loading card pool...".to_string();
                                spawn_worker(
                                    tx.clone(),
                                    shared_progress.clone(),
                                    app.deck_input.trim().to_string(),
                                    app.format.clone(),
                                    app.games_input.parse().unwrap_or(args.games),
                                    args.days,
                                    args.top,
                                    args.seed,
                                );
                            }
                        }
                        _ => {}
                    },
                    Phase::Loading | Phase::Running => {
                        if key.code == KeyCode::Esc || ctrl_c {
                            return Ok(());
                        }
                    }
                    Phase::Results | Phase::Failed => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => return Ok(()),
                        _ if ctrl_c => return Ok(()),
                        KeyCode::Char('r') => {
                            app.phase = Phase::Setup;
                            app.results = None;
                            app.error.clear();
                        }
                        KeyCode::Char('s') => {
                            if let Some(stats) = &app.results {
                                let path = format!(
                                    "tolaria-{}-{}.json",
                                    stats.deck_name.replace(' ', "-"),
                                    stats.format
                                );
                                if std::fs::write(
                                    &path,
                                    serde_json::to_vec_pretty(stats).unwrap_or_default(),
                                )
                                .is_ok()
                                {
                                    app.status = format!("saved {path}");
                                }
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_worker(
    tx: Sender<WorkerMsg>,
    shared_progress: Arc<Mutex<Vec<Arc<MatchupProgress>>>>,
    deck: String,
    format: String,
    games: u32,
    days: i64,
    top: usize,
    seed: u64,
) {
    std::thread::spawn(move || {
        let send = |m: WorkerMsg| {
            let _ = tx.send(m);
        };
        let run = || -> anyhow::Result<GauntletStats> {
            send(WorkerMsg::Status("loading card pool...".into()));
            let paths = mtg_data::Paths::resolve()?;
            let (pool, _) = mtg_data::ensure_pool(&paths, &mtg_data::EnsureOptions::default())?;

            send(WorkerMsg::Status("reading deck...".into()));
            let user = mtg_sources::load_deck_file(&pool, std::path::Path::new(&deck))?;
            let user_sim = SimDeck {
                name: user.name.clone(),
                cards: user.main.clone(),
                commander: user.commander,
                meta_share: 1.0,
                pilot_warning: false,
            };

            send(WorkerMsg::Status(format!("syncing {format} meta...")));
            let meta = crate::meta_loader::load_meta(&pool, &format, days, top, &mut |s| {
                send(WorkerMsg::Status(s));
            })?;
            if meta.is_empty() {
                anyhow::bail!("no meta decks resolved for {format}");
            }

            let (_, _, cov) = mtg_sim::build_db(&pool, &[&user_sim]);
            let progress: Vec<Arc<MatchupProgress>> =
                (0..meta.len()).map(|_| Default::default()).collect();
            *shared_progress.lock().unwrap() = progress.clone();
            send(WorkerMsg::MetaReady {
                names: meta.iter().map(|m| m.name.clone()).collect(),
                user_coverage: (cov[0].full_frac(), cov[0].playable_frac()),
            });

            let is_commander = format.eq_ignore_ascii_case("commander");
            let cfg = mtg_sim::SimConfig {
                games_cap: games,
                floor: 200.min(games),
                early_stop: true,
                master_seed: seed,
                rules: if is_commander {
                    mtg_engine::RulesConfig::commander_pod(2)
                } else {
                    mtg_engine::RulesConfig::duel()
                },
            };
            let mut stats = mtg_sim::run_gauntlet(&pool, &user_sim, &meta, &cfg, &progress);
            stats.format = format.clone();
            Ok(stats)
        };
        match run() {
            Ok(stats) => send(WorkerMsg::Done(Box::new(stats))),
            Err(e) => send(WorkerMsg::Error(e.to_string())),
        }
    });
}

fn draw(f: &mut ratatui::Frame, app: &App) {
    let area = f.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(4), Constraint::Length(1)])
        .split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled("TOLARIA", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  a time bubble for your decklist"),
    ]))
    .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(title, outer[0]);

    match app.phase {
        Phase::Setup => draw_setup(f, outer[1], app),
        Phase::Loading => {
            let p = Paragraph::new(app.status.clone())
                .block(Block::default().borders(Borders::ALL).title(" loading "));
            f.render_widget(p, outer[1]);
        }
        Phase::Running => draw_running(f, outer[1], app),
        Phase::Results => draw_results(f, outer[1], app),
        Phase::Failed => {
            let p = Paragraph::new(app.error.clone())
                .style(Style::default().fg(Color::Red))
                .block(Block::default().borders(Borders::ALL).title(" error "));
            f.render_widget(p, outer[1]);
        }
    }

    let help = match app.phase {
        Phase::Setup => "tab: next field   left/right: format   enter: run   esc: quit",
        Phase::Loading | Phase::Running => "esc: quit",
        Phase::Results => "s: save json   r: run again   q: quit",
        Phase::Failed => "r: back   q: quit",
    };
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(Color::DarkGray)),
        outer[2],
    );
}

fn field_style(selected: bool) -> Style {
    if selected {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn draw_setup(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
    let deck = Paragraph::new(app.deck_input.clone())
        .style(field_style(app.field_idx == 0))
        .block(Block::default().borders(Borders::ALL).title(" deck file "));
    f.render_widget(deck, rows[0]);
    let fmt = Paragraph::new(app.format.clone())
        .style(field_style(app.field_idx == 1))
        .block(Block::default().borders(Borders::ALL).title(" format "));
    f.render_widget(fmt, rows[1]);
    let games = Paragraph::new(app.games_input.clone())
        .style(field_style(app.field_idx == 2))
        .block(Block::default().borders(Borders::ALL).title(" games per matchup "));
    f.render_widget(games, rows[2]);
    if !app.status.is_empty() {
        f.render_widget(
            Paragraph::new(app.status.clone()).style(Style::default().fg(Color::Yellow)),
            rows[3],
        );
    }
}

fn draw_running(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let n = app.matchup_names.len().max(1);
    let mut constraints = vec![Constraint::Length(2); n];
    constraints.push(Constraint::Min(0));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut total_done = 0u64;
    for (i, name) in app.matchup_names.iter().enumerate() {
        let (done, target, wins, losses, draws, stopped) = app
            .progress
            .get(i)
            .map(|p| {
                (
                    p.done.load(Ordering::Relaxed),
                    p.target.load(Ordering::Relaxed).max(1),
                    p.wins.load(Ordering::Relaxed),
                    p.losses.load(Ordering::Relaxed),
                    p.draws.load(Ordering::Relaxed),
                    p.stopped.load(Ordering::Relaxed),
                )
            })
            .unwrap_or((0, 1, 0, 0, 0, false));
        total_done += done as u64;
        let games = wins + losses + draws;
        let wr = if games > 0 {
            (wins as f64 + draws as f64 * 0.5) / games as f64 * 100.0
        } else {
            50.0
        };
        let label = format!(
            "{name:<34} {done:>5} games  {wr:>5.1}%{}",
            if stopped { "  [decided]" } else { "" }
        );
        let ratio = if stopped { 1.0 } else { (done as f64 / target as f64).min(1.0) };
        let gauge = Gauge::default()
            .ratio(ratio)
            .label(label)
            .gauge_style(Style::default().fg(if stopped { Color::Green } else { Color::Cyan }));
        f.render_widget(gauge, rows[i]);
    }
    let rate = total_done as f64 / app.started.elapsed().as_secs_f64().max(0.001);
    f.render_widget(
        Paragraph::new(format!("{rate:.0} games/s")).style(Style::default().fg(Color::DarkGray)),
        rows[n],
    );
}

fn draw_results(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let Some(stats) = &app.results else { return };
    let rows_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(4)])
        .split(area);

    let (cov_full, cov_play) = app.user_coverage;
    let banner = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" field win rate {:.1}% ", stats.weighted_win_rate() * 100.0),
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  {} games   deck coverage {:.0}% full / {:.0}% playable",
            stats.total_games(),
            cov_full * 100.0,
            cov_play * 100.0
        )),
    ]));
    f.render_widget(banner, rows_layout[0]);

    let header = Row::new(vec![
        "matchup", "share", "games", "win%", "95% ci", "play", "draw", "opp cov", "",
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = stats
        .sorted_worst_first()
        .into_iter()
        .map(|m| {
            let (lo, hi) = m.ci95();
            let wr = m.win_rate();
            let color = if wr < 0.45 {
                Color::Red
            } else if wr > 0.55 {
                Color::Green
            } else {
                Color::Yellow
            };
            Row::new(vec![
                Cell::from(m.opponent.clone()),
                Cell::from(format!("{:.1}%", m.meta_share * 100.0)),
                Cell::from(m.games.to_string()),
                Cell::from(format!("{:.1}%", wr * 100.0)).style(Style::default().fg(color)),
                Cell::from(format!("{:.0}..{:.0}", lo * 100.0, hi * 100.0)),
                Cell::from(format!("{:.0}%", m.on_play_rate() * 100.0)),
                Cell::from(format!("{:.0}%", m.on_draw_rate() * 100.0)),
                Cell::from(format!("{:.0}%", m.opp_coverage_playable_frac * 100.0)),
                Cell::from(if m.opp_pilot_warning { "!" } else { "" }),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Min(24),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(2),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(" matchups, worst first "));
    f.render_widget(table, rows_layout[1]);
}

pub mod meta_loader;
