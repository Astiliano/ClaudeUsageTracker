import type { CSSProperties, JSX } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { AccountCard } from "./components/AccountCard";
import { AccountsTable } from "./components/AccountsTable";
import { FailureDetail } from "./components/FailureDetail";
import { Header } from "./components/Header";
import { Settings } from "./components/Settings";
import { useDashboard } from "./hooks/useDashboard";
import { usePrefs } from "./hooks/usePrefs";
import { useSystem } from "./hooks/useSystem";
import { useViewport } from "./hooks/useViewport";
import { backend } from "./lib/backend";
import { moveVisible, visibleColumns } from "./lib/columns";
import { errorMessage } from "./lib/errors";
import { autoHiddenColumns, layoutFor } from "./lib/layout";
import { FONTS, SIZES } from "./lib/theme";
import "./styles.css";

export default function App(): JSX.Element {
  const { dashboard, history, now, cycle, error, refetch } = useDashboard();
  const { report: system, error: systemError } = useSystem();
  const { prefs, update } = usePrefs();
  const viewportWidth = useViewport();
  const zoom = SIZES[prefs.size].zoom;
  const layout = layoutFor(viewportWidth, zoom);
  const effectiveHidden = [...prefs.hiddenColumns, ...autoHiddenColumns(layout)];
  const visible = visibleColumns(prefs.columnOrder, effectiveHidden);
  const [showSettings, setShowSettings] = useState(false);
  const [failureId, setFailureId] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const toastTimer = useRef<number | null>(null);
  // Stable identity: consumers (e.g. Settings) key a load effect off this
  // callback, and App re-renders every second from the `now` tick.
  const showError = useCallback((message: string): void => {
    if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
    setToast(message);
    toastTimer.current = window.setTimeout(() => setToast(null), 6000);
  }, []);
  useEffect(() => () => { if (toastTimer.current !== null) window.clearTimeout(toastTimer.current); }, []);

  useEffect(() => {
    let cancelled = false;
    const apply = async (): Promise<void> => {
      try {
        await backend().setAlwaysOnTop(prefs.alwaysOnTop);
      } catch (e) {
        if (!cancelled) showError(errorMessage(e));
      }
    };
    void apply();
    return () => { cancelled = true; };
  }, [prefs.alwaysOnTop, showError]);

  const font = FONTS[prefs.font];
  // Custom properties are not in CSSProperties; this is the one typed seam.
  const shellStyle = {
    "--ui": font.ui,
    "--mono": font.mono,
    zoom,
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
          compact={layout === "cards"}
          system={system}
          systemError={systemError}
          now={now}
          stayOnTop={prefs.alwaysOnTop}
          onToggleStayOnTop={() => update({ alwaysOnTop: !prefs.alwaysOnTop })}
          onToggleSettings={() => setShowSettings((v) => !v)}
          onChanged={refetch}
          onError={showError}
        />
        {layout === "cards" ? (
          <div className="cards" role="list" aria-label="Accounts">
            {dashboard.accounts.map((row) => (
              <AccountCard key={row.account.id} row={row} now={now} onShowFailure={(id) => setFailureId(id)} />
            ))}
          </div>
        ) : (
          <AccountsTable
            rows={dashboard.accounts}
            history={history}
            now={now}
            cycle={cycle}
            zoom={zoom}
            columnOrder={visible}
            onColumnMove={(from, to) => update({ columnOrder: moveVisible(prefs.columnOrder, effectiveHidden, from, to) })}
            onChanged={refetch}
            onError={showError}
            onShowFailure={(id) => setFailureId(id)}
          />
        )}
        {showSettings && (
          <Settings
            binary={dashboard.binary}
            prefs={prefs}
            layout={layout}
            onPrefs={update}
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
