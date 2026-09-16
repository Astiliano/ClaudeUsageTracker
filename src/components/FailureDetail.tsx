import { invoke } from "@tauri-apps/api/core";
import type { JSX } from "react";
import { useEffect, useState } from "react";
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
        setData(await invoke<RawSnapshot>("get_snapshot_raw", { snapshotId }));
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    };
    void load();
  }, [snapshotId]);

  return (
    <div className="modal" role="dialog" aria-modal="true">
      <div className="modal-body">
        <h2>Poll failure</h2>
        {error !== null && <p className="error">{error}</p>}
        {data !== null && (
          <>
            <h3>Error</h3>
            <pre>{data.error ?? "(none recorded)"}</pre>
            <h3>Raw output</h3>
            <pre className="raw">{data.raw ?? "(no output captured)"}</pre>
          </>
        )}
        <button type="button" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}
