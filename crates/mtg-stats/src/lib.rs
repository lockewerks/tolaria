//! Pure math: Wilson confidence intervals, sequential early stopping,
//! matchup aggregation, meta-share weighting.

use serde::{Deserialize, Serialize};

/// Wilson score interval for a binomial proportion.
pub fn wilson(wins: f64, games: f64, z: f64) -> (f64, f64) {
    if games <= 0.0 {
        return (0.0, 1.0);
    }
    let p = wins / games;
    let z2 = z * z;
    let denom = 1.0 + z2 / games;
    let center = (p + z2 / (2.0 * games)) / denom;
    let half = (z / denom) * ((p * (1.0 - p) / games) + z2 / (4.0 * games * games)).sqrt();
    ((center - half).max(0.0), (center + half).min(1.0))
}

/// Stop when the 95% CI excludes an even matchup, after a floor of games.
/// Draws count as half-wins so lopsided-but-drawish matchups still settle.
pub fn early_stop_decided(wins: u32, draws: u32, games: u32, floor: u32) -> bool {
    if games < floor {
        return false;
    }
    let effective_wins = wins as f64 + draws as f64 * 0.5;
    let (lo, hi) = wilson(effective_wins, games as f64, 1.96);
    lo > 0.5 || hi < 0.5
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatchupStats {
    pub opponent: String,
    pub meta_share: f64,
    pub games: u32,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub panics: u32,
    pub on_play_wins: u32,
    pub on_play_games: u32,
    pub turns_sum: u64,
    pub my_mulligans: u32,
    pub stopped_early: bool,
    /// 0 full .. 3 unplayable, worst tier fraction of the opponent list.
    pub opp_coverage_full_frac: f64,
    pub opp_pilot_warning: bool,
}

impl MatchupStats {
    pub fn win_rate(&self) -> f64 {
        if self.games == 0 {
            return 0.5;
        }
        (self.wins as f64 + self.draws as f64 * 0.5) / self.games as f64
    }

    pub fn ci95(&self) -> (f64, f64) {
        wilson(
            self.wins as f64 + self.draws as f64 * 0.5,
            self.games as f64,
            1.96,
        )
    }

    pub fn on_play_rate(&self) -> f64 {
        if self.on_play_games == 0 {
            return 0.5;
        }
        self.on_play_wins as f64 / self.on_play_games as f64
    }

    pub fn on_draw_rate(&self) -> f64 {
        let g = self.games - self.on_play_games;
        if g == 0 {
            return 0.5;
        }
        (self.wins - self.on_play_wins) as f64 / g as f64
    }

    pub fn avg_turns(&self) -> f64 {
        if self.games == 0 {
            return 0.0;
        }
        self.turns_sum as f64 / self.games as f64
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GauntletStats {
    pub deck_name: String,
    pub format: String,
    pub matchups: Vec<MatchupStats>,
}

impl GauntletStats {
    /// Meta-share-weighted expected win rate across the gauntlet.
    pub fn weighted_win_rate(&self) -> f64 {
        let total_share: f64 = self.matchups.iter().map(|m| m.meta_share.max(0.0)).sum();
        if total_share <= 0.0 {
            let n = self.matchups.len().max(1) as f64;
            return self.matchups.iter().map(|m| m.win_rate()).sum::<f64>() / n;
        }
        self.matchups
            .iter()
            .map(|m| m.win_rate() * m.meta_share.max(0.0))
            .sum::<f64>()
            / total_share
    }

    pub fn total_games(&self) -> u32 {
        self.matchups.iter().map(|m| m.games).sum()
    }

    /// Matchups sorted worst first.
    pub fn sorted_worst_first(&self) -> Vec<&MatchupStats> {
        let mut v: Vec<&MatchupStats> = self.matchups.iter().collect();
        v.sort_by(|a, b| a.win_rate().partial_cmp(&b.win_rate()).unwrap());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_sane() {
        let (lo, hi) = wilson(500.0, 1000.0, 1.96);
        assert!(lo > 0.45 && hi < 0.55);
        let (lo, _) = wilson(700.0, 1000.0, 1.96);
        assert!(lo > 0.5);
    }

    #[test]
    fn early_stop() {
        assert!(!early_stop_decided(60, 0, 100, 200));
        assert!(early_stop_decided(140, 0, 200, 200));
        assert!(!early_stop_decided(105, 0, 200, 200));
    }
}
