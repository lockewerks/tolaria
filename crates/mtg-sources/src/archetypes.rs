//! Archetype classification: a Rust port of the MTGOFormatData condition
//! engine, fed by the Badaro/MTGOFormatData repository.

use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;

use crate::http::get_bytes;
use crate::SourceError;

pub const FORMATDATA_TARBALL: &str =
    "https://codeload.github.com/Badaro/MTGOFormatData/tar.gz/refs/heads/main";

#[derive(Debug, Deserialize)]
pub struct ArchetypeRule {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Conditions", default)]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Deserialize)]
pub struct Condition {
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Cards", default)]
    pub cards: Vec<String>,
}

#[derive(Debug, Default)]
pub struct FormatRules {
    pub archetypes: Vec<ArchetypeRule>,
    pub fallbacks: Vec<FallbackRule>,
}

#[derive(Debug, Deserialize)]
pub struct FallbackRule {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "CommonCards", default)]
    pub common_cards: Vec<String>,
}

/// Download and unpack Formats/<format>/ rules into the destination dir.
pub fn fetch_format_rules(
    agent: &ureq::Agent,
    dest: &Path,
) -> Result<(), SourceError> {
    let bytes = get_bytes(agent, FORMATDATA_TARBALL)?;
    let gz = flate2::read::GzDecoder::new(bytes.as_slice());
    let mut tar = tar::Archive::new(gz);
    for entry in tar.entries().map_err(SourceError::Io)? {
        let mut entry = entry.map_err(SourceError::Io)?;
        let path = entry.path().map_err(SourceError::Io)?.into_owned();
        let rel: std::path::PathBuf = path.components().skip(1).collect();
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if !rel_str.starts_with("Formats/") || !rel_str.ends_with(".json") {
            continue;
        }
        let out = dest.join(&rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(&out).map_err(SourceError::Io)?;
    }
    Ok(())
}

/// MTGOFormatData format directory names.
pub fn format_dir_name(format: mtg_data::Format) -> &'static str {
    match format {
        mtg_data::Format::Standard => "Standard",
        mtg_data::Format::Pioneer => "Pioneer",
        mtg_data::Format::Modern => "Modern",
        mtg_data::Format::Legacy => "Legacy",
        mtg_data::Format::Vintage => "Vintage",
        mtg_data::Format::Pauper => "Pauper",
        mtg_data::Format::Commander => "Commander",
    }
}

pub fn load_rules(rules_dir: &Path, format: mtg_data::Format) -> Result<FormatRules, SourceError> {
    let base = rules_dir.join("Formats").join(format_dir_name(format));
    let mut out = FormatRules::default();
    let arch = base.join("Archetypes");
    if arch.exists() {
        for entry in std::fs::read_dir(&arch)? {
            let path = entry?.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(body) = std::fs::read_to_string(&path) {
                    if let Ok(rule) = serde_json::from_str::<ArchetypeRule>(&body) {
                        out.archetypes.push(rule);
                    }
                }
            }
        }
    }
    let fall = base.join("Fallbacks");
    if fall.exists() {
        for entry in std::fs::read_dir(&fall)? {
            let path = entry?.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(body) = std::fs::read_to_string(&path) {
                    if let Ok(rule) = serde_json::from_str::<FallbackRule>(&body) {
                        out.fallbacks.push(rule);
                    }
                }
            }
        }
    }
    Ok(out)
}

fn count_in(set: &HashSet<String>, cards: &[String]) -> usize {
    cards.iter().filter(|c| set.contains(&c.to_ascii_lowercase())).count()
}

/// Classify a deck by the first matching archetype rule, falling back to
/// the best common-cards overlap.
pub fn classify(
    rules: &FormatRules,
    main: &[(String, u8)],
    side: &[(String, u8)],
) -> Option<String> {
    let main_set: HashSet<String> = main.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
    let side_set: HashSet<String> = side.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();

    'rules: for rule in &rules.archetypes {
        if rule.conditions.is_empty() {
            continue;
        }
        for cond in &rule.conditions {
            let ok = match cond.kind.as_str() {
                "InMainboard" => count_in(&main_set, &cond.cards) == cond.cards.len(),
                "InSideboard" => count_in(&side_set, &cond.cards) == cond.cards.len(),
                "InMainOrSideboard" => cond.cards.iter().all(|c| {
                    let k = c.to_ascii_lowercase();
                    main_set.contains(&k) || side_set.contains(&k)
                }),
                "OneOrMoreInMainboard" => count_in(&main_set, &cond.cards) >= 1,
                "OneOrMoreInSideboard" => count_in(&side_set, &cond.cards) >= 1,
                "OneOrMoreInMainOrSideboard" => {
                    count_in(&main_set, &cond.cards) >= 1 || count_in(&side_set, &cond.cards) >= 1
                }
                "TwoOrMoreInMainboard" => count_in(&main_set, &cond.cards) >= 2,
                "DoesNotContain" => {
                    cond.cards.iter().all(|c| {
                        let k = c.to_ascii_lowercase();
                        !main_set.contains(&k) && !side_set.contains(&k)
                    })
                }
                "DoesNotContainMainboard" => count_in(&main_set, &cond.cards) == 0,
                "DoesNotContainSideboard" => count_in(&side_set, &cond.cards) == 0,
                _ => false,
            };
            if !ok {
                continue 'rules;
            }
        }
        return Some(rule.name.clone());
    }

    // Fallback: strongest common-card overlap, requiring a minimum signal.
    let mut best: Option<(usize, &str)> = None;
    for fb in &rules.fallbacks {
        let score = count_in(&main_set, &fb.common_cards);
        if score >= 10 && best.map(|(s, _)| score > s).unwrap_or(true) {
            best = Some((score, &fb.name));
        }
    }
    best.map(|(_, n)| n.to_string())
}
