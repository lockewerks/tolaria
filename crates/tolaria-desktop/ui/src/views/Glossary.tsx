import { useEffect, useState } from "react";
import { GLOSSARY } from "../glossary";
import { Panel } from "../components/bits";
import { api } from "../api";
import type { Limit } from "../types";

const GROUPS = ["Modes", "Setup", "Results", "Coverage"] as const;

export function GlossaryView() {
  const [limits, setLimits] = useState<Limit[]>([]);
  useEffect(() => {
    void api.listLimits().then(setLimits).catch(() => setLimits([]));
  }, []);

  const categories = [...new Set(limits.map((l) => l.category))];

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

      <h1 style={{ marginTop: 28 }}>What the simulator does not model</h1>
      <Panel>
        <div className="hint">
          every known divergence from real Magic, with the direction it pushes the numbers. an
          honest tool ships its own errata.
        </div>
      </Panel>
      {categories.map((cat) => (
        <div key={cat}>
          <h2 style={{ margin: "16px 0 8px" }}>{cat}</h2>
          <div className="glossary-grid">
            {limits
              .filter((l) => l.category === cat)
              .map((l) => (
                <div key={l.id} className="glossary-card">
                  <b>
                    {l.summary}
                    {l.rule_ref !== "-" ? <span className="hint"> [{l.rule_ref}]</span> : null}
                  </b>
                  <p>{l.impact}</p>
                </div>
              ))}
          </div>
        </div>
      ))}
    </div>
  );
}
