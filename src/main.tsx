import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/globals.css";

// The app is not a browser: never show the WebView context menu, in the
// overlay, toolbar, settings modal, or anywhere else. Capture-phase listener
// runs before React handlers and works on Windows (WebView2), macOS (WKWebView)
// and Linux (WebKitGTK).
document.addEventListener(
  "contextmenu",
  (event) => {
    event.preventDefault();
  },
  { capture: true }
);

// Developer tools are unavailable in release builds (the Tauri `devtools`
// feature is not enabled for production). Additionally block the common
// WebView shortcuts as defence in depth, while keeping them in `tauri dev`.
if (!import.meta.env.DEV) {
  document.addEventListener(
    "keydown",
    (event) => {
      const key = event.key.toUpperCase();
      const devtoolsChord =
        key === "F12" ||
        ((event.ctrlKey || event.metaKey) &&
          event.shiftKey &&
          (key === "I" || key === "J" || key === "C")) ||
        (event.metaKey && event.altKey && key === "I") ||
        ((event.ctrlKey || event.metaKey) && key === "U");
      if (devtoolsChord) {
        event.preventDefault();
        event.stopPropagation();
      }
    },
    { capture: true }
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);