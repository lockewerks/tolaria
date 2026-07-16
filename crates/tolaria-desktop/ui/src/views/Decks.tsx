import { useRef, useState } from "react";
import { api } from "../api";
import { DeckFile, DeckInfo, pct } from "../types";
import { Panel, TierBadge } from "../components/bits";
import { CoverageDonut, CurveChart } from "../components/charts";

export function DecksView({
  decks,
  refreshDecks,
  current,
  setCurrent,
  currentText,
  setCurrentText,
  goRun,
}: {
  decks: DeckFile[];
  refreshDecks: () => void;
  current: DeckInfo | null;
  setCurrent: (d: DeckInfo | null) => void;
  currentText: string;
  setCurrentText: (t: string) => void;
  goRun: () => void;
}) {
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const analyze = async (text: string, deckName: string) => {
    setBusy(true);
    setErr("");
    try {
      const info = await api.parseDeck(text, deckName || "deck");
      setCurrent(info);
      setCurrentText(text);
      if (!name) setName(info.name);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onFile = (f: File | null) => {
    if (!f) return;
    const reader = new FileReader();
    reader.onload = () => {
      const text = String(reader.result ?? "");
      setCurrentText(text);
      const base = f.name.replace(/\.[^.]+$/, "");
      setName(base);
      void analyze(text, base);
    };
    reader.readAsText(f);
  };

  const save = async () => {
    if (!name.trim() || !currentText.trim()) return;
    await api.saveDeck(name.trim(), currentText);
    refreshDecks();
  };

  return (
    <div>
      <h1>Decks</h1>
      <div className="row">
        <div style={{ width: 250, flexShrink: 0 }}>
          <Panel title="Saved decks">
            {decks.length === 0 ? <div className="hint">nothing saved yet</div> : null}
            {decks.map((d) => (
              <div
                key={d.name}
                className={`deck-list-item${current?.name === d.name ? " active" : ""}`}
                onClick={() => {
                  setName(d.name);
                  void analyze(d.text, d.name);
                }}
              >
                <span>{d.name}</span>
                <button
                  title="delete"
                  style={{ padding: "1px 7px" }}
                  onClick={(e) => {
                    e.stopPropagation();
                    void api.deleteDeck(d.name).then(refreshDecks);
                  }}
                >
                  x
                </button>
              </div>
            ))}
          </Panel>
          <Panel title="Import">
            <label className="field">
              <span className="cap">deck name</span>
              <input type="text" value={name} onChange={(e) => setName(e.target.value)} />
            </label>
            <label className="field">
              <span className="cap">paste a decklist (MTGA, MTGO, or plain)</span>
              <textarea
                value={currentText}
                onChange={(e) => setCurrentText(e.target.value)}
                placeholder={"4 Lightning Bolt\n20 Mountain"}
              />
            </label>
            <div className="row">
              <button onClick={() => fileRef.current?.click()}>open file</button>
              <button
                className="primary"
                disabled={busy || !currentText.trim()}
                onClick={() => void analyze(currentText, name)}
              >
                {busy ? "analyzing..." : "analyze"}
              </button>
              <button disabled={!current || !name.trim()} onClick={() => void save()}>
                save
              </button>
            </div>
            <input
              ref={fileRef}
              type="file"
              accept=".txt,.dek,.dec"
              style={{ display: "none" }}
              onChange={(e) => onFile(e.target.files?.[0] ?? null)}
            />
            {err ? <div className="error">{err}</div> : null}
          </Panel>
        </div>

        <div className="grow">
          {current ? (
            <>
              <Panel title={`${current.name}  (${current.total} cards${current.colors ? `, ${current.colors}` : ""}${current.commander ? `, commander: ${current.commander}` : ""})`}>
                <div className="row">
                  <CoverageDonut
                    full={current.full}
                    partial={current.partial}
                    proxy={current.proxy}
                    unplayable={current.unplayable}
                  />
                  <div className="grow">
                    <h2>Mana curve (nonland)</h2>
                    <CurveChart curve={current.curve} />
                  </div>
                </div>
                {current.unresolved.length > 0 ? (
                  <div className="error">
                    unresolved: {current.unresolved.slice(0, 4).join("; ")}
                    {current.unresolved.length > 4 ? ` and ${current.unresolved.length - 4} more` : ""}
                  </div>
                ) : null}
                <div className="row" style={{ marginTop: 8 }}>
                  <button className="primary" onClick={goRun}>
                    run this deck
                  </button>
                </div>
              </Panel>
              <Panel title="Cards and coverage">
                <div className="scroll-panel">
                  <table>
                    <thead>
                      <tr>
                        <th className="num">#</th>
                        <th>card</th>
                        <th className="num">mv</th>
                        <th>type</th>
                        <th>tier</th>
                      </tr>
                    </thead>
                    <tbody>
                      {current.rows.map((r) => (
                        <>
                          <tr
                            key={r.name}
                            className={r.dropped.length > 0 ? "clickable" : ""}
                            onClick={() =>
                              r.dropped.length > 0 && setExpanded(expanded === r.name ? null : r.name)
                            }
                          >
                            <td className="num">{r.count}</td>
                            <td style={{ color: "var(--ink)" }}>{r.name}</td>
                            <td className="num">{r.mana_value}</td>
                            <td>{r.type_line}</td>
                            <td>
                              <TierBadge tier={r.tier} />
                              {r.dropped.length > 0 ? (
                                <span className="hint"> {expanded === r.name ? "▾" : "▸"}</span>
                              ) : null}
                            </td>
                          </tr>
                          {expanded === r.name
                            ? r.dropped.map((d, i) => (
                                <tr key={`${r.name}-d${i}`}>
                                  <td />
                                  <td colSpan={4} className="drop-text">
                                    dropped: {d}
                                  </td>
                                </tr>
                              ))
                            : null}
                        </>
                      ))}
                    </tbody>
                  </table>
                </div>
                <div className="hint">
                  playable {pct((current.full + current.partial) / Math.max(1, current.total), 0)}; click a
                  row with a caret to see exactly which clauses the compiler dropped
                </div>
              </Panel>
            </>
          ) : (
            <Panel>
              <div className="hint">
                import or pick a deck to see its coverage, curve, and per-card compilation detail
              </div>
            </Panel>
          )}
        </div>
      </div>
    </div>
  );
}
