export interface PoolInfo {
  cards: number;
  updated_at: string;
  source: string;
}

export interface Limit {
  id: string;
  category: string;
  rule_ref: string;
  summary: string;
  impact: string;
}

export interface TierCounts {
  full: number;
  partial: number;
  proxy: number;
  unplayable: number;
}

export interface DroppedCard {
  name: string;
  count: number;
  tier: string;
  clauses: string[];
}

export interface DeckTrust {
  name: string;
  tiers: TierCounts;
  coverage_full_frac: number;
  coverage_playable_frac: number;
  pilot_warning: boolean;
  pilot_grade?: number | null;
  dropped: DroppedCard[];
  list: [string, number][];
}

export interface RenderedWarning {
  code: string;
  severity: "info" | "caution" | "bias";
  text: string;
}

export interface TrustReport {
  schema_version: number;
  tolaria_version: string;
  compiler_version: number;
  ci_method: string;
  seed: number;
  gauntlet_seeded: boolean;
  user_deck: DeckTrust;
  opponents: DeckTrust[];
  early_stopped_matchups: number;
  panics: number;
  turn_cap_draws: number;
  decision_cap_draws: number;
  turn_cap: number;
  decision_cap: number;
  total_games: number;
  warnings: RenderedWarning[];
  calibration?: unknown;
}

export interface CardRow {
  name: string;
  count: number;
  mana_value: number;
  type_line: string;
  tier: string;
  dropped: string[];
}

export interface FormatFit {
  name: string;
  legal_frac: number;
  size_ok: boolean;
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
  lands: number;
  avg_mana_value: number;
  formats: FormatFit[];
  recommended: string;
  pilot_warning: boolean;
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

export interface MetaResponse {
  entries: MetaEntry[];
  archetypes_total: number;
  eligible: number;
  classified_decks: number;
  randomized: boolean;
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
  turn_hist: number[];
  win_reasons: number[];
  loss_reasons: number[];
  mull_hist: number[];
  win_life_sum: number;
  win_opp_life_sum: number;
  loss_life_sum: number;
  loss_opp_life_sum: number;
}

export const signed = (x: number, digits = 1) => `${x >= 0 ? "+" : ""}${x.toFixed(digits)}`;

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

export interface GoldfishStats {
  games: number;
  kills: number;
  no_kill: number;
  panics: number;
  kill_hist: number[];
  mull_hist: number[];
  avg_kill_turn: number;
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
  goldfish: GoldfishStats | null;
  seed: number;
  deck_pilot_warning?: boolean;
  trust?: TrustReport | null;
  deck_text?: string | null;
  vs_text?: string | null;
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
  selection: string;
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
