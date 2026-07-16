//! EDHREC commander data: top commanders and average decklists.

use serde::Deserialize;

use crate::http::get_json;
use crate::SourceError;

#[derive(Debug, Clone)]
pub struct TopCommander {
    pub name: String,
    pub slug: String,
    pub num_decks: u64,
}

#[derive(Deserialize)]
struct CommandersPage {
    container: Container,
}

#[derive(Deserialize)]
struct Container {
    json_dict: JsonDict,
}

#[derive(Deserialize)]
struct JsonDict {
    cardlists: Vec<CardList>,
}

#[derive(Deserialize)]
struct CardList {
    #[serde(default)]
    cardviews: Vec<CardView>,
}

#[derive(Deserialize)]
struct CardView {
    name: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    num_decks: u64,
}

pub fn top_commanders(
    agent: &ureq::Agent,
    period: &str,
    limit: usize,
) -> Result<Vec<TopCommander>, SourceError> {
    let url = format!("https://json.edhrec.com/pages/commanders/{period}.json");
    let page: CommandersPage = get_json(agent, &url)?;
    let mut out = Vec::new();
    for list in page.container.json_dict.cardlists {
        for cv in list.cardviews {
            if cv.slug.is_empty() {
                continue;
            }
            out.push(TopCommander { name: cv.name, slug: cv.slug, num_decks: cv.num_decks });
            if out.len() >= limit {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

#[derive(Deserialize)]
struct AverageDeckPage {
    #[serde(default)]
    deck: Vec<String>,
}

/// The average decklist: ready-to-parse "1 Card Name" strings.
pub fn average_deck(agent: &ureq::Agent, slug: &str) -> Result<Vec<(String, u8)>, SourceError> {
    let url = format!("https://json.edhrec.com/pages/average-decks/{slug}.json");
    let page: AverageDeckPage = get_json(agent, &url)?;
    let mut out = Vec::new();
    for line in page.deck {
        let mut it = line.splitn(2, ' ');
        let count: u8 = it.next().and_then(|c| c.parse().ok()).unwrap_or(1);
        if let Some(name) = it.next() {
            out.push((name.trim().to_string(), count));
        }
    }
    Ok(out)
}
