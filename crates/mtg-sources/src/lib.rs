//! Internet data sources: tournament decklist caches, archetype
//! classification, EDHREC commander data, deck text parsing.

pub mod deck_import;

pub use deck_import::{load_deck_file, parse_deck_text, resolve_deck, ParsedDeck, ResolvedDeck};

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("http: {0}")]
    Http(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("deck: {0}")]
    Deck(#[from] deck_import::DeckError),
    #[error("{0}")]
    Other(String),
}
