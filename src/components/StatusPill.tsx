import type { JSX } from "react";
import type { Pill } from "../lib/pill";

export function StatusPill({ pill, onShowFailure }: { pill: Pill; onShowFailure: (id: number) => void }): JSX.Element {
  const cls = `pill pill-${pill.kind} pill-tone-${pill.tone}`;
  const id = pill.snapshotId;
  if (id !== undefined && pill.outcome !== "ok") {
    return (
      <button type="button" className={cls} title={pill.tooltip} onClick={() => onShowFailure(id)}>
        {pill.label}
      </button>
    );
  }
  return <span className={cls} title={pill.tooltip}>{pill.label}</span>;
}
