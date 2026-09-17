import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** The backend primitives the UI uses. Swappable for a browser mock. */
export interface Backend {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: () => void): Promise<() => void>;
  setAlwaysOnTop(flag: boolean): Promise<void>;
}

const real: Backend = {
  invoke: <T>(command: string, args?: Record<string, unknown>): Promise<T> =>
    tauriInvoke<T>(command, args),
  listen: async (event, handler) => {
    const off = await tauriListen(event, () => handler());
    return () => off();
  },
  setAlwaysOnTop: (flag) => getCurrentWindow().setAlwaysOnTop(flag),
};

let current: Backend = real;

export function backend(): Backend {
  return current;
}

/** `VITE_MOCK_BACKEND=1 npm run dev` renders the UI in a plain browser. */
export async function installMockBackendIfRequested(): Promise<void> {
  if (import.meta.env.VITE_MOCK_BACKEND !== "1") return;
  const { createMockBackend } = await import("./mockBackend");
  current = createMockBackend();
  console.warn("usage tracker: mock backend active (VITE_MOCK_BACKEND=1)");
}
