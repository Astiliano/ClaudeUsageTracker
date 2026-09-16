import type { JSX } from "react";
import { backend } from "../lib/backend";
import { bannerFor } from "../lib/banner";
import { accountCountLabel, chipFor } from "../lib/present";
import type { Dashboard } from "../lib/types";

interface Props {
  dashboard: Dashboard;
  settingsOpen: boolean;
  onToggleSettings: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
}

export function Header({ dashboard, settingsOpen, onToggleSettings, onChanged, onError }: Props): JSX.Element {
  const banner = bannerFor(dashboard);
  const chip = chipFor(dashboard);

  const run = async (command: string): Promise<void> => {
    try {
      await backend().invoke(command);
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <header className="header">
      <div className="topbar">
        <div className="topbar-left">
          <h1 className="topbar-title">Usage Tracker</h1>
          <span className="topbar-count">{accountCountLabel(dashboard.accounts.length)}</span>
        </div>
        <div className="topbar-actions">
          <div className="chip" title={banner?.text}>
            <span className={`chip-dot chip-dot-${chip.dot}`} />
            <span className="chip-text">{chip.text}</span>
          </div>
          <button type="button" className="btn" onClick={() => void run("poll_now")} disabled={dashboard.busy}>
            {dashboard.busy ? "refreshing…" : "refresh"}
          </button>
          <button type="button" className={`btn${settingsOpen ? " btn-edit-on" : ""}`} onClick={onToggleSettings}>
            settings
          </button>
        </div>
      </div>
      {banner !== null && banner.tone !== "info" && (
        <div className={`banner banner-${banner.tone}`} role="status">
          <span>{banner.text}</span>
          {banner.action === "clear_halt" && (
            <button type="button" className="btn btn-sm" onClick={() => void run("clear_halt")}>clear halt</button>
          )}
          {banner.action === "open_settings" && !settingsOpen && (
            <button type="button" className="btn btn-sm" onClick={onToggleSettings}>open settings</button>
          )}
        </div>
      )}
    </header>
  );
}
