//! Engine-level property tests over hand-built cards. Invariants that must
//! hold for any legal-ish deck of vanilla creatures, basic lands, and a
//! damage spell, checked with proptest across random compositions. No card
//! pool or Scryfall cache is touched: every card is constructed inline, so
//! these run offline and fast.

use std::sync::Arc;

use mtg_data::{Layout, Legalities, OracleCard, OracleFace, OracleId};
use mtg_engine::{
    Agent, Agents, CardDb, DeckList, GameEnd, GameOutcome, GameSetup, NaiveAgent, ObjectId,
    PassAgent, RulesConfig, View,
};
use mtg_ir::{
    parse_type_line, AbilityCost, ColorSet, CompiledCard, CompiledFace, CoverageTier, Effect,
    ManaAbility, ManaCost, ManaProduction, Recipient, SpellAbility, TargetSpec, TargetWhat,
    ValueExpr,
};
use proptest::prelude::*;

// Card construction, mirroring the helpers in vanilla.rs and mechanics.rs.
// Tests cannot share code across files, so the pattern is copied here.

fn oracle(idx: u8, name: &str, cost: &str, type_line: &str, pt: Option<(i32, i32)>) -> OracleCard {
    let (types, supertypes, subtypes) = parse_type_line(type_line);
    let is_land = type_line.contains("Land");
    OracleCard {
        oracle_id: OracleId([idx; 16]),
        name: name.into(),
        layout: Layout::Normal,
        cmc: 2.0,
        color_identity: ColorSet::G,
        keywords: Vec::new(),
        legalities: Legalities::default(),
        produced_mana: if is_land { ColorSet::G } else { ColorSet::empty() },
        faces: vec![OracleFace {
            name: name.into(),
            mana_cost: cost.into(),
            type_line: type_line.into(),
            oracle_text: "".into(),
            types,
            supertypes,
            subtypes,
            power: pt.map(|(p, _)| p),
            toughness: pt.map(|(_, t)| t),
            pt_star: false,
            loyalty: None,
            colors: if cost.contains('G') { ColorSet::G } else { ColorSet::empty() },
        }],
    }
}

fn compiled(face: CompiledFace) -> CompiledCard {
    CompiledCard { tier: CoverageTier::Full, dropped: Vec::new(), faces: vec![face], compiler_version: 1 }
}

/// A basic land that taps for one green.
fn land_face() -> CompiledFace {
    CompiledFace {
        mana_abilities: vec![ManaAbility {
            cost: AbilityCost::tap(),
            produce: ManaProduction::AnyOneOf(ColorSet::G),
        }],
        ..Default::default()
    }
}

/// A vanilla creature: cost only, no abilities. P/T live on the oracle face.
fn creature_face(cost: &str) -> CompiledFace {
    CompiledFace { cost: ManaCost::parse(cost), ..Default::default() }
}

/// A bolt-like instant: deal `dmg` to any one damageable target. No lifegain.
fn bolt_face(dmg: i32) -> CompiledFace {
    CompiledFace {
        cost: ManaCost::parse("{G}"),
        spell: Some(SpellAbility {
            targets: vec![TargetSpec::one(TargetWhat::AnyDamageable)],
            effect: Effect::DealDamage { n: ValueExpr::Fixed(dmg), to: Recipient::Target(0) },
        }),
        ..Default::default()
    }
}

#[derive(Debug, Clone)]
struct DeckSpec {
    lands: u32,
    spells: u32,
    creatures: u32,
    /// (power, toughness) per creature kind; the creature slots are dealt
    /// across these kinds round-robin.
    kinds: Vec<(i32, i32)>,
    bolt_dmg: i32,
    seed: u64,
}

fn build(spec: &DeckSpec) -> (Arc<CardDb>, DeckList) {
    let mut db = CardDb::default();
    let forest = db.add(oracle(1, "Forest", "", "Basic Land \u{2014} Forest", None), compiled(land_face()));
    let bolt = db.add(oracle(2, "Jab", "{G}", "Instant", None), compiled(bolt_face(spec.bolt_dmg)));
    let creatures: Vec<_> = spec
        .kinds
        .iter()
        .enumerate()
        .map(|(i, &(p, t))| {
            let name = format!("Beast{i}");
            db.add(
                oracle((10 + i) as u8, &name, "{1}{G}", "Creature \u{2014} Beast", Some((p, t))),
                compiled(creature_face("{1}{G}")),
            )
        })
        .collect();

    let mut cards = Vec::new();
    for _ in 0..spec.lands {
        cards.push(forest);
    }
    for _ in 0..spec.spells {
        cards.push(bolt);
    }
    for n in 0..spec.creatures as usize {
        cards.push(creatures[n % creatures.len()]);
    }
    (Arc::new(db), DeckList { cards, commander: None })
}

/// Total is pinned to 40..=60 by construction: lands and spells are capped
/// first, then creatures take the remainder (always >= 10).
fn deck_spec() -> impl Strategy<Value = DeckSpec> {
    (
        40u32..=60,
        16u32..=24,
        0u32..=6,
        prop::collection::vec((1i32..=5, 1i32..=5), 1..=4),
        2i32..=4,
        any::<u64>(),
    )
        .prop_map(|(total, lands, spells, kinds, bolt_dmg, seed)| {
            let lands = lands.min(total);
            let spells = spells.min(total - lands);
            let creatures = total - lands - spells;
            DeckSpec { lands, spells, creatures, kinds, bolt_dmg, seed }
        })
}

fn run(
    db: Arc<CardDb>,
    deck: &DeckList,
    cfg: RulesConfig,
    seed: u64,
    a: Box<dyn Agent>,
    b: Box<dyn Agent>,
) -> GameOutcome {
    let setup = GameSetup { cfg, first: Some(0), trace: false, forced_top: None };
    let mut agents = Agents { seats: vec![a, b] };
    mtg_engine::run_game(db, &[deck.clone(), deck.clone()], &setup, &mut agents, seed)
}

/// Always takes a mulligan, so the London loop climbs to its ceiling.
struct AlwaysMulligan;
impl Agent for AlwaysMulligan {
    fn mulligan(&mut self, _v: &View, _hand: &[ObjectId], _taken: u8) -> bool {
        true
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// No card in this pool gains life (no lifelink, no GainLife effect), and
    /// damage only ever subtracts. So no seat can end above its starting life.
    #[test]
    fn life_conservation_under_damage_only(spec in deck_spec()) {
        let cfg = RulesConfig::duel();
        let (db, deck) = build(&spec);
        let out = run(db, &deck, cfg, spec.seed, Box::new(NaiveAgent), Box::new(NaiveAgent));
        for (s, &life) in out.life.iter().enumerate() {
            prop_assert!(
                life <= cfg.starting_life,
                "seat {s} ended at {life} life, above starting {}",
                cfg.starting_life
            );
        }
    }

    /// London mulligans are bounded: the loop stops offering at six.
    #[test]
    fn mulligan_bounds(spec in deck_spec()) {
        let cfg = RulesConfig::duel();
        let (db, deck) = build(&spec);
        let out = run(
            db,
            &deck,
            cfg,
            spec.seed,
            Box::new(AlwaysMulligan),
            Box::new(AlwaysMulligan),
        );
        for (s, &m) in out.mulligans.iter().enumerate() {
            prop_assert!(m <= 6, "seat {s} took {m} mulligans");
        }
    }

    /// Same seed, same decks, same agents reproduces the game byte for byte.
    #[test]
    fn deterministic_replay(spec in deck_spec()) {
        let cfg = RulesConfig::duel();
        let (db, deck) = build(&spec);
        let a = run(db.clone(), &deck, cfg, spec.seed, Box::new(NaiveAgent), Box::new(NaiveAgent));
        let b = run(db, &deck, cfg, spec.seed, Box::new(NaiveAgent), Box::new(NaiveAgent));
        prop_assert_eq!(a.end, b.end);
        prop_assert_eq!(a.turns, b.turns);
        prop_assert_eq!(a.decisions, b.decisions);
        prop_assert_eq!(&a.life[..], &b.life[..]);
        prop_assert_eq!(&a.mulligans[..], &b.mulligans[..]);
    }

    /// run_game increments the turn counter before testing it against the cap,
    /// so the reported turn count tops out at turn_cap + 1. A passive mirror
    /// plays no lands and never attacks, so it provably rides to the cap and
    /// pins that off-by-one exactly.
    #[test]
    fn turn_cap_respected(spec in deck_spec()) {
        let mut cfg = RulesConfig::duel();
        cfg.turn_cap = 30;
        let (db, deck) = build(&spec);
        let naive = run(db.clone(), &deck, cfg, spec.seed, Box::new(NaiveAgent), Box::new(NaiveAgent));
        prop_assert!(
            naive.turns <= cfg.turn_cap + 1,
            "naive game ran {} turns, cap {}",
            naive.turns,
            cfg.turn_cap
        );
        let passive = run(db, &deck, cfg, spec.seed, Box::new(PassAgent), Box::new(PassAgent));
        prop_assert_eq!(passive.turns, cfg.turn_cap + 1);
        prop_assert_eq!(passive.end, GameEnd::Draw);
    }
}
