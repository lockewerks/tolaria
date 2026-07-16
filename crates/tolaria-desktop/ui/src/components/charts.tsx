// SVG charts following the reference dataviz palette: thin marks, rounded
// data ends, recessive grid, diverging poles only for polarity, ordinal
// single-hue ramp for tiers, text in ink tokens.

import { GauntletStats, MatchupStats, ci95, pct, winRate } from "../types";

const BLUE = "#3987e5";
const RED = "#e66767";
const NEUTRAL = "#898781";
// Validated ordinal blue ramp, uniform 150-step spacing, dark surface.
const ORDINAL = ["#b7d3f6", "#6da7ec", "#2a78d6", "#184f95"];

/** Win rate per matchup with CI whiskers around the 50% line. */
export function ForestPlot({
  stats,
  onPick,
  selected,
}: {
  stats: GauntletStats;
  onPick?: (name: string) => void;
  selected?: string | null;
}) {
  const rows = [...stats.matchups].sort((a, b) => winRate(a) - winRate(b));
  const rowH = 26;
  const left = 210;
  const right = 60;
  const width = 760;
  const plotW = width - left - right;
  const height = rows.length * rowH + 34;
  const x = (v: number) => left + v * plotW;

  return (
    <div className="chart-wrap">
      <svg width={width} height={height} role="img" aria-label="Matchup win rates with 95% confidence intervals">
        {[0, 0.25, 0.5, 0.75, 1].map((v) => (
          <g key={v}>
            <line
              x1={x(v)}
              y1={8}
              x2={x(v)}
              y2={height - 26}
              className={v === 0.5 ? "base-line" : "grid-line"}
            />
            <text x={x(v)} y={height - 12} textAnchor="middle" className="axis-label">
              {Math.round(v * 100)}%
            </text>
          </g>
        ))}
        {rows.map((m, i) => {
          const y = 20 + i * rowH;
          const wr = winRate(m);
          const [lo, hi] = ci95(m);
          const color = lo > 0.5 ? BLUE : hi < 0.5 ? RED : NEUTRAL;
          const isSel = selected === m.opponent;
          return (
            <g
              key={m.opponent}
              onClick={() => onPick?.(m.opponent)}
              style={{ cursor: onPick ? "pointer" : "default" }}
            >
              <title>
                {`${m.opponent}: ${pct(wr)} (CI ${pct(lo)}..${pct(hi)}), ${m.games} games`}
              </title>
              <text x={left - 10} y={y + 4} textAnchor="end" fontSize={11.5} opacity={isSel ? 1 : 0.85}>
                {m.opponent.length > 30 ? m.opponent.slice(0, 29) + "…" : m.opponent}
              </text>
              <line x1={x(lo)} y1={y} x2={x(hi)} y2={y} stroke={color} strokeWidth={2} />
              <line x1={x(lo)} y1={y - 4} x2={x(lo)} y2={y + 4} stroke={color} strokeWidth={2} />
              <line x1={x(hi)} y1={y - 4} x2={x(hi)} y2={y + 4} stroke={color} strokeWidth={2} />
              <circle cx={x(wr)} cy={y} r={isSel ? 5.5 : 4.5} fill={color} stroke="#1a1a19" strokeWidth={2} />
              <text x={x(hi) + 8} y={y + 4} fontSize={11} className="axis-label">
                {pct(wr)}
              </text>
            </g>
          );
        })}
      </svg>
      <div className="legend">
        <span>
          <span className="sw" style={{ background: BLUE }} />
          favorable (CI above 50%)
        </span>
        <span>
          <span className="sw" style={{ background: NEUTRAL }} />
          undecided
        </span>
        <span>
          <span className="sw" style={{ background: RED }} />
          unfavorable (CI below 50%)
        </span>
      </div>
    </div>
  );
}

/** Nonland mana curve: single-series bars. */
export function CurveChart({ curve }: { curve: number[] }) {
  const width = 300;
  const height = 130;
  const max = Math.max(1, ...curve);
  const barW = 26;
  const gap = 8;
  const left = 8;
  return (
    <svg width={width} height={height} role="img" aria-label="Mana curve">
      <line x1={left} y1={height - 22} x2={width - 8} y2={height - 22} className="base-line" />
      {curve.map((n, i) => {
        const h = n === 0 ? 0 : Math.max(3, (n / max) * (height - 48));
        const bx = left + 4 + i * (barW + gap);
        const by = height - 22 - h;
        return (
          <g key={i}>
            <title>{`mana value ${i === 7 ? "7+" : i}: ${n} cards`}</title>
            {n > 0 ? <rect x={bx} y={by} width={barW} height={h} rx={4} fill={BLUE} /> : null}
            {n > 0 ? (
              <text x={bx + barW / 2} y={by - 5} textAnchor="middle" className="axis-label">
                {n}
              </text>
            ) : null}
            <text x={bx + barW / 2} y={height - 8} textAnchor="middle" className="axis-label">
              {i === 7 ? "7+" : i}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

/** Coverage donut: ordinal single-hue ramp, brightest = Full. */
export function CoverageDonut({
  full,
  partial,
  proxy,
  unplayable,
}: {
  full: number;
  partial: number;
  proxy: number;
  unplayable: number;
}) {
  const parts = [
    { label: "Full", value: full, color: ORDINAL[0] },
    { label: "Partial", value: partial, color: ORDINAL[1] },
    { label: "Proxy", value: proxy, color: ORDINAL[2] },
    { label: "Unplayable", value: unplayable, color: ORDINAL[3] },
  ];
  const total = Math.max(1, full + partial + proxy + unplayable);
  const cx = 62;
  const cy = 62;
  const r = 46;
  let angle = -Math.PI / 2;
  const arcs = parts
    .filter((p) => p.value > 0)
    .map((p) => {
      const span = (p.value / total) * Math.PI * 2;
      const a0 = angle;
      const a1 = angle + span;
      angle = a1;
      const large = span > Math.PI ? 1 : 0;
      const p0 = [cx + r * Math.cos(a0), cy + r * Math.sin(a0)];
      const p1 = [cx + r * Math.cos(a1), cy + r * Math.sin(a1)];
      return { ...p, d: `M ${p0[0]} ${p0[1]} A ${r} ${r} 0 ${large} 1 ${p1[0]} ${p1[1]}` };
    });
  return (
    <div className="row" style={{ alignItems: "center" }}>
      <svg width={124} height={124} role="img" aria-label="Deck coverage by tier">
        {arcs.map((a) => (
          <g key={a.label}>
            <title>{`${a.label}: ${a.value} cards (${pct(a.value / total, 0)})`}</title>
            <path d={a.d} fill="none" stroke={a.color} strokeWidth={16} />
          </g>
        ))}
        {/* 2px surface gaps between segments via overdrawn separators */}
        <circle cx={cx} cy={cy} r={r - 10} fill="none" />
        <text x={cx} y={cy + 1} textAnchor="middle" fontSize={16} fontWeight={650} fill="#ffffff">
          {pct((full + partial) / total, 0)}
        </text>
        <text x={cx} y={cy + 15} textAnchor="middle" className="axis-label">
          playable
        </text>
      </svg>
      <div className="legend" style={{ flexDirection: "column", gap: 5 }}>
        {parts.map((p) => (
          <span key={p.label}>
            <span className="sw" style={{ background: p.color }} />
            {p.label}: {p.value}
          </span>
        ))}
      </div>
    </div>
  );
}

/** Sweep histogram: probability mass across win-rate buckets. */
export function SweepHistogram({ histogram }: { histogram: number[] }) {
  const width = 560;
  const height = 160;
  const left = 34;
  const bottom = 24;
  const max = Math.max(0.0001, ...histogram);
  const n = histogram.length;
  const plotW = width - left - 12;
  const barW = plotW / n - 2;
  return (
    <div className="chart-wrap">
      <svg width={width} height={height} role="img" aria-label="Opening hand win rate distribution">
        <line x1={left} y1={height - bottom} x2={width - 8} y2={height - bottom} className="base-line" />
        {[0, 25, 50, 75, 100].map((v) => (
          <text
            key={v}
            x={left + (v / 100) * plotW}
            y={height - 8}
            textAnchor="middle"
            className="axis-label"
          >
            {v}%
          </text>
        ))}
        {histogram.map((p, i) => {
          const h = p === 0 ? 0 : Math.max(2, (p / max) * (height - bottom - 18));
          const bx = left + (i / n) * plotW + 1;
          const by = height - bottom - h;
          return (
            <g key={i}>
              <title>{`${i * 5}-${i * 5 + 5}% win rate: ${(p * 100).toFixed(2)}% of hands`}</title>
              {p > 0 ? <rect x={bx} y={by} width={barW} height={h} rx={3} fill={BLUE} /> : null}
            </g>
          );
        })}
      </svg>
      <div className="hint">probability-weighted share of opening hands by win rate</div>
    </div>
  );
}

/** Simple two-value comparison bars for play/draw splits. */
export function SplitBars({ a, b, aLabel, bLabel }: { a: number; b: number; aLabel: string; bLabel: string }) {
  const rows = [
    { label: aLabel, v: a },
    { label: bLabel, v: b },
  ];
  return (
    <svg width={320} height={64} role="img" aria-label="Play versus draw win rate">
      {rows.map((r, i) => {
        const y = 8 + i * 28;
        return (
          <g key={r.label}>
            <title>{`${r.label}: ${pct(r.v)}`}</title>
            <text x={0} y={y + 10} fontSize={11} className="axis-label">
              {r.label}
            </text>
            <rect x={70} y={y} width={Math.max(2, r.v * 180)} height={13} rx={4} fill={BLUE} />
            <text x={70 + Math.max(2, r.v * 180) + 7} y={y + 10.5} fontSize={11.5}>
              {pct(r.v)}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

export function matchupTurns(m: MatchupStats): number {
  return m.games > 0 ? m.turns_sum / m.games : 0;
}

/** Generic count histogram with sparse x labels. */
export function CountBars({
  values,
  labelEvery,
  labelOffset = 1,
  width = 560,
  ariaLabel,
}: {
  values: number[];
  labelEvery: number;
  labelOffset?: number;
  width?: number;
  ariaLabel: string;
}) {
  const height = 140;
  const left = 10;
  const bottom = 22;
  const max = Math.max(1, ...values);
  const n = Math.max(1, values.length);
  const plotW = width - left - 8;
  const barW = Math.max(2, plotW / n - 2);
  return (
    <div className="chart-wrap">
      <svg width={width} height={height} role="img" aria-label={ariaLabel}>
        <line x1={left} y1={height - bottom} x2={width - 4} y2={height - bottom} className="base-line" />
        {values.map((v, i) => {
          const h = v === 0 ? 0 : Math.max(2, (v / max) * (height - bottom - 16));
          const bx = left + (i / n) * plotW + 1;
          return (
            <g key={i}>
              <title>{`${i + labelOffset}: ${v.toLocaleString()} games`}</title>
              {v > 0 ? (
                <rect x={bx} y={height - bottom - h} width={barW} height={h} rx={2} fill={BLUE} />
              ) : null}
              {(i + labelOffset) % labelEvery === 0 ? (
                <text x={bx + barW / 2} y={height - 7} textAnchor="middle" className="axis-label">
                  {i + labelOffset}
                </text>
              ) : null}
            </g>
          );
        })}
      </svg>
    </div>
  );
}

const REASON_LABELS = ["damage", "poison", "decked", "commander", "other"];

/** Paired end-reason breakdown for wins and losses. */
export function ReasonBars({ wins, losses }: { wins: number[]; losses: number[] }) {
  const totalW = Math.max(1, wins.reduce((a, b) => a + b, 0));
  const totalL = Math.max(1, losses.reduce((a, b) => a + b, 0));
  return (
    <table style={{ maxWidth: 460 }}>
      <thead>
        <tr>
          <th>ended by</th>
          <th className="num">your wins</th>
          <th className="num">your losses</th>
        </tr>
      </thead>
      <tbody>
        {REASON_LABELS.map((label, i) => {
          const w = wins[i] ?? 0;
          const l = losses[i] ?? 0;
          if (w === 0 && l === 0) return null;
          return (
            <tr key={label}>
              <td>{label}</td>
              <td className="num">
                {w.toLocaleString()} ({((w / totalW) * 100).toFixed(0)}%)
              </td>
              <td className="num">
                {l.toLocaleString()} ({((l / totalL) * 100).toFixed(0)}%)
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
