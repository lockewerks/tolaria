//! M4/M5/M6 checkpoints: targeted removal via the stack, combat keywords,
//! static anthems, and ETB triggers, driven by hand-built compiled cards.

use std::sync::Arc;

use mtg_data::{Layout, Legalities, OracleCard, OracleFace, OracleId};
use mtg_engine::actions::{apply_action, legal_actions, LegalAction};
use mtg_engine::agent::{Agents, PassAgent};
use mtg_engine::combat::{self, Defender};
use mtg_engine::state::{GameState, ObjectId, RulesConfig, Zone};
use mtg_engine::{Agent, CardDb, CardRef, DeckList, GameSetup};
use mtg_ir::{
    parse_type_line, AffectSpec, CardTypes, ColorSet, CompiledCard, CompiledFace, CoverageTier,
    Effect, KeywordSet, ManaCost, ObjFilter, PlayerSel, Recipient, SpellAbility, StaticAbility,
    TargetSpec, TargetWhat, TriggerCondition, TrigSubject, TriggeredAbility, ValueExpr, Whose,
};

fn oracle(idx: u8, name: &str, cost: &str, type_line: &str, pt: Option<(i32, i32)>) -> OracleCard {
    let (types, supertypes, subtypes) = parse_type_line(type_line);
    OracleCard {
        oracle_id: OracleId([idx; 16]),
        name: name.into(),
        layout: Layout::Normal,
        cmc: 1.0,
        color_identity: ColorSet::R,
        keywords: Vec::new(),
        legalities: Legalities::default(),
        produced_mana: if type_line.contains("Land") { ColorSet::R } else { ColorSet::empty() },
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
            colors: ColorSet::R,
        }],
    }
}

fn compiled(face: CompiledFace) -> CompiledCard {
    CompiledCard { tier: CoverageTier::Full, dropped: Vec::new(), faces: vec![face], compiler_version: 1 }
}

fn vanilla_face(cost: &str, kw: KeywordSet) -> CompiledFace {
    CompiledFace { cost: ManaCost::parse(cost), keywords: kw, ..Default::default() }
}

struct TestDb {
    db: CardDb,
}

impl TestDb {
    fn new() -> TestDb {
        TestDb { db: CardDb::default() }
    }

    fn add(&mut self, idx: u8, name: &str, cost: &str, tl: &str, pt: Option<(i32, i32)>, cf: CompiledFace) -> CardRef {
        self.db.add(oracle(idx, name, cost, tl, pt), compiled(cf))
    }

    fn game(self) -> GameState {
        let db = Arc::new(self.db);
        let decks = vec![
            DeckList { cards: vec![], commander: None },
            DeckList { cards: vec![], commander: None },
        ];
        let setup = GameSetup { cfg: RulesConfig::duel(), first: Some(0), trace: false };
        mtg_engine::new_game(db, &decks, &setup, 99)
    }
}

fn put_battlefield(gs: &mut GameState, seat: u8, card: CardRef) -> ObjectId {
    let id = gs.new_object(card, seat, Zone::Limbo, None);
    mtg_engine::zones::move_to(gs, id, Zone::Battlefield, Some(seat));
    gs.obj_mut(id).sick = false;
    mtg_engine::layers::recompute_chars(gs);
    id
}

fn put_hand(gs: &mut GameState, seat: u8, card: CardRef) -> ObjectId {
    gs.new_object(card, seat, Zone::Hand, None)
}

fn agents() -> Agents {
    Agents { seats: vec![Box::new(PassAgent), Box::new(PassAgent)] }
}

/// Blocks the first attacker with every candidate.
struct BlockAll;
impl Agent for BlockAll {
    fn declare_blockers(
        &mut self,
        _v: &mtg_engine::View,
        attackers: &[ObjectId],
        candidates: &[ObjectId],
    ) -> Vec<(ObjectId, ObjectId)> {
        candidates.iter().map(|&b| (b, attackers[0])).collect()
    }
}

/// Attacks with everything at the first defender.
struct AttackAll;
impl Agent for AttackAll {
    fn declare_attackers(
        &mut self,
        _v: &mtg_engine::View,
        candidates: &[ObjectId],
        defenders: &[Defender],
    ) -> Vec<(ObjectId, Defender)> {
        candidates.iter().map(|&c| (c, defenders[0])).collect()
    }
}

fn bolt_face() -> CompiledFace {
    CompiledFace {
        cost: ManaCost::parse("{R}"),
        spell: Some(SpellAbility {
            targets: vec![TargetSpec::one(TargetWhat::AnyDamageable)],
            effect: Effect::DealDamage { n: ValueExpr::Fixed(3), to: Recipient::Target(0) },
        }),
        ..Default::default()
    }
}

#[test]
fn removal_kills_via_stack_with_legal_targeting() {
    let mut t = TestDb::new();
    let mountain = t.add(1, "Mountain", "", "Basic Land \u{2014} Mountain", None, CompiledFace {
        mana_abilities: vec![mtg_ir::ManaAbility {
            cost: mtg_ir::AbilityCost::tap(),
            produce: mtg_ir::ManaProduction::AnyOneOf(ColorSet::R),
        }],
        ..Default::default()
    });
    let bolt = t.add(2, "Bolt", "{R}", "Instant", None, bolt_face());
    let bear = t.add(3, "Bear", "{1}{G}", "Creature \u{2014} Bear", Some((2, 2)), vanilla_face("{1}{G}", KeywordSet::empty()));

    let mut gs = t.game();
    let mut ag = agents();
    put_battlefield(&mut gs, 0, mountain);
    let bolt_id = put_hand(&mut gs, 0, bolt);
    let bear_id = put_battlefield(&mut gs, 1, bear);

    gs.step = mtg_engine::state::Step::Main1;
    gs.phase = mtg_engine::state::Phase::Main1;
    let legal = legal_actions(&gs, 0);
    let cast = legal
        .iter()
        .find(|a| matches!(a, LegalAction::Cast { card, .. } if *card == bolt_id))
        .expect("bolt must be castable")
        .clone();
    assert!(apply_action(&mut gs, &mut ag, 0, &cast));
    assert_eq!(gs.stack.len(), 1);
    // The default agent targeted the bear (first candidate).
    let item = gs.stack.pop().unwrap();
    mtg_engine::resolve::resolve(&mut gs, &mut ag, item);
    mtg_engine::sba::run_sba(&mut gs);
    assert_eq!(gs.obj(bear_id).zone, Zone::Graveyard, "bear should die to 3 damage");
    assert_eq!(gs.obj(bolt_id).zone, Zone::Graveyard, "bolt should be yarded");
}

#[test]
fn first_strike_kills_before_normal_damage() {
    let mut t = TestDb::new();
    let fs = t.add(1, "Fencer", "{1}{W}", "Creature \u{2014} Human", Some((2, 2)), vanilla_face("{1}{W}", KeywordSet::FIRST_STRIKE));
    let bear = t.add(2, "Bear", "{1}{G}", "Creature \u{2014} Bear", Some((2, 2)), vanilla_face("{1}{G}", KeywordSet::empty()));

    let mut gs = t.game();
    let mut ag = Agents { seats: vec![Box::new(AttackAll), Box::new(BlockAll)] };
    let fencer_id = put_battlefield(&mut gs, 0, fs);
    let bear_id = put_battlefield(&mut gs, 1, bear);

    assert!(combat::declare_attackers(&mut gs, &mut ag));
    combat::declare_blockers(&mut gs, &mut ag);
    assert!(combat::has_first_strike_step(&gs));
    combat::combat_damage(&mut gs, true);
    mtg_engine::sba::run_sba(&mut gs);
    assert_eq!(gs.obj(bear_id).zone, Zone::Graveyard, "bear dies to first strike");
    combat::combat_damage(&mut gs, false);
    mtg_engine::sba::run_sba(&mut gs);
    assert_eq!(gs.obj(fencer_id).zone, Zone::Battlefield, "fencer takes no damage back");
}

#[test]
fn trample_spills_over_and_deathtouch_needs_one() {
    let mut t = TestDb::new();
    let juggernaut = t.add(1, "Tusker", "{3}{G}", "Creature \u{2014} Beast", Some((4, 4)),
        vanilla_face("{3}{G}", KeywordSet::TRAMPLE | KeywordSet::DEATHTOUCH));
    let wall = t.add(2, "Wall", "{1}{W}", "Creature \u{2014} Wall", Some((0, 5)), vanilla_face("{1}{W}", KeywordSet::DEFENDER));

    let mut gs = t.game();
    let mut ag = Agents { seats: vec![Box::new(AttackAll), Box::new(BlockAll)] };
    put_battlefield(&mut gs, 0, juggernaut);
    let wall_id = put_battlefield(&mut gs, 1, wall);
    let life_before = gs.player(1).life;

    assert!(combat::declare_attackers(&mut gs, &mut ag));
    combat::declare_blockers(&mut gs, &mut ag);
    combat::combat_damage(&mut gs, false);
    mtg_engine::sba::run_sba(&mut gs);

    // Deathtouch means 1 damage is lethal to the wall; trample sends 3 over.
    assert_eq!(gs.obj(wall_id).zone, Zone::Graveyard, "deathtouch fells the wall");
    assert_eq!(gs.player(1).life, life_before - 3, "trample spills 3 past a lethal 1");
}

#[test]
fn anthem_pumps_and_etb_draws() {
    let mut t = TestDb::new();
    let bear = t.add(1, "Bear", "{1}{G}", "Creature \u{2014} Bear", Some((2, 2)), vanilla_face("{1}{G}", KeywordSet::empty()));
    let anthem = t.add(2, "Banner", "{1}{W}{W}", "Enchantment", None, CompiledFace {
        cost: ManaCost::parse("{1}{W}{W}"),
        statics: vec![StaticAbility::PtBuff {
            affects: AffectSpec {
                filter: ObjFilter {
                    types: CardTypes::CREATURE,
                    controller: Whose::You,
                    ..Default::default()
                },
                include_self: false,
            },
            p: 1,
            t: 1,
        }],
        ..Default::default()
    });
    let visionary = t.add(3, "Visionary", "{1}{U}", "Creature \u{2014} Elf", Some((1, 1)), CompiledFace {
        cost: ManaCost::parse("{1}{U}"),
        triggered: vec![TriggeredAbility {
            when: TriggerCondition::Etb(TrigSubject::This),
            ability: SpellAbility::untargeted(Effect::Draw { who: PlayerSel::You, n: ValueExpr::ONE }),
            once_per_turn: false,
        }],
        ..Default::default()
    });
    let filler = t.add(4, "Filler", "", "Basic Land \u{2014} Mountain", None, CompiledFace::default());

    let mut gs = t.game();
    let mut ag = agents();
    let bear_id = put_battlefield(&mut gs, 0, bear);
    assert_eq!(gs.obj(bear_id).chars.power, 2);
    put_battlefield(&mut gs, 0, anthem);
    assert_eq!(gs.obj(bear_id).chars.power, 3, "anthem grants +1/+1");
    assert_eq!(gs.obj(bear_id).chars.toughness, 3);

    // Seed a library card so the ETB draw has something to draw.
    let lib_card = gs.new_object(filler, 0, Zone::Library, None);
    let hand_before = gs.player(0).hand.len();
    put_battlefield(&mut gs, 0, visionary);
    // The ETB trigger is pending; flush and resolve it.
    mtg_engine::triggers::flush_triggers(&mut gs, &mut ag);
    let item = gs.stack.pop().expect("etb trigger on the stack");
    mtg_engine::resolve::resolve(&mut gs, &mut ag, item);
    assert_eq!(gs.player(0).hand.len(), hand_before + 1, "etb draw resolved");
    assert_eq!(gs.obj(lib_card).zone, Zone::Hand);
}

#[test]
fn lifelink_gains_and_menace_blocks() {
    let mut t = TestDb::new();
    let vampire = t.add(1, "Vamp", "{1}{B}", "Creature \u{2014} Vampire", Some((2, 2)),
        vanilla_face("{1}{B}", KeywordSet::LIFELINK | KeywordSet::MENACE));
    let bear = t.add(2, "Bear", "{1}{G}", "Creature \u{2014} Bear", Some((2, 2)), vanilla_face("{1}{G}", KeywordSet::empty()));

    let mut gs = t.game();
    let mut ag = Agents { seats: vec![Box::new(AttackAll), Box::new(BlockAll)] };
    put_battlefield(&mut gs, 0, vampire);
    // One bear cannot block a menace attacker alone.
    let bear_id = put_battlefield(&mut gs, 1, bear);
    let my_life = gs.player(0).life;
    let their_life = gs.player(1).life;

    assert!(combat::declare_attackers(&mut gs, &mut ag));
    combat::declare_blockers(&mut gs, &mut ag);
    combat::combat_damage(&mut gs, false);
    mtg_engine::sba::run_sba(&mut gs);

    assert_eq!(gs.obj(bear_id).zone, Zone::Battlefield, "solo menace block dropped");
    assert_eq!(gs.player(1).life, their_life - 2, "unblocked menace connects");
    assert_eq!(gs.player(0).life, my_life + 2, "lifelink gains");
}
