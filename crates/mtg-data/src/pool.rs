//! The in-memory card pool with a name index covering full names and
//! individual face names.

use std::collections::HashMap;

use crate::model::{CardId, OracleCard};

pub struct CardPool {
    cards: Vec<OracleCard>,
    by_name: HashMap<Box<str>, u32>,
}

/// Lowercase, straight apostrophes, collapsed whitespace.
pub fn normalize_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.trim().chars() {
        let c = match c {
            '\u{2019}' => '\'',
            c => c,
        };
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.extend(c.to_lowercase());
            last_space = false;
        }
    }
    out.trim_end().to_string()
}

impl CardPool {
    pub fn from_cards(cards: Vec<OracleCard>) -> CardPool {
        let mut by_name: HashMap<Box<str>, u32> = HashMap::with_capacity(cards.len() * 2);
        // Two passes so real cards claim names before tokens and emblems;
        // "Saproling" the token must not shadow a deck-playable card.
        for pass in 0..2 {
            for (i, card) in cards.iter().enumerate() {
                if (pass == 0) == card.is_extra() {
                    continue;
                }
                let mut insert = |name: &str| {
                    let key = normalize_name(name).into_boxed_str();
                    by_name.entry(key).or_insert(i as u32);
                };
                insert(&card.name);
                if card.faces.len() > 1 {
                    for f in &card.faces {
                        insert(&f.name);
                    }
                }
            }
        }
        CardPool { cards, by_name }
    }

    pub fn len(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub fn get(&self, id: CardId) -> &OracleCard {
        &self.cards[id.0 as usize]
    }

    pub fn iter(&self) -> impl Iterator<Item = (CardId, &OracleCard)> {
        self.cards.iter().enumerate().map(|(i, c)| (CardId(i as u32), c))
    }

    pub fn lookup(&self, name: &str) -> Option<CardId> {
        let key = normalize_name(name);
        if let Some(&i) = self.by_name.get(key.as_str()) {
            return Some(CardId(i));
        }
        // Deck files sometimes list only the front half of a split card or
        // include the full "A // B" for a card indexed by face.
        if let Some((front, _)) = key.split_once(" // ") {
            if let Some(&i) = self.by_name.get(front.trim()) {
                return Some(CardId(i));
            }
        }
        None
    }

    /// Closest names by edit distance, for unresolved-card error messages.
    pub fn suggest(&self, name: &str, max: usize) -> Vec<&str> {
        let key = normalize_name(name);
        let mut best: Vec<(usize, &str)> = Vec::new();
        for candidate in self.by_name.keys() {
            let d = bounded_levenshtein(&key, candidate, 4);
            if let Some(d) = d {
                best.push((d, candidate));
            }
        }
        best.sort_by_key(|(d, s)| (*d, s.len()));
        best.into_iter().take(max).map(|(_, s)| s).collect()
    }
}

/// Levenshtein with an early-exit bound; None when distance exceeds it.
fn bounded_levenshtein(a: &str, b: &str, bound: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > bound {
        return None;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            row_min = row_min.min(cur[j]);
        }
        if row_min > bound {
            return None;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    (prev[b.len()] <= bound).then_some(prev[b.len()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_normalization() {
        assert_eq!(normalize_name("  Lightning   BOLT "), "lightning bolt");
        assert_eq!(normalize_name("Urza\u{2019}s Saga"), "urza's saga");
    }

    #[test]
    fn edit_distance() {
        assert_eq!(bounded_levenshtein("bolt", "bolt", 2), Some(0));
        assert_eq!(bounded_levenshtein("bolt", "bort", 2), Some(1));
        assert_eq!(bounded_levenshtein("bolt", "counterspell", 2), None);
    }
}
