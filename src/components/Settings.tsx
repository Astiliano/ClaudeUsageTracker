import type { JSX, KeyboardEvent } from "react";
import { useEffect, useState } from "react";
import { backend } from "../lib/backend";
import { errorMessage } from "../lib/errors";
import type { Prefs } from "../lib/prefs";
import { FONT_KEYS, FONTS, SIZE_KEYS, SIZES } from "../lib/theme";
import type { BinaryInfo, UserSettings } from "../lib/types";
import { Toggle } from "./Toggle";

interface Props {
  binary: BinaryInfo;
  prefs: Prefs;
  onPrefs: (patch: Partial<Prefs>) => void;
  onClose: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
}

export function Settings({
  binary,
  prefs,
  onPrefs,
  onClose,
  onChanged,
  onError,
}: Props): JSX.Element {
  const [settings, setSettings] = useState<UserSettings | null>(null);
  const [intervalDraft, setIntervalDraft] = useState<string>("");
  const [timeoutDraft, setTimeoutDraft] = useState<string>("");
  const [binaryDraft, setBinaryDraft] = useState<string>("");
  const [newPath, setNewPath] = useState<string>("");

  useEffect(() => {
    const load = async (): Promise<void> => {
      try {
        const loaded = await backend().invoke<UserSettings>("get_settings");
        setSettings(loaded);
        setIntervalDraft(String(loaded.interval_secs));
        setTimeoutDraft(String(loaded.timeout_secs));
        setBinaryDraft(loaded.claude_binary);
      } catch (e) {
        onError(errorMessage(e));
      }
    };
    void load();
  }, [onError]);

  if (settings === null) {
    return (
      <section className="panel settings">
        <p className="loading">loading…</p>
      </section>
    );
  }

  // The backend rejects out-of-range values; on rejection the previous value
  // is restored into both settings and whichever draft mirrors it.
  const save = async (next: UserSettings): Promise<void> => {
    const previous = settings;
    setSettings(next);
    try {
      await backend().invoke("set_settings", { settings: next });
      onChanged();
    } catch (e) {
      setSettings(previous);
      setIntervalDraft(String(previous.interval_secs));
      setTimeoutDraft(String(previous.timeout_secs));
      setBinaryDraft(previous.claude_binary);
      onError(errorMessage(e));
    }
  };

  const call = async (
    command: string,
    args: Record<string, unknown> = {},
  ): Promise<void> => {
    try {
      await backend().invoke(command, args);
      onChanged();
    } catch (e) {
      onError(errorMessage(e));
    }
  };

  const commitInterval = (): void => {
    const n = Number(intervalDraft);
    if (Number.isNaN(n)) {
      setIntervalDraft(String(settings.interval_secs));
      return;
    }
    void save({ ...settings, interval_secs: n });
  };
  const onIntervalKeyDown = (e: KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter") commitInterval();
  };

  const commitTimeout = (): void => {
    const n = Number(timeoutDraft);
    if (Number.isNaN(n)) {
      setTimeoutDraft(String(settings.timeout_secs));
      return;
    }
    void save({ ...settings, timeout_secs: n });
  };
  const onTimeoutKeyDown = (e: KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter") commitTimeout();
  };

  const commitBinary = (): void => {
    void save({ ...settings, claude_binary: binaryDraft });
  };
  const onBinaryKeyDown = (e: KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter") commitBinary();
  };

  return (
    <section className="panel settings">
      <div className="settings-head">
        <h2 className="settings-title">Settings</h2>
        <button type="button" className="btn btn-sm btn-ghost" onClick={onClose}>
          close
        </button>
      </div>

      <div className="section">
        <div className="section-label">Typeface</div>
        <div className="choices">
          {FONT_KEYS.map((k) => (
            <button
              key={k}
              type="button"
              className={"choice" + (prefs.font === k ? " choice-on" : "")}
              onClick={() => onPrefs({ font: k })}
            >
              <span className="choice-sample" style={{ fontFamily: FONTS[k].ui }}>
                {FONTS[k].label}
              </span>
              <span className="choice-sub" style={{ fontFamily: FONTS[k].mono }}>
                46% · 1 min ago
              </span>
            </button>
          ))}
        </div>
      </div>

      <div className="section">
        <div className="section-label">Text size</div>
        <div className="choices">
          {SIZE_KEYS.map((k) => (
            <button
              key={k}
              type="button"
              className={"choice choice-size" + (prefs.size === k ? " choice-on" : "")}
              onClick={() => onPrefs({ size: k })}
            >
              {SIZES[k].label}
            </button>
          ))}
        </div>
      </div>

      <div className="divider" />

      <div className="settings-grid">
        <div className="field">
          <div className="section-label">Poll gap</div>
          <div className="field-row">
            <input
              type="number"
              min={10}
              max={3600}
              className="input input-mono input-num"
              value={intervalDraft}
              onChange={(e) => setIntervalDraft(e.target.value)}
              onBlur={commitInterval}
              onKeyDown={onIntervalKeyDown}
            />
            <span className="hint">seconds · 10–3600</span>
          </div>
        </div>
        <div className="field">
          <div className="section-label">Poll timeout</div>
          <div className="field-row">
            <input
              type="number"
              min={5}
              max={120}
              className="input input-mono input-num"
              value={timeoutDraft}
              onChange={(e) => setTimeoutDraft(e.target.value)}
              onBlur={commitTimeout}
              onKeyDown={onTimeoutKeyDown}
            />
            <span className="hint">seconds · 5–120</span>
          </div>
        </div>
      </div>

      <div className="field">
        <div className="section-label">Binary override</div>
        <input
          type="text"
          className="input input-mono"
          style={{ maxWidth: 520 }}
          placeholder="leave blank to auto-detect"
          value={binaryDraft}
          onChange={(e) => setBinaryDraft(e.target.value)}
          onBlur={commitBinary}
          onKeyDown={onBinaryKeyDown}
        />
        <span className="hint">
          detected: {binary.path ?? "none"}
          {binary.source !== null && ` (${binary.source})`}
        </span>
      </div>

      <div className="divider" />

      <div className="toggles">
        <Toggle
          label="Close to tray"
          hint="keep polling in the background"
          checked={settings.close_to_tray}
          onChange={(next) => void save({ ...settings, close_to_tray: next })}
        />
        <Toggle
          label="Launch at login"
          hint="start with the system"
          checked={settings.launch_at_login}
          onChange={(next) => void save({ ...settings, launch_at_login: next })}
        />
        <Toggle
          label="Debug logging"
          hint="verbose logs, pruned after 30 days"
          checked={settings.log_level === "debug"}
          onChange={(next) => void save({ ...settings, log_level: next ? "debug" : "info" })}
        />
      </div>

      <form
        className="accounts-row"
        onSubmit={(e) => {
          e.preventDefault();
          void call("add_account", { configDir: newPath });
          setNewPath("");
        }}
      >
        <input
          type="text"
          className="input input-mono input-grow"
          placeholder="path to a config directory"
          value={newPath}
          onChange={(e) => setNewPath(e.target.value)}
        />
        <button type="submit" className="btn">
          add account
        </button>
        <button type="button" className="btn" onClick={() => void call("rescan_profiles")}>
          rescan profiles
        </button>
        <button type="button" className="btn btn-ghost" onClick={() => void call("open_log_dir")}>
          open logs
        </button>
      </form>
    </section>
  );
}
