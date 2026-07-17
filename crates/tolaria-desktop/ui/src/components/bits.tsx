import { ReactNode, useState } from "react";
import { GLOSSARY } from "../glossary";
import type { RenderedWarning, TrustReport } from "../types";
import { pct, pilotGradeLabel } from "../types";

/** Dwell tooltip: hover for just over a second and the explanation pops. */
export function Tip({ k, children }: { k: string; children: ReactNode }) {
  const g = GLOSSARY[k];
  if (!g) return <>{children}</>;
  return (
    <span className="tip">
      {children}
      <span className="tip-pop">
        <b>{g.title}</b>
        {g.text}
      </span>
    </span>
  );
}

export function Stat({ value, label, tip }: { value: string; label: string; tip?: string }) {
  return (
    <div className="stat">
      <div className="v">{value}</div>
      <div className="k">{tip ? <Tip k={tip}>{label}</Tip> : label}</div>
    </div>
  );
}

export function TierBadge({ tier }: { tier: string }) {
  return (
    <Tip k={`tier-${tier.toLowerCase()}`}>
      <span className={`badge ${tier.toLowerCase()}`}>{tier}</span>
    </Tip>
  );
}

/** Every caveat a result raised, styled by severity, each tied to its glossary term. */
export function WarningList({ warnings }: { warnings: RenderedWarning[] }) {
  if (!warnings.length) return null;
  return (
    <div className="warnings">
      {warnings.map((w, i) => (
        <div key={i} className={w.severity === "info" ? "hint" : "error"}>
          <Tip k={`warn-${w.code}`}>{w.text}</Tip>
        </div>
      ))}
    </div>
  );
}

/** The collapsible reliability manifest that rides every result. */
export function TrustPanel({ trust }: { trust: TrustReport }) {
  const [open, setOpen] = useState(false);
  const u = trust.user_deck;
  const t = u.tiers;
  const capDraws = trust.turn_cap_draws + trust.decision_cap_draws;
  return (
    <div className="panel">
      <h2 style={{ cursor: "pointer" }} onClick={() => setOpen(!open)}>
        {open ? "▾" : "▸"} <Tip k="trust-report">trust report</Tip>
      </h2>
      {open ? (
        <div>
          <div className="trust-grid">
            <div>
              <b>{u.name}</b>: {pct(u.coverage_full_frac, 0)} full / {pct(u.coverage_playable_frac, 0)}{" "}
              playable ({t.full} full, {t.partial} partial, {t.proxy} proxy, {t.unplayable} unplayable)
              {u.pilot_grade != null ? (
                <>
                  {" · "}
                  <Tip k="pilot">pilot fidelity {pilotGradeLabel(u.pilot_grade)}</Tip>
                  {u.pilot_factors?.length ? ` (${u.pilot_factors.join(", ")})` : ""}
                </>
              ) : null}
            </div>
            {trust.opponents.length ? (
              <div>
                {trust.opponents.length} opponent{trust.opponents.length > 1 ? "s" : ""}, avg{" "}
                {pct(
                  trust.opponents.reduce((s, o) => s + o.coverage_playable_frac, 0) /
                    trust.opponents.length,
                  0,
                )}{" "}
                playable
              </div>
            ) : null}
            <div>
              seed {trust.seed} · {trust.total_games.toLocaleString()} games ·{" "}
              {trust.panics} panics · {capDraws} cap-forced draws · {trust.early_stopped_matchups}{" "}
              early-stopped
            </div>
            <div className="hint">
              {trust.ci_method} · compiler v{trust.compiler_version} · tolaria {trust.tolaria_version}
            </div>
          </div>
          {u.dropped.length ? (
            <div style={{ marginTop: 10 }}>
              <div className="hint">cards whose text was dropped or proxied:</div>
              {u.dropped.map((d, i) => (
                <details key={i} className="drop-row">
                  <summary>
                    {d.count > 1 ? `${d.count}x ` : ""}
                    {d.name} <span className={`badge ${d.tier.toLowerCase()}`}>{d.tier}</span>
                  </summary>
                  {d.clauses.map((c, j) => (
                    <div key={j} className="drop-clause">
                      dropped: {c}
                    </div>
                  ))}
                </details>
              ))}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

export function Panel({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <div className="panel">
      {title ? <h2>{title}</h2> : null}
      {children}
    </div>
  );
}

export function ProgressRow({
  name,
  done,
  target,
  wr,
  stopped,
}: {
  name: string;
  done: number;
  target: number;
  wr: number | null;
  stopped: boolean;
}) {
  const ratio = stopped ? 1 : target > 0 ? Math.min(1, done / target) : 0;
  return (
    <div className="prog-row">
      <div className="top">
        <span>
          {name}
          {stopped ? "  [decided]" : ""}
        </span>
        <span>
          {done.toLocaleString()} games{wr !== null ? `  ${(wr * 100).toFixed(1)}%` : ""}
        </span>
      </div>
      <div className="prog-track">
        <div className={`prog-fill${stopped ? " done" : ""}`} style={{ width: `${ratio * 100}%` }} />
      </div>
    </div>
  );
}
