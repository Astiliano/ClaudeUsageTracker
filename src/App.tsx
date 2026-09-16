import type { JSX } from "react";
import { useState } from "react";
import { AccountsTable } from "./components/AccountsTable";
import { FailureDetail } from "./components/FailureDetail";
import { Header } from "./components/Header";
import { Settings } from "./components/Settings";
import { useDashboard } from "./hooks/useDashboard";
import "./styles.css";

export default function App(): JSX.Element {
  const { dashboard, history, now, error, refetch } = useDashboard();
  const [showSettings, setShowSettings] = useState(false);
  const [failureId, setFailureId] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const showError = (message: string): void => {
    setToast(message);
    window.setTimeout(() => setToast(null), 6000);
  };

  if (dashboard === null) {
    return (
      <main className="app">
        <p>{error ?? "Loading…"}</p>
      </main>
    );
  }

  return (
    <main className="app">
      <Header
        dashboard={dashboard}
        onOpenSettings={() => setShowSettings(true)}
        onChanged={refetch}
        onError={showError}
      />

      <AccountsTable
        rows={dashboard.accounts}
        history={history}
        now={now}
        onChanged={refetch}
        onError={showError}
        onShowFailure={(id) => setFailureId(id)}
      />

      {showSettings && (
        <Settings
          binary={dashboard.binary}
          onClose={() => setShowSettings(false)}
          onChanged={refetch}
          onError={showError}
        />
      )}

      {failureId !== null && (
        <FailureDetail
          snapshotId={failureId}
          onClose={() => setFailureId(null)}
        />
      )}

      {toast !== null && <div className="toast">{toast}</div>}
    </main>
  );
}
