import { ReactNode } from "react";
import { GLOSSARY } from "../glossary";

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
