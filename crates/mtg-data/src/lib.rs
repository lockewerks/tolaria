//! Scryfall oracle card model, bulk data loader, and local binary cache.

pub mod cache;
pub mod model;
pub mod pool;
pub mod scryfall;

pub use cache::{ensure_pool, EnsureOptions, Paths, PoolSource, PoolStatus};
pub use model::*;
pub use pool::CardPool;

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("http: {0}")]
    Http(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cache encoding: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("no platform data directory available")]
    NoDataDir,
    #[error("scryfall bulk manifest has no oracle_cards entry")]
    MissingBulk,
    #[error("card pool not cached and network unavailable: {0}")]
    Offline(String),
}

/// User-Agent sent to Scryfall.
///
/// Scryfall asks callers to identify themselves and offer a way to be reached.
/// The repository URL does that on its own, so it is the default. Set
/// `TOLARIA_CONTACT` to add an email or other address for a deployment that
/// wants one.
pub fn default_user_agent() -> String {
    const REPO: &str = "https://github.com/lockewerks/tolaria";
    let version = env!("CARGO_PKG_VERSION");
    match std::env::var("TOLARIA_CONTACT") {
        Ok(contact) if !contact.trim().is_empty() => {
            format!("Tolaria/{} (+{}; {})", version, REPO, contact.trim())
        }
        _ => format!("Tolaria/{} (+{})", version, REPO),
    }
}
