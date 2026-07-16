//! Local cache orchestration: platform paths, freshness policy, and the
//! load-or-fetch entry point.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::pool::CardPool;
use crate::scryfall;
use crate::DataError;

#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
}

impl Paths {
    pub fn resolve() -> Result<Paths, DataError> {
        let dirs = directories::ProjectDirs::from("gg", "modusimagery", "Tolaria")
            .ok_or(DataError::NoDataDir)?;
        Ok(Paths {
            data_dir: dirs.data_dir().to_path_buf(),
            config_dir: dirs.config_dir().to_path_buf(),
        })
    }

    pub fn cards_bin(&self) -> PathBuf {
        self.data_dir.join("cards").join("oracle_cards.bin")
    }

    pub fn cards_meta(&self) -> PathBuf {
        self.data_dir.join("cards").join("manifest.json")
    }

    pub fn cards_raw(&self) -> PathBuf {
        self.data_dir.join("cards").join("oracle_cards.jsonl.gz")
    }

    pub fn meta_dir(&self) -> PathBuf {
        self.data_dir.join("meta")
    }

    pub fn commander_dir(&self) -> PathBuf {
        self.data_dir.join("commander")
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheMeta {
    updated_at: String,
    fetched_unix: u64,
    card_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolSource {
    FreshCache,
    StaleCache,
    Downloaded,
}

#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub source: PoolSource,
    pub card_count: usize,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct EnsureOptions {
    pub user_agent: Option<String>,
    /// Skip the freshness window and re-check the manifest.
    pub force_refresh: bool,
    /// Never touch the network.
    pub offline: bool,
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

const FRESH_SECS: u64 = 24 * 60 * 60;

fn load_bin(paths: &Paths) -> Result<(CardPool, CacheMeta), DataError> {
    let bytes = std::fs::read(paths.cards_bin())?;
    let cards: Vec<crate::model::OracleCard> = postcard::from_bytes(&bytes)?;
    let meta: CacheMeta = serde_json::from_slice(&std::fs::read(paths.cards_meta())?)?;
    Ok((CardPool::from_cards(cards), meta))
}

fn save_bin(paths: &Paths, cards: &[crate::model::OracleCard], updated_at: &str) -> Result<(), DataError> {
    std::fs::create_dir_all(paths.cards_bin().parent().unwrap())?;
    let bytes = postcard::to_allocvec(cards)?;
    std::fs::write(paths.cards_bin(), bytes)?;
    let meta = CacheMeta {
        updated_at: updated_at.to_string(),
        fetched_unix: now_unix(),
        card_count: cards.len() as u32,
    };
    std::fs::write(paths.cards_meta(), serde_json::to_vec_pretty(&meta)?)?;
    Ok(())
}

/// Load the card pool, downloading or refreshing the Scryfall bulk data as
/// needed. Falls back to a stale cache when the network is unavailable.
pub fn ensure_pool(paths: &Paths, opts: &EnsureOptions) -> Result<(CardPool, PoolStatus), DataError> {
    let ua = opts.user_agent.clone().unwrap_or_else(crate::default_user_agent);
    let cached = load_bin(paths).ok();

    if let Some((pool, meta)) = &cached {
        let age_ok = now_unix().saturating_sub(meta.fetched_unix) < FRESH_SECS;
        if opts.offline || (age_ok && !opts.force_refresh) {
            let status = PoolStatus {
                source: if age_ok { PoolSource::FreshCache } else { PoolSource::StaleCache },
                card_count: pool.len(),
                updated_at: meta.updated_at.clone(),
            };
            // A cheap move: cached is consumed below only in this branch.
            let (pool, _) = cached.unwrap();
            return Ok((pool, status));
        }
    } else if opts.offline {
        return Err(DataError::Offline("no cached card pool".into()));
    }

    let agent = scryfall::agent(&ua);
    let manifest = match scryfall::fetch_manifest(&agent) {
        Ok(m) => m,
        Err(e) => {
            // Network trouble: a stale pool beats no pool.
            if let Some((pool, meta)) = cached {
                let status = PoolStatus {
                    source: PoolSource::StaleCache,
                    card_count: pool.len(),
                    updated_at: meta.updated_at,
                };
                return Ok((pool, status));
            }
            return Err(e);
        }
    };

    if let Some((pool, meta)) = &cached {
        if meta.updated_at == manifest.updated_at {
            // Manifest unchanged: refresh the freshness stamp and reuse.
            let meta2 = CacheMeta {
                updated_at: meta.updated_at.clone(),
                fetched_unix: now_unix(),
                card_count: pool.len() as u32,
            };
            let _ = std::fs::write(paths.cards_meta(), serde_json::to_vec_pretty(&meta2)?);
            let status = PoolStatus {
                source: PoolSource::FreshCache,
                card_count: pool.len(),
                updated_at: meta.updated_at.clone(),
            };
            let (pool, _) = cached.unwrap();
            return Ok((pool, status));
        }
    }

    let url = manifest.jsonl_uri.as_deref().unwrap_or(manifest.json_uri.as_str());
    scryfall::download_to(&agent, url, &paths.cards_raw())?;
    let cards = scryfall::parse_bulk_file(&paths.cards_raw())?;
    save_bin(paths, &cards, &manifest.updated_at)?;
    let count = cards.len();
    Ok((
        CardPool::from_cards(cards),
        PoolStatus {
            source: PoolSource::Downloaded,
            card_count: count,
            updated_at: manifest.updated_at,
        },
    ))
}
