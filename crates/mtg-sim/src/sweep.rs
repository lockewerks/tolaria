//! Exhaustive opening-hand sweep: enumerate every distinct opening seven of
//! the user's deck, weight each by its exact hypergeometric probability,
//! and sample continuations per hand.
//!
//! Full deck-order enumeration is physically impossible (a 60-card deck has
//! on the order of 10^60 to 10^80 distinct orderings), but the opening hand
//! is where deal variance concentrates, and the distinct hands of a
//! constructed deck number in the thousands. Exact where exactness is
//! possible, Monte Carlo where it is not.

use std::sync::atomic::Ordering;

use rayon::prelude::*;

use mtg_data::{CardId, CardPool};
use mtg_engine::{Agents, GameEnd, GameSetup};

use crate::{build_db, game_seed, MatchupProgress, SimConfig, SimDeck};

/// Exact binomial coefficient in u128 (n up to a few hundred, k <= 7).
fn binom(n: u64, k: u64) -> u128 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut num: u128 = 1;
    let mut den: u128 = 1;
    for i in 0..k {
        num *= (n - i) as u128;
        den *= (i + 1) as u128;
    }
    num / den
}

/// One distinct opening hand: counts per distinct card, with its exact
/// probability of being dealt.
#[derive(Debug, Clone)]
pub struct HandCombo {
    pub cards: Vec<(CardId, u8)>,
    pub probability: f64,
}

/// Enumerate every distinct hand of `hand_size` cards from the deck's
/// distinct-card counts. The probabilities sum to 1 exactly (up to float
/// rounding) because this is the full hypergeometric support.
pub fn enumerate_hands(deck: &[(CardId, u8)], hand_size: u8) -> Vec<HandCombo> {
    let total: u64 = deck.iter().map(|(_, c)| *c as u64).sum();
    let denom = binom(total, hand_size as u64) as f64;
    let mut out = Vec::new();
    let mut current: Vec<(CardId, u8)> = Vec::new();

    fn recurse(
        deck: &[(CardId, u8)],
        idx: usize,
        left: u8,
        weight: u128,
        current: &mut Vec<(CardId, u8)>,
        denom: f64,
        out: &mut Vec<HandCombo>,
    ) {
        if left == 0 {
            out.push(HandCombo {
                cards: current.clone(),
                probability: weight as f64 / denom,
            });
            return;
        }
        if idx >= deck.len() {
            return;
        }
        // Bound: remaining copies must be able to fill the hand.
        let remaining: u64 = deck[idx..].iter().map(|(_, c)| *c as u64).sum();
        if remaining < left as u64 {
            return;
        }
        let (cid, count) = deck[idx];
        let max_take = count.min(left);
        for take in 0..=max_take {
            let w = weight * binom(count as u64, take as u64);
            if take > 0 {
                current.push((cid, take));
            }
            recurse(deck, idx + 1, left - take, w, current, denom, out);
            if take > 0 {
                current.pop();
            }
        }
    }
    recurse(deck, 0, hand_size, 1, &mut current, denom, &mut out);
    out
}

/// Per-hand outcome of the sweep.
#[derive(Debug, Clone)]
pub struct HandOutcome {
    pub cards: Vec<(CardId, u8)>,
    pub probability: f64,
    pub games: u32,
    pub wins: u32,
    pub draws: u32,
}

impl HandOutcome {
    pub fn win_rate(&self) -> f64 {
        if self.games == 0 {
            return 0.5;
        }
        (self.wins as f64 + self.draws as f64 * 0.5) / self.games as f64
    }
}

#[derive(Debug, Clone)]
pub struct SweepStats {
    /// Probability-weighted win rate over all distinct opening hands.
    pub weighted_win_rate: f64,
    /// Stratified standard error of the weighted estimate.
    pub standard_error: f64,
    pub total_games: u64,
    pub distinct_hands: usize,
    pub panics: u32,
    pub hands: Vec<HandOutcome>,
}

impl SweepStats {
    pub fn ci95(&self) -> (f64, f64) {
        let half = 1.96 * self.standard_error;
        (
            (self.weighted_win_rate - half).max(0.0),
            (self.weighted_win_rate + half).min(1.0),
        )
    }
}

/// Refuse to sweep hand spaces that cannot finish. Singleton piles explode
/// combinatorially; Monte Carlo is the right tool there.
pub const MAX_SWEEP_HANDS: usize = 250_000;

pub fn count_hands(deck: &[(CardId, u8)], hand_size: u8) -> u128 {
    // Same recursion as enumerate_hands, counting only.
    fn recurse(deck: &[(CardId, u8)], idx: usize, left: u8) -> u128 {
        if left == 0 {
            return 1;
        }
        if idx >= deck.len() {
            return 0;
        }
        let remaining: u64 = deck[idx..].iter().map(|(_, c)| *c as u64).sum();
        if remaining < left as u64 {
            return 0;
        }
        let (_, count) = deck[idx];
        let mut n = 0u128;
        for take in 0..=count.min(left) {
            n += recurse(deck, idx + 1, left - take);
        }
        n
    }
    recurse(deck, 0, hand_size)
}

/// Run the sweep: every distinct opening hand for seat 0, `per_hand`
/// continuations each (opponent shuffles freely; play/draw alternates).
pub fn run_hand_sweep(
    pool: &CardPool,
    user: &SimDeck,
    opp: &SimDeck,
    cfg: &SimConfig,
    per_hand: u32,
    progress: &MatchupProgress,
) -> SweepStats {
    let (db, lists, _) = build_db(pool, &[user, opp]);

    // Map CardId -> CardRef by replaying the deck-expansion order used by
    // build_db: lists[0].cards is user.cards expanded in order.
    let mut id_to_ref: std::collections::HashMap<CardId, mtg_engine::CardRef> =
        std::collections::HashMap::new();
    {
        let mut offset = 0usize;
        for (cid, count) in &user.cards {
            if *count > 0 {
                id_to_ref.insert(*cid, lists[0].cards[offset]);
                offset += *count as usize;
            }
        }
    }

    let hands = enumerate_hands(&user.cards, 7);
    progress
        .target
        .store((hands.len() as u64 * per_hand as u64).min(u32::MAX as u64) as u32, Ordering::Relaxed);

    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let outcomes: Vec<(HandOutcome, u32)> = hands
        .par_iter()
        .enumerate()
        .map(|(hi, hand)| {
            let forced: Vec<mtg_engine::CardRef> = hand
                .cards
                .iter()
                .flat_map(|(cid, n)| {
                    std::iter::repeat(id_to_ref.get(cid).copied()).take(*n as usize)
                })
                .flatten()
                .collect();
            let mut wins = 0u32;
            let mut draws = 0u32;
            let mut games = 0u32;
            let mut panics = 0u32;
            for g in 0..per_hand {
                let seed = game_seed(cfg.master_seed ^ 0x48414e44, hi as u64, g);
                let setup = GameSetup {
                    cfg: cfg.rules,
                    first: Some((g % 2) as u8),
                    trace: false,
                    forced_top: Some(forced.clone()),
                };
                let db = db.clone();
                let lists = lists.clone();
                let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    let mut agents = Agents {
                        seats: vec![Box::new(mtg_ai::GreedyAgent), Box::new(mtg_ai::GreedyAgent)],
                    };
                    mtg_engine::run_game(db, &lists, &setup, &mut agents, seed)
                }));
                match out {
                    Ok(o) => {
                        games += 1;
                        match o.end {
                            GameEnd::Winner(0) => wins += 1,
                            GameEnd::Winner(_) => {}
                            GameEnd::Draw => draws += 1,
                        }
                    }
                    Err(_) => panics += 1,
                }
            }
            progress.done.fetch_add(games, Ordering::Relaxed);
            progress.wins.fetch_add(wins, Ordering::Relaxed);
            (
                HandOutcome {
                    cards: hand.cards.clone(),
                    probability: hand.probability,
                    games,
                    wins,
                    draws,
                },
                panics,
            )
        })
        .collect();

    std::panic::set_hook(old_hook);

    let panics: u32 = outcomes.iter().map(|(_, p)| *p).sum();
    let hands: Vec<HandOutcome> = outcomes.into_iter().map(|(h, _)| h).collect();
    let total_games: u64 = hands.iter().map(|h| h.games as u64).sum();

    // Stratified estimate: weighted mean and its standard error.
    let mut wr = 0.0f64;
    let mut var = 0.0f64;
    for h in &hands {
        let p = h.probability;
        let w = h.win_rate();
        wr += p * w;
        if h.games > 0 {
            var += p * p * w * (1.0 - w) / h.games as f64;
        }
    }
    SweepStats {
        weighted_win_rate: wr,
        standard_error: var.sqrt(),
        total_games,
        distinct_hands: hands.len(),
        panics,
        hands,
    }
}
