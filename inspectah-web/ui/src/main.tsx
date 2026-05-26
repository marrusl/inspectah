import React from "react";
import ReactDOM from "react-dom/client";
import "@patternfly/react-core/dist/styles/base.css";
import App from "./App";

function applyTheme() {
  const stored = localStorage.getItem("inspectah-theme");
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const dark = stored === "dark" || (stored === null && prefersDark);
  document.documentElement.classList.toggle("pf-v6-theme-dark", dark);
}

applyTheme();
window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
  if (!localStorage.getItem("inspectah-theme")) applyTheme();
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
