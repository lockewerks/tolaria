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
