//! The divergence ledger's shared shape. Each crate declares what it does
//! not model in a `LIMITS` slice next to the code responsible, so the list
//! and the behavior drift together or not at all.

/// Where a limitation lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitCategory {
    /// The rules engine diverges from the comprehensive rules.
    Rules,
    /// The card compiler's coverage semantics.
    Cards,
    /// The built-in pilot's decision quality.
    Pilot,
    /// How the numbers are computed and reported.
    Statistics,
    /// How the opposing metagame is constructed.
    Meta,
}

impl LimitCategory {
    pub fn label(self) -> &'static str {
        match self {
            LimitCategory::Rules => "Rules",
            LimitCategory::Cards => "Cards",
            LimitCategory::Pilot => "Pilot",
            LimitCategory::Statistics => "Statistics",
            LimitCategory::Meta => "Meta",
        }
    }
}

/// One honest admission: a thing the simulator does not model, and which
/// way that pushes the numbers.
#[derive(Debug, Clone, Copy)]
pub struct Limit {
    /// Stable dotted id, e.g. "engine.layers.dependencies".
    pub id: &'static str,
    pub category: LimitCategory,
    /// Comprehensive-rules reference, or "-" when none applies.
    pub rule_ref: &'static str,
    /// One line: what is not modeled.
    pub summary: &'static str,
    /// Who it biases and in which direction.
    pub impact: &'static str,
}
