import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/700.css";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// xterm measures the font when a terminal is created, so load the bundled
// terminal font first (with a short cap in case loading stalls).
const fontsReady = Promise.race([
  Promise.all([document.fonts.load('400 14px "JetBrains Mono"'), document.fonts.load('700 14px "JetBrains Mono"')]),
  new Promise((r) => setTimeout(r, 1500)),
]).catch(() => {});

fontsReady.finally(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
});
