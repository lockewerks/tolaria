//! Deck text parsing: MTGA exports, MTGO text, and plain lists.

use mtg_data::{CardId, CardPool};

#[derive(Debug, Clone, Default)]
pub struct ParsedDeck {
    pub name: Option<String>,
    pub main: Vec<(String, u8)>,
    pub side: Vec<(String, u8)>,
    pub commanders: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedDeck {
    pub name: String,
    pub main: Vec<(CardId, u8)>,
    pub side: Vec<(CardId, u8)>,
    pub commander: Option<CardId>,
}

#[derive(Debug, thiserror::Error)]
pub enum DeckError {
    #[error("deck has no cards")]
    Empty,
    #[error("unresolved card names:\n{0}")]
    Unresolved(String),
}

#[derive(PartialEq)]
enum Section {
    Main,
    Side,
    Commander,
    Ignore,
}

/// Parse one line into (count, name). Handles "4 Name", "4x Name",
/// "Name", trailing "(SET) 123" and foil markers.
fn parse_line(line: &str) -> Option<(String, u8)> {
    let line = line.trim();
    let mut count = 1u8;
    let mut rest = line;
    let first = line.split_whitespace().next()?;
    let numeric = first.trim_end_matches(['x', 'X']);
    if let Ok(n) = numeric.parse::<u16>() {
        count = n.min(250) as u8;
        rest = line[first.len()..].trim_start();
    }
    // Strip "(SET) 123" style suffixes and foil markers.
    let mut name = rest;
    if let Some(idx) = name.find(" (") {
        name = &name[..idx];
    }
    if let Some(idx) = name.find(" *") {
        name = &name[..idx];
    }
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), count))
}

pub fn parse_deck_text(text: &str) -> ParsedDeck {
    let mut deck = ParsedDeck::default();
    let mut section = Section::Main;
    let mut seen_cards = false;
    let mut blank_switched = false;

    for raw in text.lines() {
        let line = raw.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            // The MTGO convention: the first blank line after cards starts
            // the sideboard, when no explicit headers exist.
            if seen_cards && !blank_switched && section == Section::Main {
                section = Section::Side;
                blank_switched = true;
            }
            continue;
        }
        if line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        match lower.trim_end_matches(':') {
            "deck" | "mainboard" | "main" => {
                section = Section::Main;
                continue;
            }
            "sideboard" | "side" => {
                section = Section::Side;
                continue;
            }
            "commander" | "commanders" => {
                section = Section::Commander;
                continue;
            }
            "companion" | "about" | "maybeboard" => {
                section = Section::Ignore;
                continue;
            }
            _ => {}
        }
        if let Some(rest) = line.strip_prefix("Name ") {
            deck.name = Some(rest.trim().to_string());
            continue;
        }
        if let Some((name, count)) = parse_line(line) {
            seen_cards = true;
            match section {
                Section::Main => deck.main.push((name, count)),
                Section::Side => deck.side.push((name, count)),
                Section::Commander => deck.commanders.push(name),
                Section::Ignore => {}
            }
        }
    }
    deck
}

fn resolve_names(
    pool: &CardPool,
    entries: &[(String, u8)],
    problems: &mut Vec<String>,
) -> Vec<(CardId, u8)> {
    let mut out: Vec<(CardId, u8)> = Vec::new();
    for (name, count) in entries {
        match pool.lookup(name) {
            Some(id) => {
                if let Some(slot) = out.iter_mut().find(|(i, _)| *i == id) {
                    slot.1 = slot.1.saturating_add(*count);
                } else {
                    out.push((id, *count));
                }
            }
            None => {
                let sugg = pool.suggest(name, 3).join(", ");
                problems.push(if sugg.is_empty() {
                    format!("  {name}")
                } else {
                    format!("  {name} (did you mean: {sugg})")
                });
            }
        }
    }
    out
}

pub fn resolve_deck(
    pool: &CardPool,
    parsed: &ParsedDeck,
    fallback_name: &str,
) -> Result<ResolvedDeck, DeckError> {
    let mut problems = Vec::new();
    let mut main = resolve_names(pool, &parsed.main, &mut problems);
    let side = resolve_names(pool, &parsed.side, &mut problems);
    let mut commander = None;
    if let Some(cname) = parsed.commanders.first() {
        match pool.lookup(cname) {
            Some(id) => commander = Some(id),
            None => problems.push(format!("  {cname} (commander)")),
        }
    }
    // A single-section commander list may carry the commander first.
    if commander.is_some() {
        // Commander cards accidentally left in the main list are removed.
        if let Some(c) = commander {
            main.retain(|(id, _)| *id != c);
        }
    }
    if !problems.is_empty() {
        return Err(DeckError::Unresolved(problems.join("\n")));
    }
    if main.is_empty() {
        return Err(DeckError::Empty);
    }
    Ok(ResolvedDeck { name: fallback_name.to_string(), main, side, commander })
}

pub fn load_deck_file(pool: &CardPool, path: &std::path::Path) -> Result<ResolvedDeck, crate::SourceError> {
    let text = std::fs::read_to_string(path)?;
    let parsed = parse_deck_text(&text);
    let fallback = parsed
        .name
        .clone()
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "deck".to_string())
        });
    resolve_deck(pool, &parsed, &fallback).map_err(crate::SourceError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mtga_export() {
        let text = "Deck\n4 Lightning Bolt (M11) 133\n20 Mountain\n\nSideboard\n2 Smash to Smithereens";
        let d = parse_deck_text(text);
        assert_eq!(d.main.len(), 2);
        assert_eq!(d.main[0], ("Lightning Bolt".to_string(), 4));
        assert_eq!(d.side.len(), 1);
    }

    #[test]
    fn blank_line_starts_sideboard() {
        let text = "4 Lightning Bolt\n20 Mountain\n\n2 Shattering Spree";
        let d = parse_deck_text(text);
        assert_eq!(d.main.len(), 2);
        assert_eq!(d.side.len(), 1);
    }

    #[test]
    fn x_counts_and_comments() {
        let text = "# my deck\n4x Lightning Bolt\n// land\n20 Mountain";
        let d = parse_deck_text(text);
        assert_eq!(d.main.len(), 2);
    }
}
