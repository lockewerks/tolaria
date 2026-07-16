export interface PoolInfo {
  cards: number;
  updated_at: string;
  source: string;
}

export interface CardRow {
  name: string;
  count: number;
  mana_value: number;
  type_line: string;
  tier: string;
  dropped: string[];
}

export interface DeckInfo {
  name: string;
  total: number;
  rows: CardRow[];
  full: number;
  partial: number;
  proxy: number;
  unplayable: number;
  curve: number[];
  colors: string;
  unresolved: string[];
  commander: string | null;
}

export interface DeckFile {
  name: string;
  text: string;
}

export interface NameCount {
  name: string;
  count: number;
}

export interface MetaEntry {
  name: string;
  share: number;
  pilot_warning: boolean;
  playable: number;
  cards: NameCount[];
}

export interface MatchupStats {
  opponent: string;
  meta_share: number;
  games: number;
  wins: number;
  losses: number;
  draws: number;
  panics: number;
  on_play_wins: number;
  on_play_games: number;
  turns_sum: number;
  my_mulligans: number;
  stopped_early: boolean;
  opp_coverage_full_frac: number;
  opp_coverage_playable_frac: number;
  opp_pilot_warning: boolean;
}

export interface GauntletStats {
  deck_name: string;
  format: string;
  matchups: MatchupStats[];
}

export interface HandDto {
  cards: string;
  probability: number;
  games: number;
  win_rate: number;
}

export interface SweepDto {
  weighted_win_rate: number;
  standard_error: number;
  total_games: number;
  distinct_hands: number;
  panics: number;
  best: HandDto[];
  worst: HandDto[];
  histogram: number[];
}

export interface RunResult {
  kind: string;
  deck_name: string;
  format: string;
  when: number;
  elapsed: number;
  cancelled: boolean;
  deck_full: number;
  deck_playable: number;
  gauntlet: GauntletStats | null;
  sweep: SweepDto | null;
  pod: MatchupStats | null;
}

export interface RunMeta {
  file: string;
  deck_name: string;
  format: string;
  kind: string;
  when: number;
  headline: number;
  games: number;
}

export interface MatchupProg {
  name: string;
  done: number;
  target: number;
  wins: number;
  losses: number;
  draws: number;
  stopped: boolean;
}

export interface ProgressPayload {
  phase: string;
  status: string;
  matchups: MatchupProg[];
  games_per_sec: number;
  elapsed: number;
}

export interface RunConfig {
  mode: string;
  deck_text: string;
  deck_name: string;
  vs_text: string | null;
  format: string;
  games: string;
  precision: number;
  days: number;
  top: number;
  seed: number | null;
  early_stop: boolean;
  per_hand: number;
}

export function winRate(m: MatchupStats): number {
  if (m.games === 0) return 0.5;
  return (m.wins + m.draws * 0.5) / m.games;
}

export function wilson(wins: number, games: number): [number, number] {
  if (games <= 0) return [0, 1];
  const p = wins / games;
  const z = 1.96;
  const z2 = z * z;
  const denom = 1 + z2 / games;
  const center = (p + z2 / (2 * games)) / denom;
  const half = (z / denom) * Math.sqrt((p * (1 - p)) / games + z2 / (4 * games * games));
  return [Math.max(0, center - half), Math.min(1, center + half)];
}

export function ci95(m: MatchupStats): [number, number] {
  return wilson(m.wins + m.draws * 0.5, m.games);
}

export function weightedWinRate(g: GauntletStats): number {
  const total = g.matchups.reduce((a, m) => a + Math.max(0, m.meta_share), 0);
  if (total <= 0) {
    return g.matchups.reduce((a, m) => a + winRate(m), 0) / Math.max(1, g.matchups.length);
  }
  return g.matchups.reduce((a, m) => a + winRate(m) * Math.max(0, m.meta_share), 0) / total;
}

export const pct = (x: number, digits = 1) => `${(x * 100).toFixed(digits)}%`;
