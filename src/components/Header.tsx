import { invoke } from "@tauri-apps/api/core";
import type { JSX } from "react";
import { bannerFor } from "../lib/banner";
import type { Dashboard } from "../lib/types";

interface Props {
  dashboard: Dashboard;
  onOpenSettings: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
}

export function Header({
  dashboard,
  onOpenSettings,
  onChanged,
  onError,
}: Props): JSX.Element {
  const banner = bannerFor(dashboard);

  const run = async (command: string): Promise<void> => {
    try {
      await invoke(command);
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <header className="header">
      {banner !== null && (
        <div className={`banner banner-${banner.tone}`} role="status">
          <span>{banner.text}</span>
          {banner.action === "clear_halt" && (
            <button type="button" onClick={() => void run("clear_halt")}>
              Clear halt
            </button>
          )}
          {banner.action === "open_settings" && (
            <button type="button" onClick={onOpenSettings}>
              Open Settings
            </button>
          )}
        </div>
      )}
      <div className="header-actions">
        <button
          type="button"
          onClick={() => void run("poll_now")}
          disabled={dashboard.busy}
        >
          {dashboard.busy ? "Refreshing…" : "Refresh now"}
        </button>
        <button type="button" onClick={onOpenSettings}>
          Settings
        </button>
      </div>
    </header>
  );
}
