import { ReactNode } from "react";

export function Stat({ value, label }: { value: string; label: string }) {
  return (
    <div className="stat">
      <div className="v">{value}</div>
      <div className="k">{label}</div>
    </div>
  );
}

export function TierBadge({ tier }: { tier: string }) {
  return <span className={`badge ${tier.toLowerCase()}`}>{tier}</span>;
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
