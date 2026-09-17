import type { JSX } from "react";
import { systemLine } from "../lib/system";
import type { SystemReport } from "../lib/types";
import { Ring } from "./Ring";

interface Props {
  system: SystemReport | null;
  error: string | null;
  showCount: boolean;
  now: number;
}

/**
 * The second header line: what the Claude Code processes cost. Every
 * decision — which items, and whether the line is dimmed — is made by the
 * pure `systemLine`; this component only maps the result to markup.
 */
export function SystemLine({ system, error, showCount, now }: Props): JSX.Element {
  const { items, dimmed } = systemLine({ report: system, error, showCount, now });
  return (
    <div
      className={`sysline${dimmed ? " sysline-stale" : ""}`}
      role="group"
      aria-label="Claude process usage"
    >
      {items.map((item) => (
        <span className="sysline-item" key={item.key} title={item.title}>
          {(item.key === "cpu" || item.key === "mem") && <Ring size="sm" pct={item.pct} />}
          <span className="sysline-text">{item.text}</span>
        </span>
      ))}
    </div>
  );
}
