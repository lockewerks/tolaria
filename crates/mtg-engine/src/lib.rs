//! Rules engine: game state, turn structure, priority, the stack, mana,
//! combat, state-based actions, layers, triggers, replacements.
//! Executes Effect IR; never parses oracle text.

pub mod actions;
pub mod agent;
pub mod carddb;
pub mod combat;
pub mod events;
pub mod filters;
pub mod game;
pub mod layers;
pub mod mana_pay;
pub mod resolve;
pub mod sba;
pub mod state;
pub mod triggers;
pub mod turn;
pub mod view;
pub mod zones;

pub use agent::{Agent, Agents, NaiveAgent, PassAgent};
pub use carddb::{CardDb, CardRef, GameCard};
pub use game::{new_game, run_game, DeckList, GameOutcome, GameSetup};
pub use state::{GameEnd, GameState, LossReason, ObjectId, RulesConfig, Seat};
pub use view::View;

use mtg_ir::{Limit, LimitCategory::Rules};

/// What the rules engine does not model. Kept next to the engine so a
/// change to the behavior and a change to the confession travel together.
pub const LIMITS: &[Limit] = &[
    Limit {
        id: "engine.layers.dependencies",
        category: Rules,
        rule_ref: "CR 613.8",
        summary: "static abilities read base characteristics; layer dependency ordering is not resolved",
        impact: "rare wrong power/toughness or type when one continuous effect depends on another",
    },
    Limit {
        id: "engine.ward.untargetable",
        category: Rules,
        rule_ref: "CR 702.113",
        summary: "ward is modeled as untargetable by opponents rather than as an additional cost or tax",
        impact: "warded permanents are harder to interact with than on paper; the ward cost is never paid",
    },
    Limit {
        id: "engine.triggers.whitelist",
        category: Rules,
        rule_ref: "CR 603",
        summary: "only a fixed set of ~13 trigger conditions fire; others are inert",
        impact: "cards keyed on unlisted events (storm count, prowess-likes, cast-your-Nth) underperform",
    },
    Limit {
        id: "engine.replacements.minimal",
        category: Rules,
        rule_ref: "CR 614",
        summary: "only doubling, damage prevention, and dies-to-exile replacement effects exist",
        impact: "other replacement effects are ignored, distorting engines built on them",
    },
    Limit {
        id: "engine.protection.color-only",
        category: Rules,
        rule_ref: "CR 702.16",
        summary: "protection is modeled from colors only, not from qualities like artifacts or a card name",
        impact: "non-color protection has no effect in combat or targeting",
    },
    Limit {
        id: "engine.combat.menace-solo-block",
        category: Rules,
        rule_ref: "CR 509.1c",
        summary: "a single legal blocker against a menace creature is silently dropped rather than declared illegal",
        impact: "menace is slightly stronger than it should be in lone-blocker spots",
    },
    Limit {
        id: "engine.caps.forced-draw",
        category: Rules,
        rule_ref: "-",
        summary: "games hitting the turn cap (60 duel, 100 commander) or the decision cap are scored as draws",
        impact: "grindy control mirrors and combo loops are undercounted as draws rather than resolved",
    },
    Limit {
        id: "engine.game-one-only",
        category: Rules,
        rule_ref: "CR 100.4",
        summary: "every match is game one; sideboards are parsed but never brought in",
        impact: "post-board configurations, hate cards, and transformational plans are absent",
    },
];
