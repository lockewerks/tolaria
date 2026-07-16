//! M2/M3 checkpoint: two vanilla-creature decks play full, legal,
//! deterministic games headless, and damage decides a winner.

use std::sync::Arc;

use mtg_data::{Layout, Legalities, OracleCard, OracleFace, OracleId};
use mtg_engine::agent::Agents;
use mtg_engine::state::RulesConfig;
use mtg_engine::{CardDb, DeckList, GameEnd, GameSetup, NaiveAgent, PassAgent};
use mtg_ir::{parse_type_line, ColorSet};

fn face(name: &str, cost: &str, type_line: &str, text: &str, pt: Option<(i32, i32)>) -> OracleFace {
    let (types, supertypes, subtypes) = parse_type_line(type_line);
    OracleFace {
        name: name.into(),
        mana_cost: cost.into(),
        type_line: type_line.into(),
        oracle_text: text.into(),
        types,
        supertypes,
        subtypes,
        power: pt.map(|(p, _)| p),
        toughness: pt.map(|(_, t)| t),
        pt_star: false,
        loyalty: None,
        colors: if cost.contains('G') { ColorSet::G } else { ColorSet::empty() },
    }
}

fn card(
    idx: u8,
    name: &str,
    cost: &str,
    type_line: &str,
    text: &str,
    pt: Option<(i32, i32)>,
    produced: ColorSet,
) -> OracleCard {
    OracleCard {
        oracle_id: OracleId([idx; 16]),
        name: name.into(),
        layout: Layout::Normal,
        cmc: 2.0,
        color_identity: ColorSet::G,
        keywords: Vec::new(),
        legalities: Legalities::default(),
        produced_mana: produced,
        faces: vec![face(name, cost, type_line, text, pt)],
    }
}

fn bear_setup() -> (Arc<CardDb>, DeckList) {
    let forest = card(
        1,
        "Forest",
        "",
        "Basic Land \u{2014} Forest",
        "",
        None,
        ColorSet::G,
    );
    let bears = card(
        2,
        "Grizzly Bears",
        "{1}{G}",
        "Creature \u{2014} Bear",
        "",
        Some((2, 2)),
        ColorSet::empty(),
    );
    let mut db = CardDb::default();
    let f = db.add(forest.clone(), mtg_cards::compile(&forest));
    let b = db.add(bears.clone(), mtg_cards::compile(&bears));
    let mut cards = Vec::new();
    for _ in 0..24 {
        cards.push(f);
    }
    for _ in 0..36 {
        cards.push(b);
    }
    (Arc::new(db), DeckList { cards, commander: None })
}

fn run(seed: u64, a: Box<dyn mtg_engine::Agent>, b: Box<dyn mtg_engine::Agent>) -> mtg_engine::GameOutcome {
    let (db, deck) = bear_setup();
    let setup =
        GameSetup { cfg: RulesConfig::duel(), first: Some(0), trace: false, forced_top: None };
    let mut agents = Agents { seats: vec![a, b] };
    mtg_engine::run_game(db, &[deck.clone(), deck], &setup, &mut agents, seed)
}

#[test]
fn naive_beats_pass() {
    let out = run(42, Box::new(NaiveAgent), Box::new(PassAgent));
    assert_eq!(out.end, GameEnd::Winner(0), "attacking bears must beat a passing player");
    assert!(out.turns < 40, "game took {} turns", out.turns);
}

#[test]
fn mirror_finishes_with_a_winner() {
    let out = run(7, Box::new(NaiveAgent), Box::new(NaiveAgent));
    assert!(matches!(out.end, GameEnd::Winner(_)), "got {:?}", out.end);
}

#[test]
fn deterministic_per_seed() {
    let a = run(1234, Box::new(NaiveAgent), Box::new(NaiveAgent));
    let b = run(1234, Box::new(NaiveAgent), Box::new(NaiveAgent));
    assert_eq!(a.end, b.end);
    assert_eq!(a.turns, b.turns);
    assert_eq!(a.decisions, b.decisions);
    let c = run(1235, Box::new(NaiveAgent), Box::new(NaiveAgent));
    // Different seed usually differs somewhere; decisions is the most
    // sensitive probe. Not guaranteed, but 1235 vs 1234 differs in practice.
    assert!(a.decisions != c.decisions || a.turns != c.turns || a.end != c.end);
}

#[test]
fn pass_mirror_draws_at_turn_cap() {
    let out = run(9, Box::new(PassAgent), Box::new(PassAgent));
    assert_eq!(out.end, GameEnd::Draw);
}
