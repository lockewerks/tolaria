//! Shared meta-gauntlet loading used by both the CLI and the TUI.

use anyhow::Result;
use mtg_data::CardPool;

use crate::SimDeck;

pub fn creature_count(pool: &CardPool, cards: &[(mtg_data::CardId, u8)]) -> u32 {
    cards
        .iter()
        .filter(|(id, _)| pool.get(*id).front().types.contains(mtg_ir::CardTypes::CREATURE))
        .map(|(_, c)| *c as u32)
        .sum()
}

/// Sync sources and compute the meta gauntlet for a format. Status strings
/// stream through the callback.
pub fn load_meta(
    pool: &CardPool,
    format_str: &str,
    days: i64,
    top: usize,
    status: &mut dyn FnMut(String),
) -> Result<Vec<SimDeck>> {
    let format = mtg_data::Format::parse(format_str)
        .ok_or_else(|| anyhow::anyhow!("unknown format: {format_str}"))?;
    let paths = mtg_data::Paths::resolve()?;
    let agent = mtg_sources::http::agent(&mtg_data::default_user_agent());

    if format == mtg_data::Format::Commander {
        status("fetching top commanders from EDHREC...".into());
        let commanders = mtg_sources::edhrec::top_commanders(&agent, "year", top)?;
        let total: u64 = commanders.iter().map(|c| c.num_decks.max(1)).sum();
        let mut out = Vec::new();
        for c in commanders {
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
        return Ok(out);
    }

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

    let rules = mtg_sources::archetypes::load_rules(&rules_dir, format)?;
    let decks = mtg_sources::tournaments::load_decks(&cache_dir, &format.to_string(), days)?;
    status(format!("{} tournament decks in window; computing meta...", decks.len()));
    let meta = mtg_sources::meta::build_meta(&rules, &decks, top);

    let mut out = Vec::new();
    for m in meta {
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
    Ok(out)
}
