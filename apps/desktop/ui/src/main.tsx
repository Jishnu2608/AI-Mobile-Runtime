import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { Preview } from "./Preview";
import "./styles.css";

// `?view=preview` = the docked preview window (`umr-desktop --preview`); the VS Code extension's
// panel sets `window.__UMR_VIEW__`; otherwise the wizard
const preview =
  new URLSearchParams(window.location.search).get("view") === "preview" ||
  (window as unknown as { __UMR_VIEW__?: string }).__UMR_VIEW__ === "preview";

createRoot(document.getElementById("root")!).render(
  <StrictMode>{preview ? <Preview /> : <App />}</StrictMode>,
);
