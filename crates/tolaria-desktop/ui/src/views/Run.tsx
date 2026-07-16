import { useState } from "react";
import { api } from "../api";
import { DeckFile, DeckInfo, ProgressPayload, RunConfig } from "../types";
import { Panel, ProgressRow } from "../components/bits";

const FORMATS = ["modern", "standard", "pioneer", "legacy", "vintage", "pauper", "commander"];

export function RunView({
  currentDeck,
  currentText,
  decks,
  running,
  progress,
  error,
}: {
  currentDeck: DeckInfo | null;
  currentText: string;
  decks: DeckFile[];
  running: boolean;
  progress: ProgressPayload | null;
  error: string;
}) {
  const [mode, setMode] = useState("gauntlet");
  const [format, setFormat] = useState("modern");
  const [games, setGames] = useState("1000");
  const [auto, setAuto] = useState(false);
  const [precision, setPrecision] = useState("1.0");
  const [days, setDays] = useState("60");
  const [top, setTop] = useState("12");
  const [seed, setSeed] = useState("");
  const [earlyStop, setEarlyStop] = useState(true);
  const [perHand, setPerHand] = useState("50");
  const [vsName, setVsName] = useState("");

  const needVs = mode === "duel" || mode === "sweep";
  const canStart = !!currentDeck && !running && (!needVs || !!vsName);

  const start = async () => {
    if (!currentDeck) return;
    const vs = decks.find((d) => d.name === vsName);
    const config: RunConfig = {
      mode,
      deck_text: currentText,
      deck_name: currentDeck.name,
      vs_text: needVs ? (vs?.text ?? null) : null,
      format,
      games: auto ? "auto" : games,
      precision: parseFloat(precision) || 1.0,
      days: parseInt(days) || 60,
      top: parseInt(top) || 12,
      seed: seed.trim() ? Number(seed.trim()) : null,
      early_stop: earlyStop,
      per_hand: parseInt(perHand) || 50,
    };
    await api.startRun(config);
  };

  return (
    <div>
      <h1>Run</h1>
      {!currentDeck ? (
        <Panel>
          <div className="hint">no deck loaded: import or pick one on the Decks page first</div>
        </Panel>
      ) : null}

      {running || progress ? (
        <Panel title={running ? "simulating" : "last run"}>
          {progress?.phase === "prep" ? <div className="status-line">{progress.status}</div> : null}
          {(progress?.matchups ?? []).map((m) => (
            <ProgressRow
              key={m.name}
              name={m.name}
              done={m.done}
              target={m.target}
              wr={m.wins + m.losses + m.draws > 0 ? (m.wins + m.draws * 0.5) / (m.wins + m.losses + m.draws) : null}
              stopped={m.stopped}
            />
          ))}
          {progress?.phase === "run" ? (
            <div className="status-line">
              {Math.round(progress.games_per_sec).toLocaleString()} games/s, {progress.elapsed.toFixed(1)}s
              elapsed
            </div>
          ) : null}
          {running ? (
            <button className="danger" onClick={() => void api.cancelRun()}>
              cancel
            </button>
          ) : null}
        </Panel>
      ) : null}
      {error ? <div className="error">{error}</div> : null}

      <Panel title={`configure${currentDeck ? `: ${currentDeck.name}` : ""}`}>
        <div className="row">
          <div className="grow">
            <label className="field">
              <span className="cap">mode</span>
              <select value={mode} onChange={(e) => setMode(e.target.value)}>
                <option value="gauntlet">gauntlet: versus the format meta</option>
                <option value="duel">duel: versus one saved deck</option>
                <option value="sweep">all hands: every opening seven versus one saved deck</option>
                <option value="pod">commander pods: four players versus the EDHREC meta</option>
              </select>
            </label>
            {mode === "gauntlet" ? (
              <label className="field">
                <span className="cap">format</span>
                <select value={format} onChange={(e) => setFormat(e.target.value)}>
                  {FORMATS.map((f) => (
                    <option key={f} value={f}>
                      {f}
                    </option>
                  ))}
                </select>
              </label>
            ) : null}
            {needVs ? (
              <label className="field">
                <span className="cap">opponent (saved deck)</span>
                <select value={vsName} onChange={(e) => setVsName(e.target.value)}>
                  <option value="">pick a deck</option>
                  {decks.map((d) => (
                    <option key={d.name} value={d.name}>
                      {d.name}
                    </option>
                  ))}
                </select>
              </label>
            ) : null}
            {mode === "sweep" ? (
              <label className="field">
                <span className="cap">continuations per hand</span>
                <input type="number" value={perHand} onChange={(e) => setPerHand(e.target.value)} />
              </label>
            ) : null}
          </div>
          <div className="grow">
            {mode !== "sweep" ? (
              <>
                <label className="check">
                  <input type="checkbox" checked={auto} onChange={(e) => setAuto(e.target.checked)} />
                  auto: play until the confidence interval is tight
                </label>
                {auto ? (
                  <label className="field">
                    <span className="cap">precision target (CI half-width, percentage points)</span>
                    <input type="text" value={precision} onChange={(e) => setPrecision(e.target.value)} />
                  </label>
                ) : (
                  <>
                    <label className="field">
                      <span className="cap">games per matchup (cap)</span>
                      <input type="text" value={games} onChange={(e) => setGames(e.target.value)} />
                    </label>
                    <label className="check">
                      <input
                        type="checkbox"
                        checked={earlyStop}
                        onChange={(e) => setEarlyStop(e.target.checked)}
                      />
                      stop a matchup early once the verdict is statistically decided
                    </label>
                  </>
                )}
              </>
            ) : (
              <div className="hint">
                sweeps enumerate every distinct opening hand, exactly weighted; game count is hands times
                continuations
              </div>
            )}
            {mode === "gauntlet" || mode === "pod" ? (
              <div className="row">
                <label className="field grow">
                  <span className="cap">window (days)</span>
                  <input type="text" value={days} onChange={(e) => setDays(e.target.value)} />
                </label>
                <label className="field grow">
                  <span className="cap">archetypes</span>
                  <input type="text" value={top} onChange={(e) => setTop(e.target.value)} />
                </label>
              </div>
            ) : null}
            <label className="field">
              <span className="cap">seed (blank = TOLARIA; same seed, same carnage)</span>
              <input type="text" value={seed} onChange={(e) => setSeed(e.target.value)} />
            </label>
          </div>
        </div>
        <button className="primary" disabled={!canStart} onClick={() => void start()}>
          {running ? "running..." : "start simulation"}
        </button>
      </Panel>
    </div>
  );
}
