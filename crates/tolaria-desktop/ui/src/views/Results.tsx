import { useMemo, useState } from "react";
import {
  GauntletStats,
  MatchupStats,
  RunResult,
  ci95,
  pct,
  weightedWinRate,
  winRate,
} from "../types";
import { Panel, Stat } from "../components/bits";
import { ForestPlot, SplitBars, SweepHistogram, matchupTurns } from "../components/charts";

type SortKey = "wr" | "share" | "games" | "name" | "cov";

function MatchupTable({
  g,
  selected,
  setSelected,
}: {
  g: GauntletStats;
  selected: string | null;
  setSelected: (s: string | null) => void;
}) {
  const [sort, setSort] = useState<SortKey>("wr");
  const [asc, setAsc] = useState(true);
  const rows = useMemo(() => {
    const r = [...g.matchups];
    const key = (m: MatchupStats): number | string => {
      switch (sort) {
        case "wr":
          return winRate(m);
        case "share":
          return m.meta_share;
        case "games":
          return m.games;
        case "cov":
          return m.opp_coverage_playable_frac;
        default:
          return m.opponent.toLowerCase();
      }
    };
    r.sort((a, b) => {
      const ka = key(a);
      const kb = key(b);
      const cmp = typeof ka === "number" ? ka - (kb as number) : String(ka).localeCompare(String(kb));
      return asc ? cmp : -cmp;
    });
    return r;
  }, [g, sort, asc]);

  const header = (label: string, k: SortKey, num = true) => (
    <th
      className={num ? "num" : ""}
      onClick={() => {
        if (sort === k) setAsc(!asc);
        else {
          setSort(k);
          setAsc(true);
        }
      }}
    >
      {label}
      {sort === k ? (asc ? " ↑" : " ↓") : ""}
    </th>
  );

  return (
    <table>
      <thead>
        <tr>
          {header("matchup", "name", false)}
          {header("share", "share")}
          {header("games", "games")}
          {header("win rate", "wr")}
          <th className="num">95% ci</th>
          <th className="num">play</th>
          <th className="num">draw</th>
          {header("opp cov", "cov")}
          <th />
        </tr>
      </thead>
      <tbody>
        {rows.map((m) => {
          const [lo, hi] = ci95(m);
          const onPlay = m.on_play_games > 0 ? m.on_play_wins / m.on_play_games : 0.5;
          const drawGames = m.games - m.on_play_games;
          const onDraw = drawGames > 0 ? (m.wins - m.on_play_wins) / drawGames : 0.5;
          return (
            <tr
              key={m.opponent}
              className={`clickable${selected === m.opponent ? " selected" : ""}`}
              onClick={() => setSelected(selected === m.opponent ? null : m.opponent)}
            >
              <td style={{ color: "var(--ink)" }}>{m.opponent}</td>
              <td className="num">{pct(m.meta_share)}</td>
              <td className="num">{m.games.toLocaleString()}</td>
              <td className="num" style={{ color: lo > 0.5 ? "var(--ord-1)" : hi < 0.5 ? "var(--pole-down)" : "var(--ink-2)" }}>
                {pct(winRate(m))}
              </td>
              <td className="num">
                {pct(lo, 0)}..{pct(hi, 0)}
              </td>
              <td className="num">{pct(onPlay, 0)}</td>
              <td className="num">{pct(onDraw, 0)}</td>
              <td className="num">{pct(m.opp_coverage_playable_frac, 0)}</td>
              <td>
                {m.stopped_early ? <span className="badge">early</span> : null}{" "}
                {m.opp_pilot_warning ? <span className="badge warn">pilot</span> : null}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function Drill({ m }: { m: MatchupStats }) {
  const onPlay = m.on_play_games > 0 ? m.on_play_wins / m.on_play_games : 0.5;
  const drawGames = m.games - m.on_play_games;
  const onDraw = drawGames > 0 ? (m.wins - m.on_play_wins) / drawGames : 0.5;
  return (
    <Panel title={`drill-in: ${m.opponent}`}>
      <div className="row">
        <div>
          <SplitBars a={onPlay} b={onDraw} aLabel="on the play" bLabel="on the draw" />
        </div>
        <div className="grow">
          <div className="hint">
            record {m.wins.toLocaleString()}-{m.losses.toLocaleString()}
            {m.draws > 0 ? `-${m.draws}` : ""} over {m.games.toLocaleString()} games, average game{" "}
            {matchupTurns(m).toFixed(1)} turns, {m.my_mulligans} mulligans taken, {m.panics} panics
            {m.stopped_early ? "; stopped early once the verdict was statistically decided" : ""}.
          </div>
          <div className="hint">
            opponent list coverage: {pct(m.opp_coverage_playable_frac, 0)} playable (
            {pct(m.opp_coverage_full_frac, 0)} full)
            {m.opp_pilot_warning
              ? "; low pilot fidelity: a greedy agent cannot pilot this archetype's real lines"
              : ""}
          </div>
        </div>
      </div>
    </Panel>
  );
}

export function ResultsView({ result }: { result: RunResult | null }) {
  const [selected, setSelected] = useState<string | null>(null);
  if (!result) {
    return (
      <div>
        <h1>Results</h1>
        <Panel>
          <div className="hint">no run yet: configure one on the Run page</div>
        </Panel>
      </div>
    );
  }
  const g = result.gauntlet;
  const s = result.sweep;
  const p = result.pod;
  const sel = g?.matchups.find((m) => m.opponent === selected) ?? null;
  const avgCov = g
    ? g.matchups.reduce((a, m) => a + m.opp_coverage_playable_frac, 0) / Math.max(1, g.matchups.length)
    : 1;

  return (
    <div>
      <h1>
        Results: {result.deck_name}
        {result.cancelled ? "  (cancelled)" : ""}
      </h1>
      <div className="stats">
        {g ? (
          <>
            <Stat value={pct(weightedWinRate(g))} label={g.matchups.length > 1 ? "weighted vs the field" : "win rate"} />
            <Stat value={g.matchups.reduce((a, m) => a + m.games, 0).toLocaleString()} label="games" />
          </>
        ) : null}
        {s ? (
          <>
            <Stat value={pct(s.weighted_win_rate, 2)} label="hand-exact weighted win rate" />
            <Stat value={s.distinct_hands.toLocaleString()} label="distinct opening hands" />
            <Stat value={s.total_games.toLocaleString()} label="games" />
          </>
        ) : null}
        {p ? (
          <>
            <Stat value={pct(winRate(p))} label="pod seat win rate (baseline 25%)" />
            <Stat value={p.games.toLocaleString()} label="pods" />
          </>
        ) : null}
        <Stat value={`${result.elapsed.toFixed(1)}s`} label="wall clock" />
        <Stat value={pct(result.deck_playable, 0)} label="deck playable coverage" />
      </div>

      {g && avgCov < 0.85 ? (
        <div className="error">
          average opponent coverage is {pct(avgCov, 0)}: treat absolute win rates with care
        </div>
      ) : null}

      {g ? (
        <>
          <Panel title="matchups, worst first">
            <ForestPlot stats={g} onPick={(n) => setSelected(n)} selected={selected} />
          </Panel>
          {sel ? <Drill m={sel} /> : null}
          <Panel title="table">
            <MatchupTable g={g} selected={selected} setSelected={setSelected} />
          </Panel>
        </>
      ) : null}

      {s ? (
        <>
          <Panel title="opening hand distribution">
            <SweepHistogram histogram={s.histogram} />
            <div className="hint">
              standard error {pct(s.standard_error, 3)}; {s.panics} panics
            </div>
          </Panel>
          <div className="row">
            <Panel title="worst hands">
              <table>
                <thead>
                  <tr>
                    <th className="num">dealt</th>
                    <th className="num">wr</th>
                    <th>hand</th>
                  </tr>
                </thead>
                <tbody>
                  {s.worst.map((h, i) => (
                    <tr key={i}>
                      <td className="num">{pct(h.probability, 3)}</td>
                      <td className="num">{pct(h.win_rate, 0)}</td>
                      <td>{h.cards}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </Panel>
            <Panel title="best hands">
              <table>
                <thead>
                  <tr>
                    <th className="num">dealt</th>
                    <th className="num">wr</th>
                    <th>hand</th>
                  </tr>
                </thead>
                <tbody>
                  {s.best.map((h, i) => (
                    <tr key={i}>
                      <td className="num">{pct(h.probability, 3)}</td>
                      <td className="num">{pct(h.win_rate, 0)}</td>
                      <td>{h.cards}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </Panel>
          </div>
        </>
      ) : null}

      {p ? (
        <Panel title="pod detail">
          <div className="hint">
            {p.wins} wins, {p.losses} losses, {p.draws} draws over {p.games} four-player pods; average
            pod ran {matchupTurns(p).toFixed(1)} turns; opponents sampled from the EDHREC meta by share.
          </div>
        </Panel>
      ) : null}
    </div>
  );
}
