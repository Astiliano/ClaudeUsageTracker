import type { CSSProperties, JSX } from "react";
import { useEffect, useRef, useState } from "react";
import { AccountsTable } from "./components/AccountsTable";
import { FailureDetail } from "./components/FailureDetail";
import { Header } from "./components/Header";
import { Settings } from "./components/Settings";
import { useDashboard } from "./hooks/useDashboard";
import { usePrefs } from "./hooks/usePrefs";
import { FONTS, SIZES } from "./lib/theme";
import "./styles.css";

export default function App(): JSX.Element {
  const { dashboard, history, now, error, refetch } = useDashboard();
  const { prefs } = usePrefs(); // `update` joins in Task 8
  const [showSettings, setShowSettings] = useState(false);
  const [failureId, setFailureId] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const toastTimer = useRef<number | null>(null);
  const showError = (message: string): void => {
    if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
    setToast(message);
    toastTimer.current = window.setTimeout(() => setToast(null), 6000);
  };
  useEffect(() => () => { if (toastTimer.current !== null) window.clearTimeout(toastTimer.current); }, []);

  const font = FONTS[prefs.font];
  // Custom properties are not in CSSProperties; this is the one typed seam.
  const shellStyle = {
    "--ui": font.ui,
    "--mono": font.mono,
    zoom: SIZES[prefs.size].zoom,
  } as CSSProperties;

  if (dashboard === null) {
    return (
      <main className="app" style={shellStyle}>
        <div className="app-inner"><p className="loading">{error ?? "loading…"}</p></div>
      </main>
    );
  }

  return (
    <main className="app" style={shellStyle}>
      <div className="app-inner">
        <Header
          dashboard={dashboard}
          settingsOpen={showSettings}
          onToggleSettings={() => setShowSettings((v) => !v)}
          onChanged={refetch}
          onError={showError}
        />
        {/* Task 7 state: AccountsTable and Settings keep their CURRENT props.
            Task 8 replaces this AccountsTable call, Task 11 the Settings call. */}
        <AccountsTable
          rows={dashboard.accounts}
          history={history}
          now={now}
          onChanged={refetch}
          onError={showError}
          onShowFailure={(id) => setFailureId(id)}
        />
        <p className="hint">drag a row handle to set failover priority · drag a column header to reorder columns</p>
        {showSettings && (
          <Settings
            binary={dashboard.binary}
            onClose={() => setShowSettings(false)}
            onChanged={refetch}
            onError={showError}
          />
        )}
        {failureId !== null && <FailureDetail snapshotId={failureId} onClose={() => setFailureId(null)} />}
        {toast !== null && <div className="toast" role="status">{toast}</div>}
      </div>
    </main>
  );
}
