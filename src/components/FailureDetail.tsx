import type { JSX, KeyboardEvent } from "react";
import { useEffect, useState } from "react";
import { backend } from "../lib/backend";
import { errorMessage } from "../lib/errors";
import type { RawSnapshot } from "../lib/types";

interface Props {
  snapshotId: number;
  onClose: () => void;
}

export function FailureDetail({ snapshotId, onClose }: Props): JSX.Element {
  const [data, setData] = useState<RawSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const load = async (): Promise<void> => {
      try {
        setData(await backend().invoke<RawSnapshot>("get_snapshot_raw", { snapshotId }));
      } catch (e) {
        setError(errorMessage(e));
      }
    };
    void load();
  }, [snapshotId]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>): void => {
    if (e.key === "Escape") onClose();
  };

  return (
    <div
      className="modal"
      role="dialog"
      aria-modal="true"
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <div className="modal-body">
        <h2 className="settings-title">Poll failure</h2>
        {error !== null && <p className="error">{error}</p>}
        {data !== null && (
          <>
            <h3 className="section-label">Error</h3>
            <pre>{data.error ?? "(none recorded)"}</pre>
            <h3 className="section-label">Raw output</h3>
            <pre className="raw">{data.raw ?? "(no output captured)"}</pre>
          </>
        )}
        <button type="button" className="btn btn-sm btn-ghost" autoFocus onClick={onClose}>
          close
        </button>
      </div>
    </div>
  );
}
