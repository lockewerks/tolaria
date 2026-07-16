import { RunMeta, pct } from "../types";
import { Panel } from "../components/bits";

export function HistoryView({
  runs,
  openRun,
}: {
  runs: RunMeta[];
  openRun: (file: string) => void;
}) {
  return (
    <div>
      <h1>History</h1>
      <Panel>
        {runs.length === 0 ? <div className="hint">no saved runs yet</div> : null}
        {runs.length > 0 ? (
          <table>
            <thead>
              <tr>
                <th>when</th>
                <th>deck</th>
                <th>kind</th>
                <th>format</th>
                <th className="num">headline</th>
                <th className="num">games</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((r) => (
                <tr key={r.file} className="clickable" onClick={() => openRun(r.file)}>
                  <td>{new Date(r.when * 1000).toLocaleString()}</td>
                  <td style={{ color: "var(--ink)" }}>{r.deck_name}</td>
                  <td>{r.kind}</td>
                  <td>{r.format}</td>
                  <td className="num">{pct(r.headline)}</td>
                  <td className="num">{r.games.toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : null}
      </Panel>
    </div>
  );
}
