//! Colors, mana costs, and mana production.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Color {
    W,
    U,
    B,
    R,
    G,
}

impl Color {
    pub const ALL: [Color; 5] = [Color::W, Color::U, Color::B, Color::R, Color::G];

    pub fn index(self) -> usize {
        match self {
            Color::W => 0,
            Color::U => 1,
            Color::B => 2,
            Color::R => 3,
            Color::G => 4,
        }
    }

    pub fn from_symbol(c: char) -> Option<Color> {
        Some(match c.to_ascii_uppercase() {
            'W' => Color::W,
            'U' => Color::U,
            'B' => Color::B,
            'R' => Color::R,
            'G' => Color::G,
            _ => return None,
        })
    }

    pub fn set(self) -> ColorSet {
        match self {
            Color::W => ColorSet::W,
            Color::U => ColorSet::U,
            Color::B => ColorSet::B,
            Color::R => ColorSet::R,
            Color::G => ColorSet::G,
        }
    }
}

bitflags::bitflags! {
    /// The five colors plus colorless as a produced-mana marker.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct ColorSet: u8 {
        const W = 1 << 0;
        const U = 1 << 1;
        const B = 1 << 2;
        const R = 1 << 3;
        const G = 1 << 4;
        const C = 1 << 5;
    }
}

impl Serialize for ColorSet {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.bits().serialize(s)
    }
}
impl<'de> Deserialize<'de> for ColorSet {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(ColorSet::from_bits_truncate(u8::deserialize(d)?))
    }
}

impl ColorSet {
    pub fn from_letters<'a>(letters: impl IntoIterator<Item = &'a str>) -> ColorSet {
        let mut set = ColorSet::empty();
        for l in letters {
            match l {
                "W" => set |= ColorSet::W,
                "U" => set |= ColorSet::U,
                "B" => set |= ColorSet::B,
                "R" => set |= ColorSet::R,
                "G" => set |= ColorSet::G,
                "C" => set |= ColorSet::C,
                _ => {}
            }
        }
        set
    }

    pub fn colors(self) -> impl Iterator<Item = Color> {
        Color::ALL.into_iter().filter(move |c| self.contains(c.set()))
    }
}

/// A hybrid pip: pay either side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HybridPip {
    /// {W/U} style: one of two colors.
    Colors(Color, Color),
    /// {2/W} style: two generic or the color.
    TwoOr(Color),
}

/// A Phyrexian pip: pay the color(s) or 2 life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhyPip(pub Color, pub Option<Color>);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct ManaCost {
    pub generic: u16,
    pub x_count: u8,
    /// WUBRG colored pip counts, indexed by Color::index.
    pub pips: [u8; 5],
    /// {C} pips: must be paid with colorless.
    pub colorless: u8,
    /// {S} pips: payable by any snow source. The engine treats these as
    /// generic since snow-ness of sources is not tracked.
    pub snow: u8,
    pub hybrid: SmallVec<[HybridPip; 2]>,
    pub phyrexian: SmallVec<[PhyPip; 2]>,
}

impl ManaCost {
    /// Parse a Scryfall mana cost string like "{2}{W}{W}" or "{X}{G/U}{B/P}".
    /// Returns None on symbols we do not model (un-set halves, etc.).
    pub fn parse(s: &str) -> Option<ManaCost> {
        let mut cost = ManaCost::default();
        let mut rest = s.trim();
        while !rest.is_empty() {
            if !rest.starts_with('{') {
                return None;
            }
            let end = rest.find('}')?;
            let tok_upper = rest[1..end].to_ascii_uppercase();
            let tok = tok_upper.as_str();
            rest = &rest[end + 1..];
            if let Ok(n) = tok.parse::<u16>() {
                cost.generic += n;
                continue;
            }
            match tok {
                "X" | "Y" | "Z" => cost.x_count += 1,
                "W" | "U" | "B" | "R" | "G" => {
                    let c = Color::from_symbol(tok.chars().next().unwrap()).unwrap();
                    cost.pips[c.index()] += 1;
                }
                "C" => cost.colorless += 1,
                "S" => cost.snow += 1,
                _ if tok.contains('/') => {
                    let parts: Vec<&str> = tok.split('/').collect();
                    match parts.as_slice() {
                        [a, "P"] => {
                            let c = Color::from_symbol(a.chars().next()?)?;
                            cost.phyrexian.push(PhyPip(c, None));
                        }
                        [a, b, "P"] => {
                            let c1 = Color::from_symbol(a.chars().next()?)?;
                            let c2 = Color::from_symbol(b.chars().next()?)?;
                            cost.phyrexian.push(PhyPip(c1, Some(c2)));
                        }
                        ["2", b] => {
                            let c = Color::from_symbol(b.chars().next()?)?;
                            cost.hybrid.push(HybridPip::TwoOr(c));
                        }
                        [a, b] => {
                            let c1 = Color::from_symbol(a.chars().next()?)?;
                            let c2 = Color::from_symbol(b.chars().next()?)?;
                            cost.hybrid.push(HybridPip::Colors(c1, c2));
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        Some(cost)
    }

    /// Mana value with X bound to the given value.
    pub fn mana_value(&self, x: u32) -> u32 {
        let mut v = self.generic as u32
            + self.colorless as u32
            + self.snow as u32
            + self.phyrexian.len() as u32
            + self.x_count as u32 * x;
        v += self.pips.iter().map(|&p| p as u32).sum::<u32>();
        for h in &self.hybrid {
            v += match h {
                HybridPip::Colors(..) => 1,
                HybridPip::TwoOr(..) => 2,
            };
        }
        v
    }

    /// Colors of the mana symbols, for color determination.
    pub fn colors(&self) -> ColorSet {
        let mut set = ColorSet::empty();
        for c in Color::ALL {
            if self.pips[c.index()] > 0 {
                set |= c.set();
            }
        }
        for h in &self.hybrid {
            match h {
                HybridPip::Colors(a, b) => set |= a.set() | b.set(),
                HybridPip::TwoOr(a) => set |= a.set(),
            }
        }
        for p in &self.phyrexian {
            set |= p.0.set();
            if let Some(b) = p.1 {
                set |= b.set();
            }
        }
        set
    }

    pub fn is_zero(&self) -> bool {
        self.generic == 0
            && self.x_count == 0
            && self.pips == [0; 5]
            && self.colorless == 0
            && self.snow == 0
            && self.hybrid.is_empty()
            && self.phyrexian.is_empty()
    }
}

/// What a mana ability produces.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ManaProduction {
    Fixed { w: u8, u: u8, b: u8, r: u8, g: u8, c: u8 },
    /// Add one mana of one of these colors.
    AnyOneOf(ColorSet),
    /// Add one mana of any color.
    AnyColor,
    Custom(crate::compiled::OverrideId),
}

impl ManaProduction {
    pub fn one(color: Color) -> ManaProduction {
        let mut p = ManaProduction::Fixed { w: 0, u: 0, b: 0, r: 0, g: 0, c: 0 };
        if let ManaProduction::Fixed { w, u, b, r, g, .. } = &mut p {
            match color {
                Color::W => *w = 1,
                Color::U => *u = 1,
                Color::B => *b = 1,
                Color::R => *r = 1,
                Color::G => *g = 1,
            }
        }
        p
    }

    pub fn colorless(n: u8) -> ManaProduction {
        ManaProduction::Fixed { w: 0, u: 0, b: 0, r: 0, g: 0, c: n }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple() {
        let c = ManaCost::parse("{2}{W}{W}").unwrap();
        assert_eq!(c.generic, 2);
        assert_eq!(c.pips[Color::W.index()], 2);
        assert_eq!(c.mana_value(0), 4);
    }

    #[test]
    fn parses_exotic() {
        let c = ManaCost::parse("{X}{G/U}{B/P}{2/W}{S}{C}").unwrap();
        assert_eq!(c.x_count, 1);
        assert_eq!(c.hybrid.len(), 2);
        assert_eq!(c.phyrexian.len(), 1);
        assert_eq!(c.snow, 1);
        assert_eq!(c.colorless, 1);
        assert_eq!(c.mana_value(3), 3 + 1 + 1 + 2 + 1 + 1);
    }

    #[test]
    fn rejects_unknown() {
        assert!(ManaCost::parse("{H W}").is_none());
    }

    #[test]
    fn empty_is_free() {
        assert!(ManaCost::parse("").unwrap().is_zero());
    }
}
