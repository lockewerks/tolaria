//! Shared meta-gauntlet loading used by the CLI and the desktop app.

use anyhow::Result;
use mtg_data::CardPool;

use crate::SimDeck;

/// How to pick the gauntlet from the archetype universe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaSelection {
    /// The N most-played archetypes.
    Top(usize),
    /// N archetypes drawn uniformly at random from the eligible universe.
    Random(usize),
    /// Every eligible archetype.
    All,
}

impl MetaSelection {
    /// CLI parsing: "all" or a number, with a randomize flag.
    pub fn parse(spec: &str, random: bool) -> Result<MetaSelection> {
        if spec.eq_ignore_ascii_case("all") {
            return Ok(MetaSelection::All);
        }
        let n: usize = spec
            .parse()
            .map_err(|_| anyhow::anyhow!("archetype count must be a number or 'all'"))?;
        Ok(if random { MetaSelection::Random(n) } else { MetaSelection::Top(n) })
    }
}

/// What the universe looked like and what was taken from it.
#[derive(Debug, Clone, Copy, Default)]
pub struct MetaInfo {
    pub archetypes_total: usize,
    pub eligible: usize,
    pub classified_decks: usize,
    pub selected: usize,
    pub randomized: bool,
}

pub fn creature_count(pool: &CardPool, cards: &[(mtg_data::CardId, u8)]) -> u32 {
    cards
        .iter()
        .filter(|(id, _)| pool.get(*id).front().types.contains(mtg_ir::CardTypes::CREATURE))
        .map(|(_, c)| *c as u32)
        .sum()
}

/// Archetypes with fewer lists than this cannot produce a trustworthy
/// consensus list and are excluded from the universe.
pub const MIN_LISTS: usize = 3;

fn select<T>(mut items: Vec<T>, selection: MetaSelection, seed: u64) -> (Vec<T>, bool) {
    match selection {
        MetaSelection::All => (items, false),
        MetaSelection::Top(n) => {
            items.truncate(n);
            (items, false)
        }
        MetaSelection::Random(n) => {
            // Seeded Fisher-Yates over a splitmix64 stream so random
            // gauntlet composition reproduces with the master seed.
            let mut s = crate::splitmix64(seed ^ 0x4d45_5441); // "META"
            for i in (1..items.len()).rev() {
                s = crate::splitmix64(s);
                let j = (s % (i as u64 + 1)) as usize;
                items.swap(i, j);
            }
            items.truncate(n);
            (items, true)
        }
    }
}

/// Sync the tournament decklist cache and archetype rules if stale, then
/// return (tournament cache dir, rules dir). Shared by the gauntlet loader
/// and the calibration harness.
pub fn ensure_meta_sources(
    days: i64,
    status: &mut dyn FnMut(String),
) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let paths = mtg_data::Paths::resolve()?;
    let agent = mtg_sources::http::agent(&mtg_data::default_user_agent());
    let meta_dir = paths.meta_dir();
    let cache_dir = meta_dir.join("fbettega");
    let rules_dir = meta_dir.join("formatdata");
    std::fs::create_dir_all(&cache_dir)?;

    if !rules_dir.join("Formats").exists() {
        status("fetching archetype rules (MTGOFormatData)...".into());
        mtg_sources::archetypes::fetch_format_rules(&agent, &rules_dir)?;
    }

    let stamp = meta_dir.join("last-sync");
    let stale = std::fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().map(|e| e.as_secs() > 6 * 3600).unwrap_or(true))
        .unwrap_or(true);
    if stale {
        status("syncing tournament decklists...".into());
        let mut status_inner = |done: usize, total: usize| {
            status(format!("syncing tournament decklists... {done}/{total}"));
        };
        mtg_sources::tournaments::sync_cache(&agent, &cache_dir, days, &mut status_inner)?;
        std::fs::write(&stamp, b"ok")?;
    }
    Ok((cache_dir, rules_dir))
}

/// Sync sources and compute the meta gauntlet for a format. Status strings
/// stream through the callback. The seed pins random archetype draws so a
/// recorded master seed reproduces the whole gauntlet, not just the games.
pub fn load_meta(
    pool: &CardPool,
    format_str: &str,
    days: i64,
    selection: MetaSelection,
    seed: u64,
    status: &mut dyn FnMut(String),
) -> Result<(Vec<SimDeck>, MetaInfo)> {
    let format = mtg_data::Format::parse(format_str)
        .ok_or_else(|| anyhow::anyhow!("unknown format: {format_str}"))?;
    let agent = mtg_sources::http::agent(&mtg_data::default_user_agent());

    if format == mtg_data::Format::Commander {
        // EDHREC exposes roughly its top hundred commanders; that page is
        // the commander universe here. Each selected deck costs a fetch, so
        // "all" is capped to keep the request count polite.
        const COMMANDER_POOL: usize = 100;
        const COMMANDER_ALL_CAP: usize = 30;
        status("fetching top commanders from EDHREC...".into());
        let pool_size = match selection {
            MetaSelection::Top(n) => n,
            MetaSelection::Random(_) => COMMANDER_POOL,
            MetaSelection::All => COMMANDER_ALL_CAP,
        };
        let commanders = mtg_sources::edhrec::top_commanders(&agent, "year", pool_size)?;
        let universe = commanders.len();
        let take = match selection {
            MetaSelection::Top(n) | MetaSelection::Random(n) => n,
            MetaSelection::All => COMMANDER_ALL_CAP,
        };
        let (picked, randomized) = select(
            commanders,
            match selection {
                MetaSelection::Random(n) => MetaSelection::Random(n),
                _ => MetaSelection::Top(take),
            },
            seed,
        );
        let total: u64 = picked.iter().map(|c| c.num_decks.max(1)).sum();
        let mut out = Vec::new();
        for c in picked {
            status(format!("fetching average deck: {}", c.name));
            let Ok(list) = mtg_sources::edhrec::average_deck(&agent, &c.slug) else { continue };
            let parsed = mtg_sources::ParsedDeck {
                name: Some(c.name.clone()),
                main: list,
                side: Vec::new(),
                commanders: vec![c.name.clone()],
            };
            let (resolved, _) =
                mtg_sources::deck_import::resolve_deck_lossy(pool, &parsed, &c.name);
            if let Some(resolved) = resolved {
                let creatures = creature_count(pool, &resolved.main);
                out.push(SimDeck {
                    name: c.name,
                    cards: resolved.main,
                    commander: resolved.commander,
                    meta_share: c.num_decks as f64 / total as f64,
                    pilot_warning: mtg_sources::meta::pilot_warning(creatures),
                });
            }
        }
        let info = MetaInfo {
            archetypes_total: universe,
            eligible: universe,
            classified_decks: 0,
            selected: out.len(),
            randomized,
        };
        return Ok((out, info));
    }

    let (cache_dir, rules_dir) = ensure_meta_sources(days, status)?;

    let rules = mtg_sources::archetypes::load_rules(&rules_dir, format)?;
    let decks = mtg_sources::tournaments::load_decks(&cache_dir, &format.to_string(), days)?;
    status(format!("{} tournament decks in window; computing meta...", decks.len()));
    let computation = mtg_sources::meta::compute_meta(&rules, &decks, MIN_LISTS);
    status(format!(
        "archetype universe: {} seen, {} eligible ({}+ lists), {} classified decks",
        computation.archetypes_total,
        computation.eligible,
        MIN_LISTS,
        computation.classified_decks
    ));

    let (mut picked, randomized) = select(computation.decks, selection, seed);
    // Random picks come back in shuffle order; present by share regardless.
    picked.sort_by(|a, b| b.share.partial_cmp(&a.share).unwrap_or(std::cmp::Ordering::Equal));

    let mut out = Vec::new();
    for m in picked {
        let parsed = mtg_sources::ParsedDeck {
            name: Some(m.archetype.clone()),
            main: m.main.clone(),
            side: Vec::new(),
            commanders: Vec::new(),
        };
        let (resolved, _) =
            mtg_sources::deck_import::resolve_deck_lossy(pool, &parsed, &m.archetype);
        if let Some(resolved) = resolved {
            let creatures = creature_count(pool, &resolved.main);
            out.push(SimDeck {
                name: format!("{} ({} lists)", m.archetype, m.sample_size),
                cards: resolved.main,
                commander: None,
                meta_share: m.share,
                pilot_warning: mtg_sources::meta::pilot_warning(creatures),
            });
        }
    }
    let info = MetaInfo {
        archetypes_total: computation.archetypes_total,
        eligible: computation.eligible,
        classified_decks: computation.classified_decks,
        selected: out.len(),
        randomized,
    };
    Ok((out, info))
}
