import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { installMockBackendIfRequested } from "./lib/backend";

const container = document.getElementById("root");
if (container === null) {
  throw new Error("missing #root element");
}
const root = ReactDOM.createRoot(container);

installMockBackendIfRequested()
  .catch((e: unknown) => console.warn("mock backend failed to load", e))
  .finally(() => {
    root.render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    );
  });
