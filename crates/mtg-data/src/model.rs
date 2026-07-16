//! Normalized oracle card model plus the raw Scryfall serde shapes.

use serde::{Deserialize, Serialize};

use mtg_ir::{parse_type_line, CardTypes, ColorSet, Supertypes};

/// Dense index into the card pool. Stable for one pool build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardId(pub u32);

/// Scryfall oracle UUID, stored as raw bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OracleId(pub [u8; 16]);

impl OracleId {
    pub fn parse(s: &str) -> Option<OracleId> {
        let mut bytes = [0u8; 16];
        let mut idx = 0;
        let mut hi = None::<u8>;
        for c in s.chars() {
            if c == '-' {
                continue;
            }
            let v = c.to_digit(16)? as u8;
            match hi {
                None => hi = Some(v),
                Some(h) => {
                    if idx >= 16 {
                        return None;
                    }
                    bytes[idx] = (h << 4) | v;
                    idx += 1;
                    hi = None;
                }
            }
        }
        (idx == 16 && hi.is_none()).then_some(OracleId(bytes))
    }
}

impl std::fmt::Display for OracleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                write!(f, "-")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Layout {
    Normal,
    Split,
    Flip,
    Transform,
    ModalDfc,
    Adventure,
    Meld,
    Leveler,
    Class,
    Saga,
    Case,
    Prototype,
    Mutate,
    Battle,
    Token,
    Emblem,
    Reversible,
    Other,
}

impl Layout {
    pub fn parse(s: &str) -> Layout {
        match s {
            "normal" => Layout::Normal,
            "split" => Layout::Split,
            "flip" => Layout::Flip,
            "transform" => Layout::Transform,
            "modal_dfc" => Layout::ModalDfc,
            "adventure" => Layout::Adventure,
            "meld" => Layout::Meld,
            "leveler" => Layout::Leveler,
            "class" => Layout::Class,
            "saga" => Layout::Saga,
            "case" => Layout::Case,
            "prototype" => Layout::Prototype,
            "mutate" => Layout::Mutate,
            "battle" => Layout::Battle,
            "token" | "double_faced_token" => Layout::Token,
            "emblem" => Layout::Emblem,
            "reversible_card" => Layout::Reversible,
            _ => Layout::Other,
        }
    }

    /// Faces carry their own costs and stats; top-level fields are null.
    pub fn faces_carry_data(self) -> bool {
        matches!(self, Layout::Transform | Layout::ModalDfc | Layout::Reversible | Layout::Battle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Format {
    Standard,
    Pioneer,
    Modern,
    Legacy,
    Vintage,
    Pauper,
    Commander,
}

impl Format {
    pub const ALL: [Format; 7] = [
        Format::Standard,
        Format::Pioneer,
        Format::Modern,
        Format::Legacy,
        Format::Vintage,
        Format::Pauper,
        Format::Commander,
    ];

    pub fn index(self) -> usize {
        Format::ALL.iter().position(|f| *f == self).unwrap()
    }

    /// Scryfall legalities key.
    pub fn scryfall_key(self) -> &'static str {
        match self {
            Format::Standard => "standard",
            Format::Pioneer => "pioneer",
            Format::Modern => "modern",
            Format::Legacy => "legacy",
            Format::Vintage => "vintage",
            Format::Pauper => "pauper",
            Format::Commander => "commander",
        }
    }

    pub fn parse(s: &str) -> Option<Format> {
        let n = s.trim().to_ascii_lowercase();
        Format::ALL.into_iter().find(|f| f.scryfall_key() == n).or(match n.as_str() {
            "edh" | "cedh" => Some(Format::Commander),
            _ => None,
        })
    }

    pub fn is_singleton(self) -> bool {
        matches!(self, Format::Commander)
    }

    pub fn deck_size(self) -> usize {
        match self {
            Format::Commander => 100,
            _ => 60,
        }
    }

    pub fn starting_life(self) -> i32 {
        match self {
            Format::Commander => 40,
            _ => 20,
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Format::Standard => "Standard",
            Format::Pioneer => "Pioneer",
            Format::Modern => "Modern",
            Format::Legacy => "Legacy",
            Format::Vintage => "Vintage",
            Format::Pauper => "Pauper",
            Format::Commander => "Commander",
        };
        write!(f, "{name}")
    }
}

/// Packed per-format legality. Restricted implies legal with a 1-of cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Legalities {
    legal_bits: u8,
    restricted_bits: u8,
}

impl Legalities {
    pub fn from_raw(map: &std::collections::HashMap<String, String>) -> Legalities {
        let mut l = Legalities::default();
        for f in Format::ALL {
            match map.get(f.scryfall_key()).map(String::as_str) {
                Some("legal") => l.legal_bits |= 1 << f.index(),
                Some("restricted") => {
                    l.legal_bits |= 1 << f.index();
                    l.restricted_bits |= 1 << f.index();
                }
                _ => {}
            }
        }
        l
    }

    pub fn is_legal(&self, f: Format) -> bool {
        self.legal_bits & (1 << f.index()) != 0
    }

    pub fn is_restricted(&self, f: Format) -> bool {
        self.restricted_bits & (1 << f.index()) != 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleFace {
    pub name: Box<str>,
    pub mana_cost: Box<str>,
    pub type_line: Box<str>,
    pub oracle_text: Box<str>,
    pub types: CardTypes,
    pub supertypes: Supertypes,
    pub subtypes: Vec<Box<str>>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    /// Power or toughness contained "*"; the numeric part is stored.
    pub pt_star: bool,
    pub loyalty: Option<i32>,
    pub colors: ColorSet,
}

impl OracleFace {
    pub fn is_creature(&self) -> bool {
        self.types.contains(CardTypes::CREATURE)
    }

    pub fn is_land(&self) -> bool {
        self.types.contains(CardTypes::LAND)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleCard {
    pub oracle_id: OracleId,
    pub name: Box<str>,
    pub layout: Layout,
    pub cmc: f32,
    pub color_identity: ColorSet,
    /// Raw Scryfall keyword names; the compiler consumes these.
    pub keywords: Vec<Box<str>>,
    pub legalities: Legalities,
    pub produced_mana: ColorSet,
    pub faces: Vec<OracleFace>,
}

impl OracleCard {
    pub fn front(&self) -> &OracleFace {
        &self.faces[0]
    }

    /// True for layouts that can never sit in a deck.
    pub fn is_extra(&self) -> bool {
        matches!(self.layout, Layout::Token | Layout::Emblem | Layout::Other)
    }
}

// Raw Scryfall bulk shapes. Only the fields we consume.

#[derive(Debug, Deserialize)]
pub struct RawCard {
    pub oracle_id: Option<String>,
    pub name: String,
    pub layout: String,
    pub cmc: Option<f32>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub colors: Option<Vec<String>>,
    pub color_identity: Option<Vec<String>>,
    pub keywords: Option<Vec<String>>,
    pub legalities: Option<std::collections::HashMap<String, String>>,
    pub produced_mana: Option<Vec<String>>,
    pub card_faces: Option<Vec<RawFace>>,
}

#[derive(Debug, Deserialize)]
pub struct RawFace {
    pub oracle_id: Option<String>,
    pub name: String,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub colors: Option<Vec<String>>,
}

fn parse_pt(s: &str) -> (Option<i32>, bool) {
    let star = s.contains('*');
    let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '-').collect();
    let n = cleaned.parse::<i32>().ok();
    if star {
        (Some(n.unwrap_or(0)), true)
    } else {
        (n, false)
    }
}

fn colors_of(v: &Option<Vec<String>>) -> ColorSet {
    v.as_ref()
        .map(|c| ColorSet::from_letters(c.iter().map(String::as_str)))
        .unwrap_or_default()
}

fn build_face(
    name: &str,
    mana_cost: Option<&str>,
    type_line: Option<&str>,
    oracle_text: Option<&str>,
    power: Option<&str>,
    toughness: Option<&str>,
    loyalty: Option<&str>,
    colors: ColorSet,
) -> OracleFace {
    let tl = type_line.unwrap_or("");
    let (types, supertypes, subtypes) = parse_type_line(tl);
    let (p, star_p) = power.map(parse_pt).unwrap_or((None, false));
    let (t, star_t) = toughness.map(parse_pt).unwrap_or((None, false));
    let loyalty = loyalty.and_then(|l| {
        if l.contains('X') {
            Some(0)
        } else {
            l.parse::<i32>().ok()
        }
    });
    OracleFace {
        name: name.into(),
        mana_cost: mana_cost.unwrap_or("").into(),
        type_line: tl.into(),
        oracle_text: oracle_text.unwrap_or("").into(),
        types,
        supertypes,
        subtypes,
        power: p,
        toughness: t,
        pt_star: star_p || star_t,
        loyalty,
        colors,
    }
}

/// Normalize one raw bulk entry. Returns None for cards we cannot key or
/// that are pure art products.
pub fn normalize(raw: &RawCard) -> Option<OracleCard> {
    let layout = Layout::parse(&raw.layout);
    if raw.layout == "art_series" || raw.layout == "vanguard" || raw.layout == "scheme" {
        return None;
    }
    let oracle_id = raw
        .oracle_id
        .as_deref()
        .or_else(|| {
            raw.card_faces
                .as_ref()
                .and_then(|f| f.first())
                .and_then(|f| f.oracle_id.as_deref())
        })
        .and_then(OracleId::parse)?;

    let top_colors = colors_of(&raw.colors);
    let faces: Vec<OracleFace> = match &raw.card_faces {
        Some(rf) if !rf.is_empty() => rf
            .iter()
            .map(|f| {
                let fc = if f.colors.is_some() { colors_of(&f.colors) } else { top_colors };
                build_face(
                    &f.name,
                    f.mana_cost.as_deref(),
                    f.type_line.as_deref(),
                    f.oracle_text.as_deref(),
                    f.power.as_deref(),
                    f.toughness.as_deref(),
                    f.loyalty.as_deref(),
                    fc,
                )
            })
            .collect(),
        _ => vec![build_face(
            &raw.name,
            raw.mana_cost.as_deref(),
            raw.type_line.as_deref(),
            raw.oracle_text.as_deref(),
            raw.power.as_deref(),
            raw.toughness.as_deref(),
            raw.loyalty.as_deref(),
            top_colors,
        )],
    };

    Some(OracleCard {
        oracle_id,
        name: raw.name.as_str().into(),
        layout,
        cmc: raw.cmc.unwrap_or(0.0),
        color_identity: raw
            .color_identity
            .as_ref()
            .map(|c| ColorSet::from_letters(c.iter().map(String::as_str)))
            .unwrap_or_default(),
        keywords: raw
            .keywords
            .as_ref()
            .map(|k| k.iter().map(|s| s.as_str().into()).collect())
            .unwrap_or_default(),
        legalities: raw
            .legalities
            .as_ref()
            .map(Legalities::from_raw)
            .unwrap_or_default(),
        produced_mana: raw
            .produced_mana
            .as_ref()
            .map(|c| ColorSet::from_letters(c.iter().map(String::as_str)))
            .unwrap_or_default(),
        faces,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_id_roundtrip() {
        let s = "f2a3b1c4-0000-4a5b-8c6d-9e7f01234567";
        let id = OracleId::parse(s).unwrap();
        assert_eq!(id.to_string(), s);
    }

    #[test]
    fn pt_parsing() {
        assert_eq!(parse_pt("3"), (Some(3), false));
        assert_eq!(parse_pt("*"), (Some(0), true));
        assert_eq!(parse_pt("1+*"), (Some(1), true));
        assert_eq!(parse_pt("-1"), (Some(-1), false));
    }
}
