import { invoke } from "@tauri-apps/api/core";
import type { JSX } from "react";
import { useEffect, useState } from "react";
import type { BinaryInfo, UserSettings } from "../lib/types";

interface Props {
  binary: BinaryInfo;
  onClose: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
}

export function Settings({
  binary,
  onClose,
  onChanged,
  onError,
}: Props): JSX.Element {
  const [settings, setSettings] = useState<UserSettings | null>(null);
  const [newPath, setNewPath] = useState<string>("");

  useEffect(() => {
    const load = async (): Promise<void> => {
      try {
        setSettings(await invoke<UserSettings>("get_settings"));
      } catch (e) {
        onError(e instanceof Error ? e.message : String(e));
      }
    };
    void load();
  }, [onError]);

  if (settings === null) {
    return <aside className="settings">Loading…</aside>;
  }

  // The backend rejects out-of-range values; on rejection the previous value
  // is kept by simply re-reading what the backend still holds.
  const save = async (next: UserSettings): Promise<void> => {
    const previous = settings;
    setSettings(next);
    try {
      await invoke("set_settings", { settings: next });
      onChanged();
    } catch (e) {
      setSettings(previous);
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  const call = async (
    command: string,
    args: Record<string, unknown> = {},
  ): Promise<void> => {
    try {
      await invoke(command, args);
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <aside className="settings">
      <h2>Settings</h2>

      <label>
        Poll gap (seconds, 10–3600)
        <input
          type="number"
          min={10}
          max={3600}
          value={settings.interval_secs}
          onChange={(e) =>
            void save({ ...settings, interval_secs: Number(e.target.value) })
          }
        />
      </label>

      <label>
        Poll timeout (seconds, 5–120)
        <input
          type="number"
          min={5}
          max={120}
          value={settings.timeout_secs}
          onChange={(e) =>
            void save({ ...settings, timeout_secs: Number(e.target.value) })
          }
        />
      </label>

      <label>
        Claude binary override
        <input
          type="text"
          placeholder="leave blank to auto-detect"
          value={settings.claude_binary}
          onChange={(e) =>
            void save({ ...settings, claude_binary: e.target.value })
          }
        />
      </label>
      <p className="sub">
        detected: {binary.path ?? "none"}
        {binary.source !== null && ` (${binary.source})`}
      </p>

      <label>
        <input
          type="checkbox"
          checked={settings.close_to_tray}
          onChange={(e) =>
            void save({ ...settings, close_to_tray: e.target.checked })
          }
        />
        Close to tray
      </label>

      <label>
        <input
          type="checkbox"
          checked={settings.launch_at_login}
          onChange={(e) =>
            void save({ ...settings, launch_at_login: e.target.checked })
          }
        />
        Launch at login
      </label>

      <label>
        <input
          type="checkbox"
          checked={settings.log_level === "debug"}
          onChange={(e) =>
            void save({
              ...settings,
              log_level: e.target.checked ? "debug" : "info",
            })
          }
        />
        Debug logging
      </label>

      <h3>Accounts</h3>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void call("add_account", { configDir: newPath });
          setNewPath("");
        }}
      >
        <input
          type="text"
          placeholder="path to a config directory"
          value={newPath}
          onChange={(e) => setNewPath(e.target.value)}
        />
        <button type="submit">Add account</button>
      </form>
      <button type="button" onClick={() => void call("rescan_profiles")}>
        Rescan profiles
      </button>

      <h3>Diagnostics</h3>
      <button type="button" onClick={() => void call("open_log_dir")}>
        Open log folder
      </button>
      <p className="sub">History is kept for 30 days and then pruned.</p>

      <button type="button" onClick={onClose}>
        Close
      </button>
    </aside>
  );
}
