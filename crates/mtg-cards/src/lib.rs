//! Card behavior compiler: oracle text to Effect IR, keyword short-circuit,
//! override registry, coverage grading, compiled cache.

pub mod compiler;
pub mod gaps;
pub mod templates;
pub mod text;

pub use compiler::{
    compile, compile_pool, compile_pool_detailed, CoverageStats, PoolCompilation,
    COMPILER_VERSION,
};

use mtg_ir::{Limit, LimitCategory::Cards};

/// What the coverage tiers do and do not promise.
pub const LIMITS: &[Limit] = &[
    Limit {
        id: "cards.tier.proxy",
        category: Cards,
        rule_ref: "-",
        summary: "Proxy-tier cards keep body, cost, and keywords but their rules text is not modeled",
        impact: "abilities on a Proxy card simply do not happen; a deck heavy in them plays weaker than it reads",
    },
    Limit {
        id: "cards.tier.partial",
        category: Cards,
        rule_ref: "-",
        summary: "Partial-tier cards model the main effect and drop listed rider clauses (disclosed per card)",
        impact: "the dropped rider never fires; usually minor, occasionally the point of the card",
    },
    Limit {
        id: "cards.tier.unplayable",
        category: Cards,
        rule_ref: "-",
        summary: "Unplayable-tier cards cannot be cast or resolved and sit dead in the deck",
        impact: "a slot that does nothing, so decks needing that card underperform",
    },
    Limit {
        id: "cards.compiler-version",
        category: Cards,
        rule_ref: "-",
        summary: "coverage reflects one compiler version; a newer version can regrade cards",
        impact: "numbers are only comparable within a compiler version, stamped on every result",
    },
];
