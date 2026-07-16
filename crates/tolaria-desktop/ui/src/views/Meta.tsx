import { useState } from "react";
import { api, onMetaProgress } from "../api";
import { MetaEntry, MetaResponse, pct } from "../types";
import { Panel, Tip } from "../components/bits";

const FORMATS = ["modern", "standard", "pioneer", "legacy", "vintage", "pauper", "commander"];

export function MetaView() {
  const [format, setFormat] = useState("modern");
  const [days, setDays] = useState("60");
  const [size, setSize] = useState("top-12");
  const [entries, setEntries] = useState<MetaEntry[]>([]);
  const [universe, setUniverse] = useState<MetaResponse | null>(null);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<MetaEntry | null>(null);

  const fetchIt = async () => {
    setBusy(true);
    setStatus("starting...");
    setSelected(null);
    const un = await onMetaProgress(setStatus);
    try {
      const [selKind, selCount] = size.split("-");
      const m = await api.fetchMeta(
        format,
        parseInt(days) || 60,
        parseInt(selCount ?? "12") || 12,
        selKind,
      );
      setEntries(m.entries);
      setUniverse(m);
      setStatus("");
    } catch (e) {
      setStatus(String(e));
    } finally {
      un();
      setBusy(false);
    }
  };

  return (
    <div>
      <h1>Meta</h1>
      <Panel>
        <div className="row" style={{ alignItems: "flex-end" }}>
          <label className="field grow">
            <span className="cap">format</span>
            <select value={format} onChange={(e) => setFormat(e.target.value)}>
              {FORMATS.map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
            </select>
          </label>
          <label className="field grow">
            <span className="cap">window (days)</span>
            <input type="text" value={days} onChange={(e) => setDays(e.target.value)} />
          </label>
          <label className="field grow">
            <span className="cap">
              <Tip k="archetypes">gauntlet size</Tip>
            </span>
            <select value={size} onChange={(e) => setSize(e.target.value)}>
              <option value="top-12">top 12</option>
              <option value="top-24">top 24</option>
              <option value="random-12">random 12</option>
              <option value="random-24">random 24</option>
              <option value="all">all eligible</option>
            </select>
          </label>
          <label className="field">
            <span className="cap">&nbsp;</span>
            <button className="primary" disabled={busy} onClick={() => void fetchIt()}>
              {busy ? "fetching..." : "fetch meta"}
            </button>
          </label>
        </div>
        <div className="status-line">
          {status ||
            (universe
              ? `universe: ${universe.archetypes_total} archetypes seen in the window, ` +
                `${universe.eligible} eligible (3+ lists), ${universe.classified_decks.toLocaleString()} decks classified; ` +
                `showing ${entries.length}${universe.randomized ? ", randomly drawn" : ""}`
              : "")}
        </div>
      </Panel>
      <div className="row">
        <div className="grow">
          <Panel title="archetypes by tournament share">
            <table>
              <thead>
                <tr>
                  <th>archetype</th>
                  <th className="num">share</th>
                  <th className="num">coverage</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {entries.map((m) => (
                  <tr
                    key={m.name}
                    className={`clickable${selected?.name === m.name ? " selected" : ""}`}
                    onClick={() => setSelected(m)}
                  >
                    <td style={{ color: "var(--ink)" }}>{m.name}</td>
                    <td className="num">{pct(m.share)}</td>
                    <td className="num">{pct(m.playable, 0)}</td>
                    <td>{m.pilot_warning ? <span className="badge warn">pilot</span> : null}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {entries.length === 0 ? <div className="hint">fetch a format to see its meta</div> : null}
          </Panel>
        </div>
        {selected ? (
          <div style={{ width: 320, flexShrink: 0 }}>
            <Panel title={selected.name}>
              <div className="scroll-panel">
                <table>
                  <tbody>
                    {selected.cards.map((c) => (
                      <tr key={c.name}>
                        <td className="num" style={{ width: 30 }}>
                          {c.count}
                        </td>
                        <td>{c.name}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </Panel>
          </div>
        ) : null}
      </div>
    </div>
  );
}
