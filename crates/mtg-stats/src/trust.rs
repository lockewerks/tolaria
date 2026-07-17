//! The trust report: everything a result knows about its own reliability,
//! serialized so it rides every run and renders identically in the CLI,
//! the desktop, and exports. The construction lives in mtg-sim; these are
//! the pure data types and the single source of every warning's wording.

use serde::{Deserialize, Serialize};

/// Per-tier card counts (not fractions) for one deck.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TierCounts {
    pub full: u32,
    pub partial: u32,
    pub proxy: u32,
    pub unplayable: u32,
}

impl TierCounts {
    pub fn total(&self) -> u32 {
        self.full + self.partial + self.proxy + self.unplayable
    }
    pub fn playable(&self) -> u32 {
        self.full + self.partial
    }
}

/// One card whose text the compiler could not fully model, with the exact
/// clauses it dropped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DroppedCard {
    pub name: String,
    pub count: u8,
    pub tier: String,
    pub clauses: Vec<String>,
}

/// What is known about how faithfully one deck was simulated.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeckTrust {
    pub name: String,
    pub tiers: TierCounts,
    pub coverage_full_frac: f64,
    pub coverage_playable_frac: f64,
    pub pilot_warning: bool,
    /// 0 best .. 3 worst composite pilot-difficulty grade.
    #[serde(default)]
    pub pilot_grade: Option<u8>,
    /// Human-readable factors behind the grade, e.g. "9 tutors".
    #[serde(default)]
    pub pilot_factors: Vec<String>,
    /// Per-card dropped clauses. Populated for the user's deck; left empty
    /// for opponents (their detail is reachable via the Meta view).
    #[serde(default)]
    pub dropped: Vec<DroppedCard>,
    /// The exact list simulated (name, count), so a replay survives meta
    /// drift by rebuilding the same deck rather than refetching.
    #[serde(default)]
    pub list: Vec<(String, u8)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Context, no distortion implied.
    Info,
    /// A reason to read the number with care.
    Caution,
    /// The number is known to lean a specific direction.
    Bias,
}

/// Every caveat the app can raise, as data. The wording lives in one place
/// so the CLI, the UI, and exports never drift.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Warning {
    LowOppCoverage { avg_playable: f64 },
    LowOwnCoverage { playable: f64 },
    OwnPilotFidelity { creatures: u32 },
    OppPilotFidelity { archetypes: Vec<String> },
    EarlyStopped { matchups: u32 },
    Panics { games: u32 },
    CapForcedDraws { draws: u32, frac: f64 },
    ProxyHeavyOwnDeck { count: u32, frac: f64 },
}

impl Warning {
    pub fn severity(&self) -> Severity {
        match self {
            Warning::LowOppCoverage { .. }
            | Warning::LowOwnCoverage { .. }
            | Warning::OwnPilotFidelity { .. }
            | Warning::OppPilotFidelity { .. }
            | Warning::CapForcedDraws { .. }
            | Warning::ProxyHeavyOwnDeck { .. } => Severity::Bias,
            Warning::Panics { .. } => Severity::Caution,
            Warning::EarlyStopped { .. } => Severity::Info,
        }
    }

    /// The one and only English rendering. Each states the direction of
    /// bias, not just the fact.
    pub fn message(&self) -> String {
        match self {
            Warning::LowOppCoverage { avg_playable } => format!(
                "average opponent playable coverage is {:.0}%; unmodeled opponent cards are dead slots, so your win rate reads high",
                avg_playable * 100.0
            ),
            Warning::LowOwnCoverage { playable } => format!(
                "your deck is only {:.0}% playable coverage; your own dead cards drag this win rate below the real deck",
                playable * 100.0
            ),
            Warning::OwnPilotFidelity { creatures } => format!(
                "your deck has {creatures} creatures (under 10): the greedy pilot may not play its real lines, so this win rate leans low"
            ),
            Warning::OppPilotFidelity { archetypes } => format!(
                "{} opponent archetype(s) are low pilot fidelity ({}); they lose more than they should, so your win rate reads high",
                archetypes.len(),
                archetypes.join(", ")
            ),
            Warning::EarlyStopped { matchups } => format!(
                "{matchups} matchup(s) stopped early once the verdict was statistically settled"
            ),
            Warning::Panics { games } => format!(
                "{games} game(s) crashed the engine and were dropped from the sample; the win rate is over the survivors"
            ),
            Warning::CapForcedDraws { draws, frac } => format!(
                "{draws} game(s) ({:.1}%) hit the turn or decision cap and scored as draws; grindy or loop decks are undercounted",
                frac * 100.0
            ),
            Warning::ProxyHeavyOwnDeck { count, frac } => format!(
                "{count} card(s) ({:.0}%) of your deck are Proxy or Unplayable tier; those slots do less than the paper deck",
                frac * 100.0
            ),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Warning::LowOppCoverage { .. } => "low_opp_coverage",
            Warning::LowOwnCoverage { .. } => "low_own_coverage",
            Warning::OwnPilotFidelity { .. } => "own_pilot_fidelity",
            Warning::OppPilotFidelity { .. } => "opp_pilot_fidelity",
            Warning::EarlyStopped { .. } => "early_stopped",
            Warning::Panics { .. } => "panics",
            Warning::CapForcedDraws { .. } => "cap_forced_draws",
            Warning::ProxyHeavyOwnDeck { .. } => "proxy_heavy_own_deck",
        }
    }

    pub fn render(&self) -> RenderedWarning {
        RenderedWarning {
            code: self.code().to_string(),
            severity: self.severity(),
            text: self.message(),
        }
    }
}

/// A warning flattened for display: code (for a glossary tooltip),
/// severity (for styling), and the final text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedWarning {
    pub code: String,
    pub severity: Severity,
    pub text: String,
}

/// The manifest that rides every result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrustReport {
    pub schema_version: u32,
    pub tolaria_version: String,
    pub compiler_version: u16,
    pub ci_method: String,
    pub seed: u64,
    pub gauntlet_seeded: bool,
    pub user_deck: DeckTrust,
    pub opponents: Vec<DeckTrust>,
    pub early_stopped_matchups: u32,
    pub panics: u32,
    pub turn_cap_draws: u32,
    pub decision_cap_draws: u32,
    pub turn_cap: u32,
    pub decision_cap: u32,
    pub total_games: u32,
    pub warnings: Vec<RenderedWarning>,
    /// Filled from the latest calibration report for the format, when one
    /// exists. Opaque here so mtg-stats stays dependency-light.
    #[serde(default)]
    pub calibration: Option<serde_json::Value>,
}

pub const SCHEMA_VERSION: u32 = 1;
pub const CI_METHOD: &str = "wilson score interval, z=1.96, draws counted as half-wins";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_round_trips_tagged() {
        let w = Warning::CapForcedDraws { draws: 12, frac: 0.03 };
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("\"code\":\"cap_forced_draws\""));
        let back: Warning = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code(), "cap_forced_draws");
    }

    #[test]
    fn legacy_report_absent_calibration_loads() {
        // A report serialized before the calibration slot existed.
        let json = r#"{
            "schema_version": 1, "tolaria_version": "0.1.0", "compiler_version": 2,
            "ci_method": "x", "seed": 7, "gauntlet_seeded": true,
            "user_deck": {"name":"d","tiers":{"full":1,"partial":0,"proxy":0,"unplayable":0},
                "coverage_full_frac":1.0,"coverage_playable_frac":1.0,"pilot_warning":false},
            "opponents": [], "early_stopped_matchups": 0, "panics": 0,
            "turn_cap_draws": 0, "decision_cap_draws": 0, "turn_cap": 60, "decision_cap": 4000,
            "total_games": 100, "warnings": []
        }"#;
        let r: TrustReport = serde_json::from_str(json).unwrap();
        assert_eq!(r.seed, 7);
        assert!(r.calibration.is_none());
        assert_eq!(r.user_deck.pilot_grade, None);
    }
}
