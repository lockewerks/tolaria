//! Tournament decklist cache: incremental sync from the fbettega GitHub
//! repository via the git Trees API plus the raw CDN, and CacheItem
//! deserialization.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::http::{days_from_civil, get_json, get_string, today_days};
use crate::SourceError;

pub const CACHE_REPO: &str = "fbettega/MTG_decklistcache";
pub const CACHE_BRANCH: &str = "main";

#[derive(Deserialize)]
struct TreeResponse {
    tree: Vec<TreeEntry>,
    #[serde(default)]
    truncated: bool,
}

#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

/// Days-since-epoch parsed from "Tournaments/<src>/YYYY/MM/DD/...".
fn path_date_days(path: &str) -> Option<i64> {
    let mut it = path.split('/');
    let root = it.next()?;
    if root != "Tournaments" {
        return None;
    }
    let _source = it.next()?;
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    Some(days_from_civil(y, m, d))
}

/// Sync tournament files within the trailing window into `dest`. Returns
/// (downloaded, total_in_window). Files already on disk are kept.
pub fn sync_cache(
    agent: &ureq::Agent,
    dest: &Path,
    window_days: i64,
    mut progress: impl FnMut(usize, usize),
) -> Result<(usize, usize), SourceError> {
    let url = format!(
        "https://api.github.com/repos/{CACHE_REPO}/git/trees/{CACHE_BRANCH}?recursive=1"
    );
    let tree: TreeResponse = get_json(agent, &url)?;
    if tree.truncated {
        return Err(SourceError::Other(
            "github tree listing truncated; cannot enumerate cache".into(),
        ));
    }
    let cutoff = today_days() - window_days;
    let wanted: Vec<&TreeEntry> = tree
        .tree
        .iter()
        .filter(|e| e.kind == "blob" && e.path.ends_with(".json"))
        .filter(|e| path_date_days(&e.path).map(|d| d >= cutoff).unwrap_or(false))
        .collect();

    let total = wanted.len();
    let mut downloaded = 0usize;
    for (i, e) in wanted.iter().enumerate() {
        let local = dest.join(e.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if local.exists() {
            continue;
        }
        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = format!(
            "https://raw.githubusercontent.com/{CACHE_REPO}/{CACHE_BRANCH}/{}",
            e.path
        );
        match get_string(agent, &raw) {
            Ok(body) => {
                std::fs::write(&local, body)?;
                downloaded += 1;
            }
            Err(_) => continue,
        }
        if downloaded % 25 == 0 {
            progress(i + 1, total);
        }
    }
    progress(total, total);
    Ok((downloaded, total))
}

// CacheItem schema (PascalCase across both cache repos).

#[derive(Debug, Deserialize)]
pub struct CacheItem {
    #[serde(rename = "Tournament")]
    pub tournament: TournamentInfo,
    #[serde(rename = "Decks", default)]
    pub decks: Vec<CacheDeck>,
    #[serde(rename = "Rounds", default)]
    pub rounds: Vec<CacheRound>,
}

#[derive(Debug, Deserialize)]
pub struct CacheRound {
    #[serde(rename = "RoundName", default)]
    pub name: String,
    #[serde(rename = "Matches", default)]
    pub matches: Vec<CacheMatch>,
}

/// One reported match. `result` is a game-level "W-L-D" triple from
/// player 1's perspective; byes appear as an empty or "-" player 2.
#[derive(Debug, Deserialize)]
pub struct CacheMatch {
    #[serde(rename = "Player1", default)]
    pub p1: String,
    #[serde(rename = "Player2", default)]
    pub p2: String,
    #[serde(rename = "Result", default)]
    pub result: String,
}

#[derive(Debug, Deserialize)]
pub struct TournamentInfo {
    #[serde(rename = "Date", default)]
    pub date: String,
    #[serde(rename = "Name", default)]
    pub name: String,
    #[serde(rename = "Formats", default)]
    pub formats: FormatsField,
    #[serde(rename = "Uri", default)]
    pub uri: String,
}

/// Some writers emit a string, some an array.
#[derive(Debug, Default)]
pub struct FormatsField(pub Vec<String>);

impl<'de> Deserialize<'de> for FormatsField {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            One(String),
            Many(Vec<String>),
            None,
        }
        Ok(match Raw::deserialize(d).unwrap_or(Raw::None) {
            Raw::One(s) => FormatsField(vec![s]),
            Raw::Many(v) => FormatsField(v),
            Raw::None => FormatsField(Vec::new()),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct CacheDeck {
    #[serde(rename = "Player", default)]
    pub player: String,
    #[serde(rename = "Result", default)]
    pub result: String,
    #[serde(rename = "Mainboard", default)]
    pub mainboard: Vec<CacheCard>,
    #[serde(rename = "Sideboard", default)]
    pub sideboard: Vec<CacheCard>,
}

#[derive(Debug, Deserialize)]
pub struct CacheCard {
    #[serde(rename = "Count")]
    pub count: u32,
    #[serde(rename = "CardName")]
    pub card_name: String,
}

/// A tournament deck flattened for the meta pipeline.
#[derive(Debug, Clone)]
pub struct TournamentDeck {
    pub main: Vec<(String, u8)>,
    pub side: Vec<(String, u8)>,
    pub date_days: i64,
}

/// Walk every cached tournament JSON for a format within the window,
/// handing each parsed item (with its date) to the callback.
fn walk_cache(
    dir: &Path,
    format: &str,
    window_days: i64,
    f: &mut dyn FnMut(i64, CacheItem),
) -> Result<(), SourceError> {
    let cutoff = today_days() - window_days;
    let root = dir.join("Tournaments");
    if !root.exists() {
        return Ok(());
    }
    let mut stack: Vec<PathBuf> = vec![root];
    while let Some(p) = stack.pop() {
        for entry in std::fs::read_dir(&p)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().map(|e| e != "json").unwrap_or(true) {
                continue;
            }
            let rel = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let days = match path_date_days(&rel) {
                Some(d) if d >= cutoff => d,
                _ => continue,
            };
            let Ok(body) = std::fs::read_to_string(&path) else { continue };
            let Ok(item) = serde_json::from_str::<CacheItem>(&body) else { continue };
            let matches_format = item
                .tournament
                .formats
                .0
                .iter()
                .any(|f| f.eq_ignore_ascii_case(format))
                || item.tournament.name.to_ascii_lowercase().contains(&format.to_ascii_lowercase());
            if !matches_format {
                continue;
            }
            f(days, item);
        }
    }
    Ok(())
}

/// Load all decks for a format from the local cache within the window.
pub fn load_decks(
    dir: &Path,
    format: &str,
    window_days: i64,
) -> Result<Vec<TournamentDeck>, SourceError> {
    let mut out = Vec::new();
    walk_cache(dir, format, window_days, &mut |days, item| {
        for d in item.decks {
            if d.mainboard.is_empty() {
                continue;
            }
            out.push(TournamentDeck {
                main: d
                    .mainboard
                    .iter()
                    .map(|c| (c.card_name.clone(), c.count.min(250) as u8))
                    .collect(),
                side: d
                    .sideboard
                    .iter()
                    .map(|c| (c.card_name.clone(), c.count.min(250) as u8))
                    .collect(),
                date_days: days,
            });
        }
    })?;
    Ok(out)
}

/// One cached tournament with players, lists, and reported match results.
/// Player names are only unique within a single tournament, so the
/// grouping is load-bearing for any join against Rounds.
pub struct TournamentRecord {
    pub date_days: i64,
    pub decks: Vec<CacheDeck>,
    pub rounds: Vec<CacheRound>,
}

/// Load whole tournaments (decks plus rounds) for a format in the window.
pub fn load_tournaments(
    dir: &Path,
    format: &str,
    window_days: i64,
) -> Result<Vec<TournamentRecord>, SourceError> {
    let mut out = Vec::new();
    walk_cache(dir, format, window_days, &mut |days, item| {
        out.push(TournamentRecord {
            date_days: days,
            decks: item.decks,
            rounds: item.rounds,
        });
    })?;
    Ok(out)
}
