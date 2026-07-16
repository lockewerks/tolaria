import { GLOSSARY } from "../glossary";
import { Panel } from "../components/bits";

const GROUPS = ["Modes", "Setup", "Results", "Coverage"] as const;

export function GlossaryView() {
  return (
    <div>
      <h1>Glossary</h1>
      <Panel>
        <div className="hint">
          every term in the app, explained; the same text pops up anywhere you hover a dotted label
          for a second
        </div>
      </Panel>
      {GROUPS.map((group) => (
        <div key={group}>
          <h2 style={{ margin: "16px 0 8px" }}>{group}</h2>
          <div className="glossary-grid">
            {Object.entries(GLOSSARY)
              .filter(([, e]) => e.group === group)
              .sort(([, a], [, b]) => a.title.localeCompare(b.title))
              .map(([k, e]) => (
                <div key={k} className="glossary-card">
                  <b>{e.title}</b>
                  <p>{e.text}</p>
                </div>
              ))}
          </div>
        </div>
      ))}
    </div>
  );
}
