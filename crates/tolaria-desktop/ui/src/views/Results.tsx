import { useMemo, useState } from "react";
import {
  GauntletStats,
  MatchupStats,
  RunResult,
  ci95,
  pct,
  signed,
  weightedWinRate,
  winRate,
} from "../types";
import { Panel, Stat, Tip } from "../components/bits";
import {
  CountBars,
  ForestPlot,
  ReasonBars,
  SplitBars,
  SweepHistogram,
  matchupTurns,
} from "../components/charts";

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

  const header = (label: string, k: SortKey, tip: string | null, num = true) => (
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
      {tip ? <Tip k={tip}>{label}</Tip> : label}
      {sort === k ? (asc ? " ↑" : " ↓") : ""}
    </th>
  );

  return (
    <table>
      <thead>
        <tr>
          {header("matchup", "name", null, false)}
          {header("share", "share", "share")}
          {header("games", "games", "games-cap")}
          {header("win rate", "wr", "win-rate")}
          <th className="num">
            <Tip k="ci">95% ci</Tip>
          </th>
          <th className="num">
            <Tip k="on-play">play</Tip>
          </th>
          <th className="num">
            <Tip k="on-draw">draw</Tip>
          </th>
          {header("opp cov", "cov", "opp-cov")}
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
                {m.stopped_early ? (
                  <Tip k="early-stop">
                    <span className="badge">early</span>
                  </Tip>
                ) : null}{" "}
                {m.opp_pilot_warning ? (
                  <Tip k="pilot">
                    <span className="badge warn">pilot</span>
                  </Tip>
                ) : null}
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
  const mullHist = m.mull_hist ?? [];
  const keptSeven = mullHist[0] ?? 0;
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
            {matchupTurns(m).toFixed(1)} turns, {m.panics} panics
            {m.stopped_early ? "; stopped early once the verdict was statistically decided" : ""}.
          </div>
          <div className="hint">
            <Tip k="mulligan">mulligans</Tip>: kept 7 in {keptSeven.toLocaleString()} games
            {mullHist.length > 1
              ? `, one mull ${mullHist[1] ?? 0}, two ${mullHist[2] ?? 0}, three or more ${mullHist[3] ?? 0}`
              : ""}
          </div>
          {m.wins > 0 || m.losses > 0 ? (
            <div className="hint">
              <Tip k="margin">margins</Tip>:{" "}
              {m.wins > 0
                ? `your wins end with you at ${signed(m.win_life_sum / m.wins)} life, them at ${signed(m.win_opp_life_sum / m.wins)}`
                : ""}
              {m.wins > 0 && m.losses > 0 ? "; " : ""}
              {m.losses > 0
                ? `your losses end with you at ${signed(m.loss_life_sum / m.losses)}, them at ${signed(m.loss_opp_life_sum / m.losses)}`
                : ""}{" "}
              (<Tip k="overkill">negative = past dead</Tip>)
            </div>
          ) : null}
          <div className="hint">
            opponent list coverage: {pct(m.opp_coverage_playable_frac, 0)} playable (
            {pct(m.opp_coverage_full_frac, 0)} full)
            {m.opp_pilot_warning
              ? "; low pilot fidelity: a greedy agent cannot pilot this archetype's real lines"
              : ""}
          </div>
        </div>
      </div>
      {(m.turn_hist ?? []).some((v) => v > 0) ? (
        <div className="row" style={{ marginTop: 10 }}>
          <div>
            <h2>
              <Tip k="turn-hist">game length (total turns)</Tip>
            </h2>
            <CountBars values={m.turn_hist} labelEvery={5} ariaLabel="Game length distribution" />
          </div>
          <div>
            <h2>
              <Tip k="end-reasons">end reasons</Tip>
            </h2>
            <ReasonBars wins={m.win_reasons ?? []} losses={m.loss_reasons ?? []} />
          </div>
        </div>
      ) : null}
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
  const gf = result.goldfish;
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
            <Stat
              value={pct(weightedWinRate(g))}
              label={g.matchups.length > 1 ? "weighted vs the field" : "win rate"}
              tip={g.matchups.length > 1 ? "weighted" : "win-rate"}
            />
            <Stat value={g.matchups.reduce((a, m) => a + m.games, 0).toLocaleString()} label="games" />
            {(() => {
              const wins = g.matchups.reduce((a, m) => a + m.wins, 0);
              const winLife = g.matchups.reduce((a, m) => a + (m.win_life_sum ?? 0), 0);
              const oppLife = g.matchups.reduce((a, m) => a + (m.win_opp_life_sum ?? 0), 0);
              return wins > 0 ? (
                <>
                  <Stat value={`${signed(winLife / wins)} life`} label="you win at" tip="margin" />
                  <Stat value={`${signed(oppLife / wins)} life`} label="they end at" tip="overkill" />
                </>
              ) : null;
            })()}
          </>
        ) : null}
        {s ? (
          <>
            <Stat value={pct(s.weighted_win_rate, 2)} label="hand-exact weighted win rate" tip="mode-sweep" />
            <Stat value={s.distinct_hands.toLocaleString()} label="distinct opening hands" tip="dealt-prob" />
            <Stat value={s.total_games.toLocaleString()} label="games" />
          </>
        ) : null}
        {p ? (
          <>
            <Stat value={pct(winRate(p))} label="pod seat win rate (baseline 25%)" tip="mode-pod" />
            <Stat value={p.games.toLocaleString()} label="pods" />
          </>
        ) : null}
        {gf ? (
          <>
            <Stat
              value={gf.avg_kill_turn > 0 ? gf.avg_kill_turn.toFixed(2) : "none"}
              label="average kill turn"
              tip="kill-turn"
            />
            <Stat
              value={pct(gf.games > 0 ? gf.kills / gf.games : 0, 1)}
              label="games with a kill"
              tip="mode-goldfish"
            />
            <Stat value={gf.games.toLocaleString()} label="games" />
          </>
        ) : null}
        <Stat value={`${result.elapsed.toFixed(1)}s`} label="wall clock" />
        <Stat value={pct(result.deck_playable, 0)} label="deck playable coverage" tip="playable" />
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
            {p.wins > 0 ? ` Your wins end at ${signed(p.win_life_sum / p.wins)} life;` : ""}
            {p.losses > 0 ? ` your losses end at ${signed(p.loss_life_sum / p.losses)}.` : ""}
          </div>
        </Panel>
      ) : null}

      {gf ? (
        <>
          <Panel title="kill turn distribution">
            <CountBars
              values={gf.kill_hist}
              labelEvery={2}
              ariaLabel="Kills by your turn number"
            />
            <div className="hint">
              killed by turn 4: {pct(kilBy(gf, 4), 1)}, turn 5: {pct(kilBy(gf, 5), 1)}, turn 6:{" "}
              {pct(kilBy(gf, 6), 1)}, turn 8: {pct(kilBy(gf, 8), 1)}; {gf.no_kill} games never got
              there before the turn cap; {gf.panics} panics
            </div>
          </Panel>
          <Panel title="consistency">
            <div className="hint">
              <Tip k="mulligan">mulligans</Tip>: kept 7 in {gf.mull_hist[0] ?? 0} games, one mull{" "}
              {gf.mull_hist[1] ?? 0}, two {gf.mull_hist[2] ?? 0}, three or more {gf.mull_hist[3] ?? 0}.
              Remember this is a goldfish: zero interaction, so real games land a turn or two later.
            </div>
          </Panel>
        </>
      ) : null}
    </div>
  );
}

function kilBy(gf: { kill_hist: number[]; games: number }, turn: number): number {
  if (gf.games === 0) return 0;
  const upto = gf.kill_hist.slice(0, turn).reduce((a, b) => a + b, 0);
  return upto / gf.games;
}
