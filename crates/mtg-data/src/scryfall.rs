//! Scryfall bulk data: manifest fetch, bulk download, streaming parse.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::model::{normalize, OracleCard, RawCard};
use crate::DataError;

pub const BULK_MANIFEST_URL: &str = "https://api.scryfall.com/bulk-data";

pub fn agent(user_agent: &str) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent(user_agent)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .build()
}

#[derive(Debug, Clone)]
pub struct BulkManifest {
    pub updated_at: String,
    /// Pre-gzipped JSONL, the preferred download.
    pub jsonl_uri: Option<String>,
    pub json_uri: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct ManifestList {
    data: Vec<ManifestEntry>,
}

#[derive(Deserialize)]
struct ManifestEntry {
    #[serde(rename = "type")]
    kind: String,
    updated_at: String,
    download_uri: String,
    jsonl_download_uri: Option<String>,
    size: Option<u64>,
}

fn http_err(e: ureq::Error) -> DataError {
    DataError::Http(e.to_string())
}

pub fn fetch_manifest(agent: &ureq::Agent) -> Result<BulkManifest, DataError> {
    let list: ManifestList = agent
        .get(BULK_MANIFEST_URL)
        .set("Accept", "application/json")
        .call()
        .map_err(http_err)?
        .into_json()?;
    let entry = list
        .data
        .into_iter()
        .find(|e| e.kind == "oracle_cards")
        .ok_or(DataError::MissingBulk)?;
    Ok(BulkManifest {
        updated_at: entry.updated_at,
        jsonl_uri: entry.jsonl_download_uri,
        json_uri: entry.download_uri,
        size: entry.size.unwrap_or(0),
    })
}

/// Stream a URL to a file. Returns bytes written.
pub fn download_to(agent: &ureq::Agent, url: &str, dest: &Path) -> Result<u64, DataError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let resp = agent.get(url).call().map_err(http_err)?;
    let mut reader = resp.into_reader();
    let tmp = dest.with_extension("part");
    let mut out = std::io::BufWriter::new(File::create(&tmp)?);
    let written = std::io::copy(&mut reader, &mut out)?;
    out.flush()?;
    drop(out);
    if dest.exists() {
        std::fs::remove_file(dest)?;
    }
    std::fs::rename(&tmp, dest)?;
    Ok(written)
}

/// Parse a bulk file: gzipped JSONL, plain JSONL, or a plain JSON array.
pub fn parse_bulk_file(path: &Path) -> Result<Vec<OracleCard>, DataError> {
    let mut f = File::open(path)?;
    let mut magic = [0u8; 2];
    let n = f.read(&mut magic)?;
    let f = File::open(path)?;
    let reader: Box<dyn Read> = if n == 2 && magic == [0x1f, 0x8b] {
        Box::new(flate2::read::GzDecoder::new(f))
    } else {
        Box::new(f)
    };
    let mut reader = BufReader::with_capacity(1 << 20, reader);

    // Peek the first non-whitespace byte to detect a JSON array dump.
    let first = {
        let buf = reader.fill_buf()?;
        buf.iter().copied().find(|b| !b.is_ascii_whitespace())
    };
    if first == Some(b'[') {
        let raws: Vec<RawCard> = serde_json::from_reader(reader)?;
        return Ok(raws.iter().filter_map(normalize).collect());
    }

    let mut cards = Vec::with_capacity(40_000);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let raw: RawCard = serde_json::from_str(trimmed)?;
        if let Some(card) = normalize(&raw) {
            cards.push(card);
        }
    }
    Ok(cards)
}
