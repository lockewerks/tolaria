//! Property-based fuzz over the real card pool: arbitrary 60-card decks pushed
//! through the engine to attack panics, non-determinism, and runaway games.
//! Skips cleanly when no local Scryfall cache is present (fresh checkout).

use std::collections::HashMap;
use std::sync::OnceLock;

use proptest::prelude::*;

use mtg_data::{CardId, CardPool};
use mtg_engine::RulesConfig;
use mtg_ir::Supertypes;
use mtg_sim::{run_matchup, MatchupProgress, SimConfig, SimDeck};

/// Pool plus a basic-land id for padding, loaded once and shared across every
/// proptest case. Loading per case would dominate runtime.
struct Fixture {
    pool: CardPool,
    forest: CardId,
}

fn fixture() -> Option<&'static Fixture> {
    static FIXTURE: OnceLock<Option<Fixture>> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let paths = mtg_data::Paths::resolve().ok()?;
            let opts = mtg_data::EnsureOptions { offline: true, ..Default::default() };
            let (pool, _) = mtg_data::ensure_pool(&paths, &opts).ok()?;
            // No basic means no cache worth fuzzing; treat as absent.
            let forest = pool.lookup("Forest")?;
            Some(Fixture { pool, forest })
        })
        .as_ref()
}

fn is_basic(pool: &CardPool, id: CardId) -> bool {
    pool.get(id).front().supertypes.contains(Supertypes::BASIC)
}

/// Fold arbitrary indices into a 60-card deck: non-basics capped at 4 copies,
/// padded to 60 with basic Forests. Legality is not the goal; running whatever
/// compiles is.
fn build_deck(fx: &Fixture, idxs: &[u32], name: &str) -> SimDeck {
    let n = fx.pool.len().max(1) as u32;
    let mut counts: HashMap<CardId, u8> = HashMap::new();
    let mut order: Vec<CardId> = Vec::new();
    let mut total: u32 = 0;

    for &raw in idxs {
        let cid = CardId(raw % n);
        let basic = is_basic(&fx.pool, cid);
        let c = counts.entry(cid).or_insert(0);
        if !basic && *c >= 4 {
            continue;
        }
        if *c == 0 {
            order.push(cid);
        }
        *c += 1;
        total += 1;
    }
    while total < 60 {
        let c = counts.entry(fx.forest).or_insert(0);
        if *c == 0 {
            order.push(fx.forest);
        }
        *c += 1;
        total += 1;
    }

    let cards: Vec<(CardId, u8)> = order.iter().map(|&c| (c, counts[&c])).collect();
    SimDeck {
        name: name.to_string(),
        cards,
        commander: None,
        meta_share: 1.0,
        pilot_warning: false,
    }
}

/// Non-basic contents, for pinpointing a culprit when a property fails.
fn interesting_names(fx: &Fixture, deck: &SimDeck) -> Vec<String> {
    deck.cards
        .iter()
        .filter(|(id, _)| !is_basic(&fx.pool, *id))
        .map(|(id, count)| format!("{count}x {}", fx.pool.get(*id).name))
        .collect()
}

/// One duel game, no early stop: the smallest unit that still exercises setup,
/// mulligans, the turn loop, and the turn cap.
fn one_game_cfg(seed: u64) -> SimConfig {
    SimConfig {
        games_cap: 1,
        floor: 1,
        early_stop: false,
        precision_target: None,
        cancel: None,
        master_seed: seed,
        rules: RulesConfig::duel(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// A completed game must never silently vanish. run_matchup catch_unwinds
    /// each game and decrements `games` while bumping `panics`, so a nonzero
    /// panics count is the tell that a real crash was swallowed.
    #[test]
    fn no_panic_on_arbitrary_decks(idxs in prop::collection::vec(any::<u32>(), 60)) {
        if let Some(fx) = fixture() {
            let deck = build_deck(fx, &idxs, "fuzz-user");
            let opp = deck.clone();
            let cfg = one_game_cfg(0xF0FA_1123_CAFE_0001);
            let progress = MatchupProgress::default();
            let stats = run_matchup(&fx.pool, &deck, &opp, &cfg, 0, &progress);
            prop_assert_eq!(
                stats.panics,
                0,
                "engine panicked; non-basics in deck: {:?}",
                interesting_names(fx, &deck)
            );
        }
    }

    /// Same deck, same seed, run twice: byte-identical observable stats.
    #[test]
    fn deterministic_replay(
        idxs in prop::collection::vec(any::<u32>(), 60),
        seed in any::<u64>(),
    ) {
        if let Some(fx) = fixture() {
            let deck = build_deck(fx, &idxs, "fuzz-user");
            let opp = deck.clone();
            // Two games so the per-game seed derivation is exercised, not just
            // a single fixed seed.
            let mut cfg = one_game_cfg(seed);
            cfg.games_cap = 2;
            let a = run_matchup(&fx.pool, &deck, &opp, &cfg, 7, &MatchupProgress::default());
            let b = run_matchup(&fx.pool, &deck, &opp, &cfg, 7, &MatchupProgress::default());
            prop_assert_eq!(
                (a.wins, a.losses, a.draws, a.games, a.turns_sum, a.my_mulligans),
                (b.wins, b.losses, b.draws, b.games, b.turns_sum, b.my_mulligans)
            );
        }
    }

    /// Under duel rules the engine's turn cap forces every game to end: exactly
    /// one game is recorded (no panic loss) and its length is finite and within
    /// the cap. `turn` increments before the cap check, so the ceiling is
    /// turn_cap + 1.
    #[test]
    fn termination_under_caps(idxs in prop::collection::vec(any::<u32>(), 60)) {
        if let Some(fx) = fixture() {
            let deck = build_deck(fx, &idxs, "fuzz-user");
            let opp = deck.clone();
            let cfg = one_game_cfg(0xF0FA_1123_CAFE_0003);
            let stats = run_matchup(&fx.pool, &deck, &opp, &cfg, 3, &MatchupProgress::default());
            prop_assert_eq!(stats.games, 1, "a finished game vanished (panic?)");
            let turns = stats.avg_turns();
            prop_assert!(turns.is_finite(), "avg_turns not finite: {turns}");
            let ceiling = RulesConfig::duel().turn_cap as f64 + 1.0;
            prop_assert!(turns <= ceiling, "turns {turns} exceeded cap ceiling {ceiling}");
        }
    }

    /// A player mulligans at most six times per game (the London floor of one
    /// card), so the aggregate stays within 6 per recorded game.
    #[test]
    fn mulligan_bounds(idxs in prop::collection::vec(any::<u32>(), 60)) {
        if let Some(fx) = fixture() {
            let deck = build_deck(fx, &idxs, "fuzz-user");
            let opp = deck.clone();
            let cfg = one_game_cfg(0xF0FA_1123_CAFE_0004);
            let stats = run_matchup(&fx.pool, &deck, &opp, &cfg, 5, &MatchupProgress::default());
            prop_assert!(
                stats.my_mulligans as u64 <= 6 * stats.games as u64,
                "my_mulligans {} over bound for {} games",
                stats.my_mulligans,
                stats.games
            );
        }
    }
}
