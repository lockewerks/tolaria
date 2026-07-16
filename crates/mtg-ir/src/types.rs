//! Card types, supertypes, keyword flags, counters, and the serde shim for
//! bitflags types.

use serde::{Deserialize, Serialize};

/// bitflags does not ship serde impls we can rely on across versions, so
/// serialize the raw bits ourselves.
macro_rules! serde_bits {
    ($ty:ty, $repr:ty) => {
        impl Serialize for $ty {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                self.bits().serialize(s)
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Ok(<$ty>::from_bits_truncate(<$repr>::deserialize(d)?))
            }
        }
    };
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct CardTypes: u16 {
        const ARTIFACT     = 1 << 0;
        const BATTLE       = 1 << 1;
        const CREATURE     = 1 << 2;
        const ENCHANTMENT  = 1 << 3;
        const INSTANT      = 1 << 4;
        const KINDRED      = 1 << 5;
        const LAND         = 1 << 6;
        const PLANESWALKER = 1 << 7;
        const SORCERY      = 1 << 8;
        /// Anything we do not model (schemes, conspiracies, vanguards).
        const OTHER        = 1 << 9;
    }
}
serde_bits!(CardTypes, u16);

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Supertypes: u8 {
        const BASIC     = 1 << 0;
        const LEGENDARY = 1 << 1;
        const SNOW      = 1 << 2;
        const WORLD     = 1 << 3;
        const OTHER     = 1 << 4;
    }
}
serde_bits!(Supertypes, u8);

bitflags::bitflags! {
    /// Binary keyword abilities checked directly by combat, timing, and SBA
    /// code. Keywords with payloads (ward cost, protection colors, toxic N)
    /// live as structured fields on CompiledFace instead.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct KeywordSet: u64 {
        const FLYING          = 1 << 0;
        const REACH           = 1 << 1;
        const TRAMPLE         = 1 << 2;
        const HASTE           = 1 << 3;
        const VIGILANCE       = 1 << 4;
        const FIRST_STRIKE    = 1 << 5;
        const DOUBLE_STRIKE   = 1 << 6;
        const DEATHTOUCH      = 1 << 7;
        const LIFELINK        = 1 << 8;
        const MENACE          = 1 << 9;
        const DEFENDER        = 1 << 10;
        const INDESTRUCTIBLE  = 1 << 11;
        const HEXPROOF        = 1 << 12;
        const SHROUD          = 1 << 13;
        const FLASH           = 1 << 14;
        const PROWESS         = 1 << 15;
        const FEAR            = 1 << 16;
        const INTIMIDATE      = 1 << 17;
        const SHADOW          = 1 << 18;
        const UNBLOCKABLE     = 1 << 19;
        const CANT_BLOCK      = 1 << 20;
        const INFECT          = 1 << 21;
        const WITHER          = 1 << 22;
        const PERSIST         = 1 << 23;
        const UNDYING         = 1 << 24;
        const CASCADE         = 1 << 25;
        const STORM           = 1 << 26;
        const SPLIT_SECOND    = 1 << 27;
        const ATTACKS_EACH_TURN = 1 << 28;
        const EXALTED         = 1 << 29;
        const CHANGELING      = 1 << 30;
    }
}
serde_bits!(KeywordSet, u64);

impl KeywordSet {
    /// Map a Scryfall `keywords` array entry to a flag. Case-insensitive.
    /// Returns None for keywords that need structured data or that the
    /// compiler handles elsewhere (ward, protection, flashback, ...).
    pub fn from_scryfall(name: &str) -> Option<KeywordSet> {
        let n = name.trim().to_ascii_lowercase();
        Some(match n.as_str() {
            "flying" => Self::FLYING,
            "reach" => Self::REACH,
            "trample" => Self::TRAMPLE,
            "haste" => Self::HASTE,
            "vigilance" => Self::VIGILANCE,
            "first strike" => Self::FIRST_STRIKE,
            "double strike" => Self::DOUBLE_STRIKE,
            "deathtouch" => Self::DEATHTOUCH,
            "lifelink" => Self::LIFELINK,
            "menace" => Self::MENACE,
            "defender" => Self::DEFENDER,
            "indestructible" => Self::INDESTRUCTIBLE,
            "hexproof" => Self::HEXPROOF,
            "shroud" => Self::SHROUD,
            "flash" => Self::FLASH,
            "prowess" => Self::PROWESS,
            "fear" => Self::FEAR,
            "intimidate" => Self::INTIMIDATE,
            "shadow" => Self::SHADOW,
            "infect" => Self::INFECT,
            "wither" => Self::WITHER,
            "persist" => Self::PERSIST,
            "undying" => Self::UNDYING,
            "cascade" => Self::CASCADE,
            "storm" => Self::STORM,
            "split second" => Self::SPLIT_SECOND,
            "exalted" => Self::EXALTED,
            "changeling" => Self::CHANGELING,
            _ => return None,
        })
    }
}

bitflags::bitflags! {
    /// Cost-modifying cast mechanics the payment solver understands.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct CastMods: u8 {
        const CONVOKE   = 1 << 0;
        const DELVE     = 1 << 1;
        const AFFINITY  = 1 << 2;
        const IMPROVISE = 1 << 3;
    }
}
serde_bits!(CastMods, u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CounterKind {
    PlusOne,
    MinusOne,
    Loyalty,
    Charge,
    /// Named counters whose identity we track but whose semantics come from
    /// the ability that reads them.
    Other,
}

/// Parse a type line like "Legendary Creature - Human Wizard" into parts.
/// Handles both the em dash used by oracle text and a plain hyphen.
pub fn parse_type_line(line: &str) -> (CardTypes, Supertypes, Vec<Box<str>>) {
    let mut types = CardTypes::empty();
    let mut supers = Supertypes::empty();
    let mut subtypes = Vec::new();

    // Faces of split cards can carry "Instant // Sorcery" style lines; the
    // caller passes one face at a time so this only sees a single half.
    let (left, right) = match line.split_once('\u{2014}') {
        Some((l, r)) => (l, Some(r)),
        None => match line.split_once(" - ") {
            Some((l, r)) => (l, Some(r)),
            None => (line, None),
        },
    };

    for word in left.split_whitespace() {
        match word {
            "Artifact" => types |= CardTypes::ARTIFACT,
            "Battle" => types |= CardTypes::BATTLE,
            "Creature" => types |= CardTypes::CREATURE,
            "Enchantment" => types |= CardTypes::ENCHANTMENT,
            "Instant" => types |= CardTypes::INSTANT,
            "Kindred" | "Tribal" => types |= CardTypes::KINDRED,
            "Land" => types |= CardTypes::LAND,
            "Planeswalker" => types |= CardTypes::PLANESWALKER,
            "Sorcery" => types |= CardTypes::SORCERY,
            "Basic" => supers |= Supertypes::BASIC,
            "Legendary" => supers |= Supertypes::LEGENDARY,
            "Snow" => supers |= Supertypes::SNOW,
            "World" => supers |= Supertypes::WORLD,
            "Token" => {}
            _ => {
                if types.is_empty() {
                    types |= CardTypes::OTHER;
                }
            }
        }
    }
    if let Some(r) = right {
        for word in r.split_whitespace() {
            subtypes.push(word.to_ascii_lowercase().into_boxed_str());
        }
    }
    (types, supers, subtypes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_line_parses() {
        let (t, s, sub) = parse_type_line("Legendary Creature \u{2014} Human Wizard");
        assert!(t.contains(CardTypes::CREATURE));
        assert!(s.contains(Supertypes::LEGENDARY));
        assert_eq!(sub, vec!["human".into(), "wizard".into()]);
    }

    #[test]
    fn keyword_mapping() {
        assert_eq!(
            KeywordSet::from_scryfall("First strike"),
            Some(KeywordSet::FIRST_STRIKE)
        );
        assert_eq!(KeywordSet::from_scryfall("Ward"), None);
    }
}
