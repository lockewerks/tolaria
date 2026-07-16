//! The per-match card database: only the cards present in the participating
//! decks, paired with their compiled behaviors. Small and cache-hot; shared
//! read-only across all games of a batch.

use mtg_data::{OracleCard, OracleFace};
use mtg_ir::{CompiledCard, CompiledFace};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CardRef(pub u32);

#[derive(Debug, Clone)]
pub struct GameCard {
    pub oracle: OracleCard,
    pub compiled: CompiledCard,
}

#[derive(Debug, Default)]
pub struct CardDb {
    pub cards: Vec<GameCard>,
}

impl CardDb {
    pub fn add(&mut self, oracle: OracleCard, compiled: CompiledCard) -> CardRef {
        let r = CardRef(self.cards.len() as u32);
        self.cards.push(GameCard { oracle, compiled });
        r
    }

    #[inline]
    pub fn get(&self, r: CardRef) -> &GameCard {
        &self.cards[r.0 as usize]
    }

    #[inline]
    pub fn face(&self, r: CardRef, face: u8) -> &OracleFace {
        let faces = &self.get(r).oracle.faces;
        faces.get(face as usize).unwrap_or(&faces[0])
    }

    #[inline]
    pub fn compiled_face(&self, r: CardRef, face: u8) -> &CompiledFace {
        let faces = &self.get(r).compiled.faces;
        faces.get(face as usize).unwrap_or(&faces[0])
    }
}
